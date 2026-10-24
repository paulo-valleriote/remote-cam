//! Virtual camera registration (T031).
//!
//! **NON-PRODUCTION** — F0 risk-reduction spike.
//!
//! Registration has two independent halves, and Q-1 asks which one needs an
//! administrator:
//!
//! 1. the **COM in-process server** for [`VIRTUAL_CAMERA_CLSID`], written to
//!    `HKCU\Software\Classes\CLSID\{...}\InprocServer32` (per-user) or, as the
//!    fallback, `HKLM\Software\Classes\CLSID\{...}\InprocServer32`
//!    (machine-wide, needs elevation);
//! 2. the **virtual camera instance**, created by `MFCreateVirtualCamera` with
//!    [`MFVirtualCameraAccess_CurrentUser`], which needs no administrator.
//!
//! Both registry views are written. A 32-bit consumer reads the
//! `WOW6432Node` view and can only load a 32-bit media source DLL, so the
//! caller passes a separate 32-bit DLL when it has one. Registering only the
//! 64-bit view is a matrix failure for the 32-bit consumer (Q-8), not a
//! limitation of the backend, and [`RegistrationReport`] says so explicitly.
//!
//! The virtual camera lifetime is [`MFVirtualCameraLifetime_Session`]: it
//! disappears when the process that created it exits, which is what a spike
//! needs. A product installer would use `MFVirtualCameraLifetime_System` and
//! remove it on uninstall.

use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaSource, IMFVirtualCamera, MFCreateVirtualCamera, MFVirtualCameraAccess_CurrentUser,
    MFVirtualCameraLifetime_Session, MFVirtualCameraType_SoftwareCameraSource,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS,
    REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::{GUID, HRESULT, PCWSTR, Result as WinResult};

use crate::mf_virtual_camera::VIRTUAL_CAMERA_CLSID;

/// Friendly name shown by consumers next to the camera preview.
pub const FRIENDLY_NAME: &str = "RemoteCam Virtual Camera (Spike)";
/// Value name read by COM when it loads the in-process server.
const THREADING_MODEL: &str = "Both";
/// Registry views a COM server may be registered in.
const KEY_WOW64_64KEY: u32 = 0x0100;
const KEY_WOW64_32KEY: u32 = 0x0200;

/// The registry view a COM registration targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryView {
    /// The native 64-bit view, read by 64-bit consumers.
    Native64,
    /// The `WOW6432Node` view, read by 32-bit consumers.
    Wow32,
}

impl RegistryView {
    fn flag(self) -> REG_SAM_FLAGS {
        match self {
            Self::Native64 => REG_SAM_FLAGS(KEY_WOW64_64KEY),
            Self::Wow32 => REG_SAM_FLAGS(KEY_WOW64_32KEY),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Native64 => "64_bit",
            Self::Wow32 => "32_bit",
        }
    }
}

/// Where each bitness of the media source was registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationReport {
    /// Hives that accepted the in-process server registration.
    pub scope: RegistrationScope,
    /// Views that were written, with the DLL each one points at.
    pub views: Vec<(RegistryView, String)>,
    /// Views that were deliberately skipped because no DLL was supplied.
    pub skipped: Vec<&'static str>,
}

impl RegistrationReport {
    /// A 32-bit consumer can only find the camera when this is true.
    pub fn serves_32_bit(&self) -> bool {
        self.views
            .iter()
            .any(|(view, _)| *view == RegistryView::Wow32)
    }

    /// True when the registration needed no administrator.
    pub fn is_per_user(&self) -> bool {
        self.scope == RegistrationScope::CurrentUser
    }

    /// JSON-shaped record for the harness `run.json` and the Q-1 answer.
    pub fn to_json(&self) -> String {
        let views = self
            .views
            .iter()
            .map(|(view, dll)| {
                format!(
                    "{{\"bitness\": \"{}\", \"dll\": \"{}\"}}",
                    view.label(),
                    escape(dll)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let skipped = self
            .skipped
            .iter()
            .map(|item| format!("\"{item}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{{\"scope\": \"{}\", \"requires_admin\": {}, \"views\": [{views}], \"skipped\": [{skipped}]}}",
            match self.scope {
                RegistrationScope::CurrentUser => "per_user",
                RegistrationScope::Machine => "machine_wide",
            },
            !self.is_per_user(),
        )
    }
}

/// The hive the COM server was registered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationScope {
    /// `HKCU`, no elevation required (preferred; answers Q-1).
    CurrentUser,
    /// `HKLM`, requires an administrator (fallback only).
    Machine,
}

/// A running virtual camera. Dropping it stops and removes the camera.
pub struct VirtualCamera {
    camera: IMFVirtualCamera,
    scope: RegistrationScope,
}

impl VirtualCamera {
    /// The media source behind the registered camera.
    pub fn media_source(&self) -> WinResult<IMFMediaSource> {
        unsafe { self.camera.GetMediaSource() }
    }

    /// The hive this camera's COM server was registered in.
    pub fn scope(&self) -> RegistrationScope {
        self.scope
    }

    /// Stop the camera and remove its per-session registration.
    pub fn remove(mut self) -> WinResult<()> {
        let result = self.stop_inner();
        std::mem::forget(self);
        result
    }

    fn stop_inner(&mut self) -> WinResult<()> {
        unsafe {
            let stopped = self.camera.Stop();
            let removed = self.camera.Remove();
            stopped.and(removed)
        }
    }
}

impl Drop for VirtualCamera {
    fn drop(&mut self) {
        let _ = self.stop_inner();
    }
}

/// Register the COM server per-user only, without creating a camera instance.
///
/// This is the Q-1 measurement: it isolates "does Windows accept a per-user
/// in-process server for this CLSID?" from "can the camera be started?".
pub fn register_com_current_user(
    dll_64: &Path,
    dll_32: Option<&Path>,
) -> WinResult<RegistrationReport> {
    register_inproc_servers(
        HKEY_CURRENT_USER,
        RegistrationScope::CurrentUser,
        dll_64,
        dll_32,
    )
}

/// Register the COM server machine-wide only, without creating a camera.
pub fn register_com_machine_wide(
    dll_64: &Path,
    dll_32: Option<&Path>,
) -> WinResult<RegistrationReport> {
    register_inproc_servers(
        HKEY_LOCAL_MACHINE,
        RegistrationScope::Machine,
        dll_64,
        dll_32,
    )
}

/// Register the COM server per-user and start the virtual camera.
///
/// `dll_64` is the path to the 64-bit `remotecam_injection.dll`; `dll_32` is
/// the optional 32-bit build. Returns an error when the per-user registration
/// is refused, so the caller can record Q-1 rather than silently elevating.
pub fn register_current_user(
    friendly_name: &str,
    dll_64: &Path,
    dll_32: Option<&Path>,
) -> WinResult<(RegistrationReport, VirtualCamera)> {
    let report = register_inproc_servers(
        HKEY_CURRENT_USER,
        RegistrationScope::CurrentUser,
        dll_64,
        dll_32,
    )?;
    let camera = start_camera(friendly_name, RegistrationScope::CurrentUser)?;
    Ok((report, camera))
}

/// Register the COM server machine-wide and start the virtual camera.
///
/// Only used when [`register_current_user`] is refused; `HKLM` requires
/// elevation and the result is recorded in the Q-1 answer.
pub fn register_machine_wide(
    friendly_name: &str,
    dll_64: &Path,
    dll_32: Option<&Path>,
) -> WinResult<(RegistrationReport, VirtualCamera)> {
    let report = register_inproc_servers(
        HKEY_LOCAL_MACHINE,
        RegistrationScope::Machine,
        dll_64,
        dll_32,
    )?;
    let camera = start_camera(friendly_name, RegistrationScope::Machine)?;
    Ok((report, camera))
}

/// Create and start the virtual camera instance without touching the registry.
///
/// The COM server must already be registered for this to succeed.
pub fn start_camera(friendly_name: &str, scope: RegistrationScope) -> WinResult<VirtualCamera> {
    let friendly = wide(friendly_name);
    let source_id = wide(&clsid_string());
    let camera = unsafe {
        MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            MFVirtualCameraLifetime_Session,
            MFVirtualCameraAccess_CurrentUser,
            PCWSTR(friendly.as_ptr()),
            PCWSTR(source_id.as_ptr()),
            None,
        )?
    };
    unsafe { camera.Start(None)? };
    Ok(VirtualCamera { camera, scope })
}

/// Remove both registry views written by this module.
pub fn unregister_current_user() -> WinResult<()> {
    unregister_inproc_servers(HKEY_CURRENT_USER)
}

/// Remove the machine-wide registry views, if they were written.
pub fn unregister_machine_wide() -> WinResult<()> {
    unregister_inproc_servers(HKEY_LOCAL_MACHINE)
}

/// The CLSID as the `{...}` string `MFCreateVirtualCamera` expects.
pub fn clsid_string() -> String {
    format_guid(&VIRTUAL_CAMERA_CLSID)
}

fn format_guid(guid: &GUID) -> String {
    let [a, b, c, d, e, f, g, h] = guid.data4;
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1, guid.data2, guid.data3, a, b, c, d, e, f, g, h
    )
}

fn register_inproc_servers(
    root: HKEY,
    scope: RegistrationScope,
    dll_64: &Path,
    dll_32: Option<&Path>,
) -> WinResult<RegistrationReport> {
    ensure_dll(dll_64)?;
    if let Some(dll) = dll_32 {
        ensure_dll(dll)?;
    }

    let mut views = Vec::new();
    let mut skipped = Vec::new();
    write_view(root, RegistryView::Native64, dll_64)?;
    views.push((
        RegistryView::Native64,
        dll_64.to_string_lossy().into_owned(),
    ));
    match dll_32 {
        Some(dll) => {
            write_view(root, RegistryView::Wow32, dll)?;
            views.push((RegistryView::Wow32, dll.to_string_lossy().into_owned()));
        }
        None => skipped.push("32_bit (no 32-bit DLL was supplied)"),
    }

    Ok(RegistrationReport {
        scope,
        views,
        skipped,
    })
}

fn unregister_inproc_servers(root: HKEY) -> WinResult<()> {
    // The WOW64 view is selected by the *handle*, not by the process, so each
    // view is opened with its own flag and deleted through that handle.
    for view in [RegistryView::Native64, RegistryView::Wow32] {
        let clsid_key = format!("Software\\Classes\\CLSID\\{}", clsid_string());
        let opened = open_key(root, &clsid_key, view)?;
        if let Some(key) = opened {
            let status = unsafe { RegDeleteTreeW(key, PCWSTR(wide("InprocServer32").as_ptr())) };
            unsafe {
                let _ = RegCloseKey(key);
            };
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                return Err(win32_error(status));
            }
        }

        let parent = open_key(root, "Software\\Classes\\CLSID", view)?;
        if let Some(key) = parent {
            let status = unsafe { RegDeleteKeyW(key, PCWSTR(wide(&clsid_string()).as_ptr())) };
            unsafe {
                let _ = RegCloseKey(key);
            };
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                return Err(win32_error(status));
            }
        }
    }
    Ok(())
}

/// Open a subkey for writing in one registry view, or `None` when it is absent.
fn open_key(root: HKEY, subkey: &str, view: RegistryView) -> WinResult<Option<HKEY>> {
    let path = wide(subkey);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(path.as_ptr()),
            None,
            REG_SAM_FLAGS(KEY_WRITE.0 | view.flag().0),
            &mut key,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(Some(key)),
        ERROR_FILE_NOT_FOUND => Ok(None),
        other => Err(win32_error(other)),
    }
}

fn write_view(root: HKEY, view: RegistryView, dll: &Path) -> WinResult<()> {
    let subkey = wide(&format!(
        "Software\\Classes\\CLSID\\{}\\InprocServer32",
        clsid_string()
    ));
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            root,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            REG_SAM_FLAGS(KEY_WRITE.0 | view.flag().0),
            None,
            &mut key,
            None,
        )
    };
    check(status)?;

    let result = (|| {
        set_string(key, None, &dll.to_string_lossy())?;
        set_string(key, Some("ThreadingModel"), THREADING_MODEL)
    })();
    unsafe {
        let _ = RegCloseKey(key);
    };
    result
}

fn set_string(key: HKEY, name: Option<&str>, value: &str) -> WinResult<()> {
    let name = name.map(wide);
    let data = wide(value)
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<u8>>();
    let status = unsafe {
        RegSetValueExW(
            key,
            name.as_deref()
                .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr())),
            None,
            REG_SZ,
            Some(&data),
        )
    };
    check(status)
}

fn ensure_dll(path: &Path) -> WinResult<()> {
    if path.is_file() {
        return Ok(());
    }
    Err(windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.into())
}

fn check(status: WIN32_ERROR) -> WinResult<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32_error(status))
    }
}

fn win32_error(status: WIN32_ERROR) -> windows::core::Error {
    windows::core::Error::from_hresult(HRESULT::from_win32(status.0))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clsid_is_braced_and_uppercase() {
        let clsid = clsid_string();
        assert!(clsid.starts_with('{') && clsid.ends_with('}'));
        assert_eq!(clsid, clsid.to_uppercase());
        assert_eq!(clsid.len(), 38);
    }

    #[test]
    fn the_report_states_whether_32_bit_is_served() {
        let report = RegistrationReport {
            scope: RegistrationScope::CurrentUser,
            views: vec![(RegistryView::Native64, "x.dll".into())],
            skipped: vec!["32_bit (no 32-bit DLL was supplied)"],
        };
        assert!(!report.serves_32_bit());
        assert!(report.is_per_user());
        assert!(report.to_json().contains("\"requires_admin\": false"));
    }
}

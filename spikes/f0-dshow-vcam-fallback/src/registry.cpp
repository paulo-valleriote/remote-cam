// Registration for the DirectShow fallback virtual camera (T034).
//
// **NON-PRODUCTION** — F0 risk-reduction spike. Project-owned code.
//
// `DllRegisterServer` answers the same question `vcam_register` answers for the
// primary backend: can this machine register the camera without an
// administrator? It therefore writes the per-user view and reports refusal
// rather than elevating:
//
//   HKCU\Software\Classes\CLSID\{filter-clsid}                     the filter
//   HKCU\Software\Classes\CLSID\{filter-clsid}\InprocServer32      the DLL
//   HKCU\Software\Classes\CLSID\{video capture category}\Instance\{filter-clsid}
//                                                                  makes it enumerable
//
// Three keys, because DirectShow needs all three: the first two make the COM
// server loadable, the third is what makes a capture application *list* the
// camera. `IFilterMapper2::RegisterFilter` is the documented alternative, but it
// writes to HKCR and therefore to HKLM for a per-machine filter, which is
// exactly what Q-1 asks us to avoid if we can.
//
// NOT COMPILED on the F0 host; see README.md.

#include "dshow_vcam_fallback.h"

#include <strsafe.h>

namespace {

// {860BB310-5D01-11D0-BD3B-00A0C911CE86} — DirectShow's "Video Capture Sources".
static const GUID kVideoCaptureCategory = {
    0x860bb310, 0x5d01, 0x11d0, {0xbd, 0x3b, 0x00, 0xa0, 0xc9, 0x11, 0xce, 0x86}};

const wchar_t* const kThreadingModel = L"Both";

bool WriteRegistryString(HKEY root, const wchar_t* subkey, const wchar_t* name,
                         const wchar_t* value) {
  HKEY key = nullptr;
  // No WOW64 flag: the process bitness already decides the registry view, so a
  // 32-bit build registers under WOW6432Node and a 64-bit build in the native
  // view. Passing both flags is an error on Windows, and passing one explicitly
  // would make the 32-bit build register where no 32-bit consumer looks.
  const LONG created = RegCreateKeyExW(root, subkey, 0, nullptr, REG_OPTION_NON_VOLATILE, KEY_WRITE,
                                       nullptr, &key, nullptr);
  if (created != ERROR_SUCCESS) {
    return false;
  }
  const DWORD bytes = static_cast<DWORD>((wcslen(value) + 1) * sizeof(wchar_t));
  const LONG written =
      RegSetValueExW(key, name, 0, REG_SZ, reinterpret_cast<const BYTE*>(value), bytes);
  RegCloseKey(key);
  return written == ERROR_SUCCESS;
}

bool DeleteRegistryTree(HKEY root, const wchar_t* subkey) {
  const LONG result = RegDeleteTreeW(root, subkey);
  return result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND;
}

void StringFromClsid(const GUID& clsid, wchar_t* buffer, int length) {
  StringFromGUID2(clsid, buffer, length);
}

}  // namespace

STDAPI DllRegisterServer() {
  wchar_t module[MAX_PATH] = {};
  const DWORD written = GetModuleFileNameW(RemoteCamModuleHandle(), module, MAX_PATH);
  if (written == 0 || written >= MAX_PATH) {
    return HRESULT_FROM_WIN32(GetLastError());
  }

  wchar_t clsid_text[64] = {};
  StringFromClsid(REMOTECAM_THIS_CLSID, clsid_text, 64);

  wchar_t filter_key[512] = {};
  HRESULT result = StringCchPrintfW(filter_key, 512, L"Software\\Classes\\CLSID\\%s", clsid_text);
  if (FAILED(result)) {
    return result;
  }
  if (!WriteRegistryString(HKEY_CURRENT_USER, filter_key, nullptr, kFriendlyName)) {
    // Refused: the caller decides whether to try HKLM with elevation. This
    // function never elevates on its own, so the Q-1 answer stays clean.
    return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
  }

  wchar_t inproc_key[512] = {};
  wchar_t instance_key[512] = {};
  wchar_t category_text[64] = {};
  StringFromClsid(kVideoCaptureCategory, category_text, 64);
  if (FAILED(StringCchPrintfW(inproc_key, 512, L"%s\\InprocServer32", filter_key)) ||
      FAILED(StringCchPrintfW(instance_key, 512,
                              L"Software\\Classes\\CLSID\\%s\\Instance\\%s", category_text,
                              clsid_text))) {
    return E_FAIL;
  }

  if (!WriteRegistryString(HKEY_CURRENT_USER, inproc_key, nullptr, module) ||
      !WriteRegistryString(HKEY_CURRENT_USER, inproc_key, L"ThreadingModel", kThreadingModel)) {
    return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
  }

  // The category entry is what makes a consumer enumerate the camera; without
  // it the filter is loadable and invisible.
  if (!WriteRegistryString(HKEY_CURRENT_USER, instance_key, L"CLSID", clsid_text) ||
      !WriteRegistryString(HKEY_CURRENT_USER, instance_key, L"FriendlyName", kFriendlyName)) {
    return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
  }
  return S_OK;
}

STDAPI DllUnregisterServer() {
  wchar_t clsid_text[64] = {};
  wchar_t category_text[64] = {};
  StringFromClsid(REMOTECAM_THIS_CLSID, clsid_text, 64);
  StringFromClsid(kVideoCaptureCategory, category_text, 64);

  wchar_t filter_key[512] = {};
  wchar_t instance_key[512] = {};
  if (FAILED(StringCchPrintfW(filter_key, 512, L"Software\\Classes\\CLSID\\%s", clsid_text)) ||
      FAILED(StringCchPrintfW(instance_key, 512,
                              L"Software\\Classes\\CLSID\\%s\\Instance\\%s", category_text,
                              clsid_text))) {
    return E_FAIL;
  }

  if (!DeleteRegistryTree(HKEY_CURRENT_USER, instance_key) ||
      !DeleteRegistryTree(HKEY_CURRENT_USER, filter_key)) {
    return HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED);
  }
  // The machine-wide entry is left alone: removing something this function did
  // not create would require the elevation it deliberately avoids.
  return S_OK;
}

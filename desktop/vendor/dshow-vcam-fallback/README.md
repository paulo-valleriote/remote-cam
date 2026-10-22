# DirectShow fallback virtual camera (T034)

**NON-PRODUCTION** — F0 risk-reduction spike. Project-owned code, not a vendored
third-party component.

This directory holds the **fallback** injection backend. It is only built when a
primary-backend matrix cell fails, so on a healthy reference machine it is never
compiled at all. `applications.json` records it as `proprietary`, which here
means *owned by this project*: there is no upstream licence file because there
is no upstream.

## What is here

| File | Purpose |
| --- | --- |
| `include/dshow_vcam_fallback.h` | The 32-bit and 64-bit CLSIDs and the filter's public surface |
| `src/dshow_vcam_fallback.cpp` | The filter: a live source filter with one NV12 output pin fed from a shared test-pattern buffer |
| `src/dshow_vcam_fallback.def` | The COM exports (`DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer`) |
| `src/registry.cpp` | Registration through `IFilterMapper2`, and the per-user registry fallback |
| `CMakeLists.txt` | 32-bit and 64-bit builds, each with its own CLSID |

## Status

**Written but not compiled.** The F0 host is Linux/WSL2 and has no MSVC
toolchain and no Windows SDK, so this backend has never been built or measured
(see `spikes/evidence/environment.json` and
`spikes/evidence/decisions/D-10-injection-backend.md`). It is delivered so the
fallback exists as a decision-ready artefact, not as evidence that it works.

Treat every claim in this directory as unverified until the fallback is actually
needed and built on the reference machine.

## Why a DirectShow filter is the fallback at all

The Windows 11 virtual camera API (`MFCreateVirtualCamera`) is the primary
backend because a Media Foundation media source is shared between processes and
declared to Windows in one call. The failure modes that would force the
fallback are:

* a consumer application that only enumerates DirectShow devices (some
  conferencing clients still do);
* a 32-bit consumer on 64-bit Windows that cannot load the 64-bit media source.

A DirectShow capture filter is visible to 32-bit consumers through the
`WOW6432Node` view and to Media Foundation consumers through the DirectShow
bridge, which is why it remains the documented fallback rather than a second
virtual-camera API.

## Building it (on the reference machine, only if needed)

```powershell
# 64-bit
cmake -S desktop/vendor/dshow-vcam-fallback -B build/dshow-vcam-x64 -A x64
cmake --build build/dshow-vcam-x64 --config Release

# 32-bit consumer support
cmake -S desktop/vendor/dshow-vcam-fallback -B build/dshow-vcam-x86 -A Win32
cmake --build build/dshow-vcam-x86 --config Release
```

Register per-user, without elevation:

```powershell
regsvr32 /n /i:user build\dshow-vcam-x64\Release\dshow_vcam_fallback.dll
```

`DllRegisterServer` answers Q-1 for this backend the same way `vcam_register`
does for the primary one: it writes the COM registration under
`HKEY_CURRENT_USER\Software\Classes\CLSID`, and only falls back to `HKLM` when
that write is refused.

## What still has to be done before it can be trusted

1. Build it, then register both bitnesses and list the device from a 32-bit and
   a 64-bit consumer.
2. Replace the in-process test pattern with the live frame handoff: the filter
   currently reads the same committed test pattern as the Media Foundation
   source, because a filter hosted by an application cannot see another
   process's ring buffer.
3. Feed the twenty-minute gate run and record the matrix cells under
   `dshow_filter`.

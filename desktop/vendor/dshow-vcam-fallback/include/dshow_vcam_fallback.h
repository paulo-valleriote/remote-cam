// DirectShow fallback virtual camera — public surface (T034).
//
// **NON-PRODUCTION** — F0 risk-reduction spike. Project-owned code.
//
// The two CLSIDs are deliberately distinct: a 32-bit consumer on 64-bit Windows
// reads the WOW6432Node registry view and can only load a 32-bit DLL, so one
// CLSID could never serve both. A consumer that cannot see the device is a
// matrix failure, not a limitation (Q-8), which is why both builds are
// registered rather than only the one that happens to work.
//
// The CLSIDs are fixed literals, not generated with a GUID tool, so they are
// stable across builds and can be quoted in the evidence.

#pragma once

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <streams.h>

// {8F7C1E20-5B2A-4F6E-9A3D-1C2B3A4D5E70}
static const GUID CLSID_RemoteCamVirtualCamera64 = {
    0x8f7c1e20, 0x5b2a, 0x4f6e, {0x9a, 0x3d, 0x1c, 0x2b, 0x3a, 0x4d, 0x5e, 0x70}};

// {8F7C1E21-5B2A-4F6E-9A3D-1C2B3A4D5E71}
static const GUID CLSID_RemoteCamVirtualCamera32 = {
    0x8f7c1e21, 0x5b2a, 0x4f6e, {0x9a, 0x3d, 0x1c, 0x2b, 0x3a, 0x4d, 0x5e, 0x71}};

// The CLSID this binary registers. A 32-bit DLL must announce the 32-bit CLSID,
// otherwise a consumer would find the registration but load the wrong bitness.
#ifdef _WIN64
#define REMOTECAM_THIS_CLSID CLSID_RemoteCamVirtualCamera64
#else
#define REMOTECAM_THIS_CLSID CLSID_RemoteCamVirtualCamera32
#endif

// The friendly name consumers show next to the picture.
extern const wchar_t* const kFriendlyName;

// Announced media type: NV12, 1280x720, 30 fps. The second type (640x480) is
// announced by the Media Foundation backend; this filter announces one type
// because a fallback that behaves differently from the primary backend is only
// acceptable if the difference is recorded.
#define REMOTECAM_FRAME_WIDTH 1280
#define REMOTECAM_FRAME_HEIGHT 720
#define REMOTECAM_FRAME_RATE 30
#define REMOTECAM_BUFFER_COUNT 4

// The output pin name a consumer matches against.
extern const wchar_t* const kOutputPinName;

// The module handle of this DLL, for `GetModuleFileNameW` during registration.
HMODULE RemoteCamModuleHandle();

// Shared producer: fills a frame into the caller's buffer.
//
// The filter is hosted by the consuming application, which cannot read another
// process's ring buffer, so this fills the committed test pattern. Replacing it
// with the real handoff is item 2 in README.md and belongs to the media phase.
void RemoteCamFillTestPattern(BYTE* destination, DWORD length, unsigned frame_number);

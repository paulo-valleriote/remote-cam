# ADR-0025 — Media Foundation decoder backend, selection and the `unsafe` boundary

- **Status:** accepted
- **Date:** 2026-10-27
- **Relates to:** ADR-0003, ADR-0017, ADR-0019, ADR-0021, REC-4, REC-56

## Context

F3-07 (REC-56) adds the product H.264 decoder for Windows. ADR-0021 left the
`VideoDecoder` seam open for it and made OpenH264 a `cfg(not(windows))` dependency
(patent grant covers only Cisco's binaries). The Windows crate calls Media Foundation
through COM, whose methods are `unsafe`. ADR-0017 named `remotecam-scrub` as the only
crate with `unsafe`.

## Decision

- **Selection is portable.** `decoder_selection::select_decoder(hardware, software)`
  tries the hardware factory first; if it is absent or fails to open, the software
  factory is used. The result carries a `DecoderReport { backend, hardware_probe }`
  (`NotAttempted`, `Passed` or `Failed(reason)`), exposed by `MediaCore::decoder_report`
  for the diagnostics. It runs on Linux/CI with stand-in factories.
- **The empirical check is `open`.** A `DecoderFactory::open` succeeds only when the
  backend produces NV12. `MFT_ENUM_FLAG_HARDWARE` is not used as proof; discovery uses
  `MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER`.
- **Buffer access.** `IMF2DBuffer2::Lock2DSize`; the pitched surface is packed by the
  portable, unit-tested `nv12::pitched_nv12_to_packed`. The GPU accessor goes through
  `MFGetService(MR_BUFFER_SERVICE)` to `IMFDXGIBuffer` and `ID3D11Texture2D`;
  `IMFDXGIBackBuffer` named in the TDD does not exist in the `windows` crate 0.62.2.
- **`unsafe` boundary.** `unsafe` in `media-core` is confined to `mf_decoder`
  (`cfg(windows)`), each block with a `SAFETY` comment. `remotecam-scrub` stays the only
  crate with `unsafe` for memory scrubbing (ADR-0017).
- **No distributed software fallback on Windows.** ADR-0021 still holds: on Windows the
  software factory is `None` unless a licensed decoder is supplied (C-05). Without
  hardware and without a software factory, selection fails with a closed error.

## Consequences

- The Media Foundation module is type-checked for `x86_64-pc-windows-msvc` but cannot run
  in Linux CI; its evidence comes from the Reference Device.
- Coverage of `mf_decoder` is excluded from the Linux gate; the testable parts
  (selection, report, NV12 packing) are covered.

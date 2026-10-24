// Shared test pattern for the DirectShow fallback (T034).
//
// **NON-PRODUCTION** — F0 risk-reduction spike. Project-owned code.
//
// Draws the *same* 20-block marker as
// `spikes/f0-injection/src/test_pattern.rs` and
// `spikes/tools/analysis/marker.py`, so a cell recorded against the fallback is
// judged by the same decoder as a cell recorded against the primary backend:
//
//   * 4-bit sync prefix (0b1010) then the 16-bit frame number, most significant
//     bit first, drawn top-left and again bottom-right;
//   * margin 8 px, block edge min(40, (width - 16) / 20);
//   * set bit = luma 235, clear bit = luma 16, so a consumer's own compression
//     cannot flip a bit at the threshold of 128;
//   * chroma is neutral (128) and the rest of the luma plane is a gradient, so a
//     stalled picture is visibly stalled to a human watching the preview.
//
// NOT COMPILED on the F0 host; see README.md.

#include "dshow_vcam_fallback.h"

namespace {

const int kMargin = 8;
const int kBlockMax = 40;
const int kBits = 16;
const int kSyncBits = 4;
const int kSync = 0b1010;
const int kTotalBits = kSyncBits + kBits;
const BYTE kSet = 235;
const BYTE kClear = 16;

int BlockFor(int width) {
  const int room = (width - 2 * kMargin) / kTotalBits;
  if (room > kBlockMax) {
    return kBlockMax;
  }
  return room > 1 ? room : 1;
}

void FillBlock(BYTE* plane, int stride, int x, int y, int block, BYTE value) {
  for (int row = 0; row < block; ++row) {
    for (int column = 0; column < block; ++column) {
      plane[(y + row) * stride + (x + column)] = value;
    }
  }
}

void DrawMarker(BYTE* luma, int stride, int width, int height, int origin_x, int origin_y,
                int block, unsigned raw) {
  for (int bit = 0; bit < kTotalBits; ++bit) {
    const bool set = ((raw >> (kTotalBits - 1 - bit)) & 1u) != 0;
    const int x = origin_x + bit * block;
    if (x < 0 || x + block > width || origin_y < 0 || origin_y + block > height) {
      continue;
    }
    FillBlock(luma, stride, x, origin_y, block, set ? kSet : kClear);
  }
}

}  // namespace

void RemoteCamFillTestPattern(BYTE* destination, DWORD length, unsigned frame_number) {
  const int width = REMOTECAM_FRAME_WIDTH;
  const int height = REMOTECAM_FRAME_HEIGHT;
  const DWORD expected = static_cast<DWORD>(width * height * 3 / 2);
  if (destination == nullptr || length < expected) {
    return;
  }

  BYTE* luma = destination;
  const int stride = width;
  BYTE* chroma = destination + width * height;

  // The moving gradient makes a frozen picture obvious to an operator's eye,
  // which matters because a human watches the preview during the gate run.
  for (int y = 0; y < height; ++y) {
    const BYTE base = static_cast<BYTE>(16 + ((y + frame_number) * 200) / height);
    for (int x = 0; x < width; ++x) {
      luma[y * stride + x] = base;
    }
  }

  const int block = BlockFor(width);
  const unsigned raw = (static_cast<unsigned>(kSync) << kBits) | (frame_number & 0xFFFFu);

  DrawMarker(luma, stride, width, height, kMargin, kMargin, block, raw);
  DrawMarker(luma, stride, width, height, width - kMargin - block * kTotalBits,
             height - kMargin - block, block, raw);

  // NV12 chroma is interleaved U/V and neutral grey for a colourless picture.
  const DWORD chroma_length = static_cast<DWORD>(width * height / 2);
  for (DWORD index = 0; index < chroma_length; ++index) {
    chroma[index] = 128;
  }
}

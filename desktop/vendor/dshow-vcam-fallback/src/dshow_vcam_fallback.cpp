// DirectShow fallback virtual camera (T034).
//
// **NON-PRODUCTION** — F0 risk-reduction spike. Project-owned code.
//
// A live source filter with one NV12 output pin. A live source is required
// because a virtual camera has no end: the pin keeps producing frames at
// ~30 fps for the whole gate run. The structure follows the DirectShow base
// classes (a `CSource` with one `CSourceStream`), which is the only way a filter
// can be enumerated as a capture device without also shipping a graph builder.
//
// NOT COMPILED on the F0 host: no MSVC toolchain and no Windows SDK are
// available there (see spikes/evidence/environment.json). Written to be built
// when a primary-backend matrix cell fails; see README.md for the build and
// registration commands and for what still has to be finished.

#include "dshow_vcam_fallback.h"

#include <olectl.h>
#include <strsafe.h>

#pragma comment(lib, "winmm.lib")
#pragma comment(lib, "strmiids.lib")

const wchar_t* const kFriendlyName = L"RemoteCam Virtual Camera (Fallback)";
const wchar_t* const kOutputPinName = L"Capture";

namespace {

// 100-nanosecond units, as DirectShow requires.
const LONGLONG kReferenceTimePerSecond = 10000000;
const LONGLONG kFrameDuration = kReferenceTimePerSecond / REMOTECAM_FRAME_RATE;

// A live source must not block the graph, so each frame is paced here rather
// than by a real device clock. The timestamps still advance, which is what makes
// a consumer-side stall visible in the recorded file.
const DWORD kWaitPerFrameMs = 1000 / REMOTECAM_FRAME_RATE;

const DWORD kFrameBytes = REMOTECAM_FRAME_WIDTH * REMOTECAM_FRAME_HEIGHT * 3 / 2;

LONG g_objects = 0;

}  // namespace

class RemoteCamSource;

// ---------------------------------------------------------------------------
// The output pin.
// ---------------------------------------------------------------------------

class RemoteCamStream : public CSourceStream {
 public:
  RemoteCamStream(HRESULT* phr, RemoteCamSource* source);

  HRESULT FillBuffer(IMediaSample* sample) override;
  HRESULT DecideBufferSize(IMemAllocator* allocator, ALLOCATOR_PROPERTIES* requested) override;
  HRESULT GetMediaType(int position, CMediaType* media_type) override;
  HRESULT CheckMediaType(const CMediaType* media_type) override;
  HRESULT OnThreadCreate() override;

 private:
  unsigned frame_number_ = 0;
};

// ---------------------------------------------------------------------------
// The filter.
// ---------------------------------------------------------------------------

class RemoteCamSource : public CSource, public IAMFilterMiscFlags {
 public:
  static HRESULT CreateInstance(IUnknown* outer, REFIID interface_id, void** object);
  DECLARE_IUNKNOWN

  // Both bases declare QueryInterface/AddRef/Release, so the non-delegating
  // versions are pinned to CSource and the extra interface is exposed through
  // NonDelegatingQueryInterface. This is the pattern the DirectShow samples use
  // for a live source filter.
  STDMETHODIMP_(ULONG) NonDelegatingAddRef() override { return CSource::NonDelegatingAddRef(); }
  STDMETHODIMP_(ULONG) NonDelegatingRelease() override { return CSource::NonDelegatingRelease(); }

  STDMETHODIMP NonDelegatingQueryInterface(REFIID interface_id, void** object) override;

  // A virtual camera cannot be seeked, and a consumer uses this flag to decide
  // whether to try.
  STDMETHODIMP_(ULONG) GetMiscFlags() override { return AM_FILTER_MISC_FLAGS_IS_SOURCE; }

 private:
  RemoteCamSource(LPUNKNOWN unknown, HRESULT* phr);

  friend class RemoteCamStream;
};

// ---------------------------------------------------------------------------
// The class factory.
// ---------------------------------------------------------------------------

class RemoteCamClassFactory : public IClassFactory {
 public:
  RemoteCamClassFactory() { InterlockedIncrement(&g_objects); }
  ~RemoteCamClassFactory() { InterlockedDecrement(&g_objects); }

  STDMETHODIMP QueryInterface(REFIID interface_id, void** object) override;
  STDMETHODIMP_(ULONG) AddRef() override { return 2; }
  STDMETHODIMP_(ULONG) Release() override { return 1; }
  STDMETHODIMP CreateInstance(IUnknown* outer, REFIID interface_id, void** object) override;
  STDMETHODIMP LockServer(BOOL lock) override { return S_OK; }
};

// ---------------------------------------------------------------------------
// RemoteCamStream.
// ---------------------------------------------------------------------------

RemoteCamStream::RemoteCamStream(HRESULT* phr, RemoteCamSource* source)
    : CSourceStream(L"Capture", phr, source, kOutputPinName) {}

HRESULT RemoteCamStream::GetMediaType(int position, CMediaType* media_type) {
  // One type only: NV12 1280x720. The Media Foundation backend announces a
  // second size; a difference in announced types is acceptable only if it is
  // recorded in the matrix.
  if (position < 0 || position > 0) {
    return S_FALSE;
  }

  media_type->SetType(&MEDIATYPE_Video);
  media_type->SetSubtype(&MEDIASUBTYPE_NV12);
  media_type->SetFormatType(&FORMAT_VideoInfo);

  VIDEOINFOHEADER* header =
      reinterpret_cast<VIDEOINFOHEADER*>(media_type->AllocFormatBuffer(sizeof(VIDEOINFOHEADER)));
  if (header == nullptr) {
    return E_OUTOFMEMORY;
  }
  ZeroMemory(header, sizeof(VIDEOINFOHEADER));
  header->AvgTimePerFrame = kFrameDuration;
  header->bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
  header->bmiHeader.biWidth = REMOTECAM_FRAME_WIDTH;
  header->bmiHeader.biHeight = REMOTECAM_FRAME_HEIGHT;
  header->bmiHeader.biPlanes = 1;
  header->bmiHeader.biBitCount = 12;
  header->bmiHeader.biCompression = MAKEFOURCC('N', 'V', '1', '2');
  header->bmiHeader.biSizeImage = kFrameBytes;
  media_type->SetSampleSize(kFrameBytes);
  media_type->SetTemporalCompression(FALSE);
  return S_OK;
}

HRESULT RemoteCamStream::CheckMediaType(const CMediaType* media_type) {
  if (media_type->majortype != MEDIATYPE_Video || media_type->subtype != MEDIASUBTYPE_NV12 ||
      media_type->formattype != FORMAT_VideoInfo) {
    return E_INVALIDARG;
  }
  return S_OK;
}

HRESULT RemoteCamStream::DecideBufferSize(IMemAllocator* allocator,
                                         ALLOCATOR_PROPERTIES* requested) {
  ALLOCATOR_PROPERTIES actual;
  requested->cBuffers = REMOTECAM_BUFFER_COUNT;
  requested->cbBuffer = kFrameBytes;
  requested->cbAlign = 1;
  requested->cbPrefix = 0;

  const HRESULT result = allocator->SetProperties(requested, &actual);
  if (FAILED(result)) {
    return result;
  }
  if (actual.cbBuffer < requested->cbBuffer) {
    return E_FAIL;
  }
  return S_OK;
}

HRESULT RemoteCamStream::OnThreadCreate() {
  frame_number_ = 0;
  return CSourceStream::OnThreadCreate();
}

HRESULT RemoteCamStream::FillBuffer(IMediaSample* sample) {
  BYTE* target = nullptr;
  HRESULT result = sample->GetPointer(&target);
  if (FAILED(result)) {
    return result;
  }
  const long length = sample->GetSize();
  if (length <= 0) {
    return E_UNEXPECTED;
  }

  // The filter is hosted by the consuming application, which cannot read
  // another process's ring buffer, so the committed test pattern is produced
  // locally. Replacing this with the real handoff is item 2 in README.md.
  RemoteCamFillTestPattern(target, static_cast<DWORD>(length), frame_number_);

  const REFERENCE_TIME start = static_cast<REFERENCE_TIME>(frame_number_) * kFrameDuration;
  const REFERENCE_TIME stop = start + kFrameDuration;
  sample->SetTime(&start, &stop);
  sample->SetSyncPoint(TRUE);
  sample->SetDiscontinuity(FALSE);
  sample->SetMediaTime(&start, &stop);
  sample->SetActualDataLength(static_cast<long>(kFrameBytes));

  ++frame_number_;
  Sleep(kWaitPerFrameMs);
  return S_OK;
}

// ---------------------------------------------------------------------------
// RemoteCamSource.
// ---------------------------------------------------------------------------

RemoteCamSource::RemoteCamSource(LPUNKNOWN unknown, HRESULT* phr)
    : CSource(L"RemoteCam virtual camera source", unknown, REMOTECAM_THIS_CLSID) {
  // One live pin; the filter owns it and releases it with the filter.
  RemoteCamStream* stream = new RemoteCamStream(phr, this);
  if (stream == nullptr && phr != nullptr) {
    *phr = E_OUTOFMEMORY;
  }
}

HRESULT RemoteCamSource::CreateInstance(IUnknown* outer, REFIID interface_id, void** object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;
  if (outer != nullptr) {
    return CLASS_E_NOAGGREGATION;
  }

  HRESULT result = S_OK;
  RemoteCamSource* source = new RemoteCamSource(nullptr, &result);
  if (source == nullptr) {
    return E_OUTOFMEMORY;
  }
  if (FAILED(result)) {
    delete source;
    return result;
  }
  result = source->NonDelegatingQueryInterface(interface_id, object);
  source->Release();
  return result;
}

STDMETHODIMP RemoteCamSource::NonDelegatingQueryInterface(REFIID interface_id, void** object) {
  if (interface_id == IID_IAMFilterMiscFlags) {
    return GetInterface(static_cast<IAMFilterMiscFlags*>(this), object);
  }
  return CSource::NonDelegatingQueryInterface(interface_id, object);
}

// ---------------------------------------------------------------------------
// RemoteCamClassFactory.
// ---------------------------------------------------------------------------

STDMETHODIMP RemoteCamClassFactory::QueryInterface(REFIID interface_id, void** object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  if (interface_id == IID_IUnknown || interface_id == IID_IClassFactory) {
    *object = static_cast<IClassFactory*>(this);
    return S_OK;
  }
  *object = nullptr;
  return E_NOINTERFACE;
}

STDMETHODIMP RemoteCamClassFactory::CreateInstance(IUnknown* outer, REFIID interface_id,
                                                   void** object) {
  return RemoteCamSource::CreateInstance(outer, interface_id, object);
}

// ---------------------------------------------------------------------------
// DLL entry points. Registration lives in src/registry.cpp.
// ---------------------------------------------------------------------------

static HMODULE g_module = nullptr;

HMODULE RemoteCamModuleHandle() { return g_module; }

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
  if (reason == DLL_PROCESS_ATTACH) {
    g_module = module;
    DisableThreadLibraryCalls(module);
  }
  return TRUE;
}

STDAPI DllGetClassObject(REFCLSID class_id, REFIID interface_id, void** object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;

  const bool is_64 = IsEqualGUID(class_id, CLSID_RemoteCamVirtualCamera64) != FALSE;
  const bool is_32 = IsEqualGUID(class_id, CLSID_RemoteCamVirtualCamera32) != FALSE;
  if (!is_64 && !is_32) {
    return CLASS_E_CLASSNOTAVAILABLE;
  }

  RemoteCamClassFactory* factory = new RemoteCamClassFactory();
  if (factory == nullptr) {
    return E_OUTOFMEMORY;
  }
  const HRESULT result = factory->QueryInterface(interface_id, object);
  factory->Release();
  return result;
}

STDAPI DllCanUnloadNow() { return g_objects == 0 ? S_OK : S_FALSE; }

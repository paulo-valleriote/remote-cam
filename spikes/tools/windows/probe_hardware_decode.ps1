<#
.SYNOPSIS
    Windows hardware-decode probe for the RemoteCam F0 preflight.

.DESCRIPTION
    Calls MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, SYNCMFT | SORTANDFILTER),
    activates every decoder that advertises an NV12 output type, then decodes a
    generated H.264 clip while sampling the GPU Engine utilisation counter.

    Writes a JSON object consumed by spikes/tools/preflight.py. This script is
    part of the measurement apparatus, not product code.

.PARAMETER JsonPath
    Destination for the JSON result.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$JsonPath
)

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

[ComImport, Guid("2cd2d921-c447-44a7-a13c-4adabfc247e3"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMFAttributes
{
    [PreserveSig] int GetItem(ref Guid guidKey, IntPtr pValue);
    [PreserveSig] int GetItemType(ref Guid guidKey, out int pType);
    [PreserveSig] int CompareItem(ref Guid guidKey, IntPtr value, [MarshalAs(UnmanagedType.Bool)] out bool pbResult);
    [PreserveSig] int Compare(IMFAttributes pTheirs, int matchType, [MarshalAs(UnmanagedType.Bool)] out bool pbResult);
    [PreserveSig] int GetUINT32(ref Guid guidKey, out uint punValue);
    [PreserveSig] int GetUINT64(ref Guid guidKey, out ulong punValue);
    [PreserveSig] int GetDouble(ref Guid guidKey, out double pfValue);
    [PreserveSig] int GetGUID(ref Guid guidKey, out Guid pguidValue);
    [PreserveSig] int GetStringLength(ref Guid guidKey, out uint pcchLength);
    [PreserveSig] int GetString(ref Guid guidKey, StringBuilder pwszValue, uint cchBufSize, out uint pcchLength);
    [PreserveSig] int GetAllocatedString(ref Guid guidKey, out IntPtr ppwszValue, out uint pcchLength);
    [PreserveSig] int GetBlobSize(ref Guid guidKey, out uint pcbBlobSize);
    [PreserveSig] int GetBlob(ref Guid guidKey, IntPtr pBuf, uint cbBufSize, out uint pcbBlobSize);
    [PreserveSig] int GetAllocatedBlob(ref Guid guidKey, out IntPtr ppBuf, out uint pcbSize);
    [PreserveSig] int GetUnknown(ref Guid guidKey, ref Guid riid, out IntPtr ppv);
    [PreserveSig] int SetItem(ref Guid guidKey, IntPtr value);
    [PreserveSig] int DeleteItem(ref Guid guidKey);
    [PreserveSig] int DeleteAllItems();
    [PreserveSig] int SetUINT32(ref Guid guidKey, uint unValue);
    [PreserveSig] int SetUINT64(ref Guid guidKey, ulong unValue);
    [PreserveSig] int SetDouble(ref Guid guidKey, double fValue);
    [PreserveSig] int SetGUID(ref Guid guidKey, ref Guid guidValue);
    [PreserveSig] int SetString(ref Guid guidKey, string wszValue);
    [PreserveSig] int SetBlob(ref Guid guidKey, IntPtr pBuf, uint cbBufSize);
    [PreserveSig] int SetUnknown(ref Guid guidKey, [MarshalAs(UnmanagedType.IUnknown)] object pUnknown);
    [PreserveSig] int LockStore();
    [PreserveSig] int UnlockStore();
    [PreserveSig] int GetCount(out uint pcItems);
    [PreserveSig] int GetItemByIndex(uint unIndex, out Guid pguidKey, IntPtr pValue);
    [PreserveSig] int CopyAllItems(IMFAttributes pDest);
}

[ComImport, Guid("44ae0fa8-ea31-4109-8d2e-4cae4997ae7e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMFMediaType : IMFAttributes
{
}

[ComImport, Guid("7fee9e9a-4a89-47a6-899c-b6a53a70fb67"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMFActivate : IMFAttributes
{
    [PreserveSig] int ActivateObject(ref Guid riid, out IntPtr ppv);
    [PreserveSig] int DetachObject();
    [PreserveSig] int ShutdownObject();
}

[ComImport, Guid("bf94c121-5b05-4e6f-8000-ba598961414d"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IMFTransform
{
    [PreserveSig] int GetStreamLimits(out uint inputMin, out uint inputMax, out uint outputMin, out uint outputMax);
    [PreserveSig] int GetStreamCount(out uint inputCount, out uint outputCount);
    [PreserveSig] int GetStreamIDs(uint inputIDArraySize, IntPtr inputIDs, uint outputIDArraySize, IntPtr outputIDs);
    [PreserveSig] int GetInputStreamInfo(uint inputStreamID, IntPtr pStreamInfo);
    [PreserveSig] int GetOutputStreamInfo(uint outputStreamID, IntPtr pStreamInfo);
    [PreserveSig] int GetAttributes(out IMFAttributes pAttributes);
    [PreserveSig] int GetInputStreamAttributes(uint inputStreamID, out IMFAttributes pAttributes);
    [PreserveSig] int GetOutputStreamAttributes(uint outputStreamID, out IMFAttributes pAttributes);
    [PreserveSig] int DeleteInputStream(uint streamID);
    [PreserveSig] int AddInputStreams(uint streams, IntPtr streamIDs);
    [PreserveSig] int GetInputAvailableType(uint inputStreamID, uint typeIndex, out IMFMediaType ppType);
    [PreserveSig] int GetOutputAvailableType(uint outputStreamID, uint typeIndex, out IMFMediaType ppType);
    [PreserveSig] int SetInputType(uint inputStreamID, IMFMediaType pType, uint flags);
    [PreserveSig] int SetOutputType(uint outputStreamID, IMFMediaType pType, uint flags);
    [PreserveSig] int GetInputCurrentType(uint inputStreamID, out IMFMediaType ppType);
    [PreserveSig] int GetOutputCurrentType(uint outputStreamID, out IMFMediaType ppType);
    [PreserveSig] int GetInputStatus(uint inputStreamID, out uint pdwFlags);
    [PreserveSig] int GetOutputStatus(out uint pdwFlags);
    [PreserveSig] int SetOutputBounds(long hnsLowerBound, long hnsUpperBound);
    [PreserveSig] int ProcessEvent(uint inputStreamID, IntPtr pEvent);
    [PreserveSig] int ProcessMessage(int eMessage, IntPtr ulParam);
    [PreserveSig] int ProcessInput(uint inputStreamID, IntPtr pSample, uint flags);
    [PreserveSig] int ProcessOutput(uint flags, uint outputBufferCount, IntPtr pOutputSamples, out uint pdwStatus);
}

public static class MftNative
{
    [DllImport("mfplat.dll", ExactSpelling = true)]
    public static extern int MFStartup(int version, int flags);

    [DllImport("mfplat.dll", ExactSpelling = true)]
    public static extern int MFShutdown();

    [DllImport("mfplat.dll", ExactSpelling = true)]
    public static extern int MFTEnumEx(
        Guid guidCategory,
        uint flags,
        IntPtr inputType,
        IntPtr outputType,
        out IntPtr activates,
        out uint count);
}
"@

$Category = [Guid]"d6c02d4b-6833-45b4-971a-05a4b04bab91"   # MFT_CATEGORY_VIDEO_DECODER
$IidTransform = [Guid]"bf94c121-5b05-4e6f-8000-ba598961414d" # IMFTransform
$FriendlyNameKey = [Guid]"314ffbae-5b41-4c95-9c19-4e7d586face3"
$SubtypeKey = [Guid]"f7e34c9a-42e8-4714-b74b-cb29d72c35e5"  # MF_MT_SUBTYPE
$Nv12 = [Guid]"3231564e-0000-0010-8000-00aa00389b71"        # MFVideoFormat_NV12
$Flags = [uint32]0x41                                       # SYNCMFT | SORTANDFILTER

$result = [ordered]@{
    discovery_method          = "mft_enum_syncmft"
    decoder_found             = $false
    hardware_accelerated      = $false
    output_format_supported   = "other"
    decoders                  = @()
    nv12_decoders             = @()
    gpu_engine_observed       = $false
    gpu_engine_max_pct        = 0.0
    evidence                  = ""
}

$startup = [MftNative]::MFStartup(0x00020070, 0)
if ($startup -ne 0) {
    $result.evidence = "MFStartup failed with HRESULT 0x{0:X8}" -f $startup
    $result | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -Path $JsonPath
    exit 0
}

try {
    $activates = [IntPtr]::Zero
    $count = [uint32]0
    $hr = [MftNative]::MFTEnumEx($Category, $Flags, [IntPtr]::Zero, [IntPtr]::Zero, [ref]$activates, [ref]$count)
    if ($hr -ne 0) {
        $result.evidence = "MFTEnumEx failed with HRESULT 0x{0:X8}" -f $hr
    }
    else {
        $result.decoder_found = ($count -gt 0)
        $names = @()
        $nv12Names = @()
        for ($index = 0; $index -lt $count; $index++) {
            $activatePtr = [System.Runtime.InteropServices.Marshal]::ReadIntPtr($activates, $index * [IntPtr]::Size)
            $activate = [IMFActivate][System.Runtime.InteropServices.Marshal]::GetObjectForIUnknown($activatePtr)
            $namePtr = [IntPtr]::Zero
            $nameLength = [uint32]0
            $name = "unknown"
            try {
                if ($activate.GetAllocatedString([ref]$FriendlyNameKey, [ref]$namePtr, [ref]$nameLength) -eq 0) {
                    $name = [System.Runtime.InteropServices.Marshal]::PtrToStringUni($namePtr)
                }
            }
            finally {
                if ($namePtr -ne [IntPtr]::Zero) {
                    [System.Runtime.InteropServices.Marshal]::FreeCoTaskMem($namePtr)
                }
            }
            $names += $name

            $transformPtr = [IntPtr]::Zero
            if ($activate.ActivateObject([ref]$IidTransform, [ref]$transformPtr) -eq 0) {
                $transform = [IMFTransform][System.Runtime.InteropServices.Marshal]::GetObjectForIUnknown($transformPtr)
                try {
                    for ($typeIndex = 0; $typeIndex -lt 64; $typeIndex++) {
                        $mediaType = $null
                        if ($transform.GetOutputAvailableType(0, [uint32]$typeIndex, [ref]$mediaType) -ne 0) {
                            break
                        }
                        try {
                            $subtype = [Guid]::Empty
                            if ($mediaType.GetGUID([ref]$SubtypeKey, [ref]$subtype) -eq 0 -and $subtype -eq $Nv12) {
                                $nv12Names += $name
                                break
                            }
                        }
                        finally {
                            if ($null -ne $mediaType) {
                                [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($mediaType)
                            }
                        }
                    }
                }
                finally {
                    [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($transform)
                    [System.Runtime.InteropServices.Marshal]::Release($transformPtr)
                }
            }
            [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($activate)
        }
        if ($activates -ne [IntPtr]::Zero) {
            [System.Runtime.InteropServices.Marshal]::FreeCoTaskMem($activates)
        }
        $result.decoders = $names
        $result.nv12_decoders = $nv12Names
        $result.output_format_supported = if ($nv12Names.Count -gt 0) { "nv12" } else { "other" }
    }
}
finally {
    [void][MftNative]::MFShutdown()
}

# GPU-engine counter while a hardware decode is in flight.
$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if ($null -ne $ffmpeg -and $result.decoder_found) {
    $clip = Join-Path $env:TEMP ("remotecam-probe-{0}.mp4" -f $PID)
    try {
        & ffmpeg -y -hide_banner -loglevel error -f lavfi -i "testsrc=size=640x360:rate=30" `
            -t 2 -c:v libx264 -pix_fmt yuv420p $clip 2>$null | Out-Null
        if (Test-Path $clip) {
            $decoder = Start-Process -FilePath $ffmpeg.Source `
                -ArgumentList @("-hide_banner", "-loglevel", "error", "-hwaccel", "d3d11va",
                    "-i", $clip, "-f", "null", "-") `
                -PassThru -NoNewWindow
            $maximum = 0.0
            for ($sample = 0; $sample -lt 8; $sample++) {
                try {
                    $samples = (Get-Counter '\GPU Engine(*)\Utilization Percentage' -ErrorAction Stop).CounterSamples
                    if ($samples.Count -gt 0) {
                        $value = ($samples | Measure-Object -Property CookedValue -Maximum).Maximum
                        if ($value -gt $maximum) { $maximum = $value }
                    }
                }
                catch {
                    break
                }
                Start-Sleep -Milliseconds 250
            }
            if (-not $decoder.HasExited) {
                $decoder.Kill()
            }
            $result.gpu_engine_max_pct = [math]::Round($maximum, 2)
            $result.gpu_engine_observed = ($maximum -gt 0.0)
        }
    }
    catch {
        $result.gpu_engine_observed = $false
    }
    finally {
        Remove-Item -Path $clip -ErrorAction SilentlyContinue
    }
}

$result.hardware_accelerated = (
    $result.decoder_found -and
    $result.output_format_supported -eq "nv12" -and
    $result.gpu_engine_observed
)

$decoderText = if ($result.decoders.Count -gt 0) { $result.decoders -join "; " } else { "none" }
$result.evidence = (
    "MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, SYNCMFT|SORTANDFILTER) -> {0}; " +
    "NV12 output: {1}; GPU Engine(*)\\Utilization Percentage max {2}% over 2 s decode"
) -f $decoderText, $result.output_format_supported, $result.gpu_engine_max_pct

$result | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -Path $JsonPath

# NON-PRODUCTION - F0 virtual audio signature probe (T102).
#
# Verifies the pinned VB-CABLE driver on a Secure-Boot-enabled Windows 11
# runner. Both checks are required by the gate:
#
#   * Get-AuthenticodeSignature on every .sys/.cat file;
#   * signtool verify /pa /v on the same files.
#
# A "Valid" Authenticode status alone is not enough: `signtool` performs the
# kernel-mode policy check. The script exits 0 only when every file passes both.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File signature_probe.ps1 `
#       -DriverPath "C:\Program Files\VB\CABLE" `
#       -OutFile "spikes\evidence\audio\aud-01\signature_probe.json"

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$DriverPath,

    [string]$SignTool = "signtool.exe",

    [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $DriverPath)) {
    Write-Error "driver path not found: $DriverPath"
    exit 2
}

$item = Get-Item -LiteralPath $DriverPath
if ($item.PSIsContainer) {
    $files = Get-ChildItem -LiteralPath $DriverPath -Recurse -File |
        Where-Object { $_.Extension -in @(".sys", ".cat", ".inf") } |
        Sort-Object FullName
} else {
    $files = @($item)
}

if ($files.Count -eq 0) {
    Write-Error "no .sys, .cat or .inf files under $DriverPath"
    exit 2
}

$secureBoot = $null
try {
    $secureBoot = Confirm-SecureBootUEFI
} catch {
    $secureBoot = $null
}

$results = foreach ($file in $files) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    $verifyOutput = & $SignTool verify /pa /v $file.FullName 2>&1
    $verifyExit = $LASTEXITCODE
    [pscustomobject]@{
        path          = $file.FullName
        sha256        = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
        status        = "$($signature.Status)"
        status_message = "$($signature.StatusMessage)"
        signer        = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { "" }
        verify_passed = ($verifyExit -eq 0)
        verify_exit   = $verifyExit
        verify_output = ($verifyOutput | Out-String).Trim()
    }
}

$allValid = -not ($results | Where-Object { -not $_.verify_passed -or $_.status -ne "Valid" })

$report = [pscustomobject]@{
    device_pin          = "VB-CABLE"
    device_version      = "driver pack 45"
    captured_at         = (Get-Date).ToString("o")
    host                = $env:COMPUTERNAME
    secure_boot_enabled = $secureBoot
    signtool            = $SignTool
    all_valid           = $allValid
    files               = @($results)
}

$json = $report | ConvertTo-Json -Depth 6
if ($OutFile) {
    $parent = Split-Path -Parent $OutFile
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $json | Set-Content -LiteralPath $OutFile -Encoding UTF8
    Write-Output "wrote $OutFile (all_valid=$allValid)"
} else {
    Write-Output $json
}

if ($allValid) { exit 0 } else { exit 1 }

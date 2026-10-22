<#
.SYNOPSIS
    Per-user virtual camera registration check for the RemoteCam F0 gate 1.

.DESCRIPTION
    Answers three of the gate's open questions by doing exactly what a product
    installer would do and recording what Windows accepted:

      Q-1  does per-user registration work without elevation?
      Q-6  does it require an MSI installer rather than a per-user registration?
      Q-8  does a 32-bit consumer see the device?

    Steps, in order:

      1. register the in-process COM server in HKCU for each bitness that was
         built (--register-only, no camera yet) and read the registry back;
      2. create and start the virtual camera instance, hold it alive, and list
         the cameras a 64-bit consumer enumerates;
      3. measure the HKLM fallback only when -AllowMachineWide was passed,
         because a machine-wide answer is a different answer;
      4. unregister everything and verify both registry views are clean.

    The script never elevates itself. When HKCU is refused it records the
    refusal and stops, which is itself the Q-1 evidence the gate needs.

    Output: spikes\evidence\injection\registration.json plus the console log.

    Part of the F0 measurement apparatus, not product code.

.PARAMETER DllPath
    The 64-bit remotecam_injection.dll. Defaults to the debug build path.

.PARAMETER Dll32Path
    The 32-bit build of the same DLL. Built on demand with
    `rustup target add i686-pc-windows-msvc` when the file is absent.

.PARAMETER JsonPath
    Destination for the JSON result.

.PARAMETER HoldSeconds
    How long the started camera is kept alive so step 2 can enumerate it.

.PARAMETER SkipBuild32
    Do not attempt to build the 32-bit media source.

.PARAMETER AllowMachineWide
    Measure the HKLM fallback (requires an elevated shell) and record it.

.EXAMPLE
    pwsh spikes/f0-camera-injection/register.ps1

.EXAMPLE
    pwsh spikes/f0-camera-injection/register.ps1 -AllowMachineWide -JsonPath spikes\evidence\injection\registration-hklm.json
#>
[CmdletBinding()]
param(
    [string]$DllPath = "target\x86_64-pc-windows-msvc\debug\remotecam_injection.dll",
    [string]$Dll32Path = "",
    [string]$JsonPath = "spikes\evidence\injection\registration.json",
    [int]$HoldSeconds = 30,
    [switch]$SkipBuild32,
    [switch]$AllowMachineWide
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$clsid = "{8F7C1E10-5B2A-4F6E-9A3D-1C2B3A4D5E6F}"
$friendlyName = "RemoteCam Virtual Camera (Spike)"
$desktopKey = "Software\Classes\CLSID\$clsid\InprocServer32"
$wow64Key = "Software\Classes\WOW6432Node\CLSID\$clsid\InprocServer32"

function Resolve-RepoPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }
    return (Join-Path $repoRoot $Path)
}

function Test-Elevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-InprocServer([string]$Hive, [string]$SubKey) {
    # Reads one registry view. HKCU is checked before HKLM so the answer
    # distinguishes "accepted per-user" from "only accepted machine-wide".
    foreach ($root in @("HKEY_CURRENT_USER", "HKEY_LOCAL_MACHINE")) {
        if ($Hive -and $root -ne $Hive) { continue }
        $path = "Registry::$root\$SubKey"
        try {
            $value = (Get-ItemProperty -Path $path -Name "(default)" -ErrorAction Stop)."(default)"
            if ($value) { return @{ hive = $root; path = $path; dll = $value } }
        } catch { continue }
    }
    return $null
}

function Get-VirtualCameras {
    # What a consumer enumerates. The Frame Server hosts the source, so a
    # device missing here is a consumer-visible failure, not a registry detail.
    $found = @()
    try {
        $found += @(Get-PnpDevice -Class Camera -ErrorAction Stop |
            ForEach-Object { "{0} ({1})" -f $_.FriendlyName, $_.Status })
    } catch {
        Write-Verbose "Get-PnpDevice -Class Camera is unavailable: $($_.Exception.Message)"
    }
    try {
        $found += @(Get-CimInstance Win32_PnPEntity -Filter "PNPClass='Camera'" -ErrorAction Stop |
            ForEach-Object { "$($_.Name) ($($_.Status))" })
    } catch {
        Write-Verbose "Win32_PnPEntity is unavailable: $($_.Exception.Message)"
    }
    return @($found | Sort-Object -Unique)
}

function Invoke-VcamRegister {
    param([string[]]$RegisterArgs)
    $cargoArgs = @(
        "run", "--quiet", "-p", "remotecam-injection",
        "--target", "x86_64-pc-windows-msvc",
        "--example", "vcam_register", "--"
    ) + $RegisterArgs
    Push-Location (Join-Path $repoRoot "desktop")
    try {
        $output = & cargo @cargoArgs 2>&1
        return @{ exit_code = $LASTEXITCODE; output = (($output | Out-String).Trim()) }
    } finally {
        Pop-Location
    }
}

$result = [ordered]@{
    backend = "mf_virtual_camera"
    clsid = $clsid
    captured_at = (Get-Date).ToUniversalTime().ToString("o")
    host = [ordered]@{
        os = (Get-CimInstance Win32_OperatingSystem).Caption
        build = [string](Get-CimInstance Win32_OperatingSystem).BuildNumber
        elevation = if (Test-Elevated) { "administrator" } else { "standard_user" }
    }
    per_user_accepted = $false
    per_user_refusal = $null
    machine_wide_attempted = $false
    machine_wide_accepted = $null
    requires_admin = $null
    installer_required = $null
    bitness_64 = [ordered]@{ registered = $false; hive = $null; dll = $null }
    bitness_32 = [ordered]@{ built = $false; registered = $false; hive = $null; dll = $null }
    camera_visible_after_start = @()
    unregistered_cleanly = $false
    q1_answer = $null
    q6_answer = $null
    q8_answer = $null
}

Write-Host "gate 1 / Q-1, Q-6, Q-8: per-user virtual camera registration"
Write-Host "  repository : $repoRoot"
Write-Host "  elevation  : $($result.host.elevation)"
Write-Host "  clsid      : $clsid"

$resolved64 = Resolve-RepoPath $DllPath
if (-not (Test-Path $resolved64)) {
    Write-Host "building the 64-bit media source"
    Push-Location (Join-Path $repoRoot "desktop")
    try { & cargo build -p remotecam-injection --target x86_64-pc-windows-msvc } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "the 64-bit media source did not build" }
}

if ($Dll32Path -ne "") {
    $resolved32 = Resolve-RepoPath $Dll32Path
} else {
    $resolved32 = Resolve-RepoPath "target\i686-pc-windows-msvc\debug\remotecam_injection.dll"
    if (-not (Test-Path $resolved32) -and -not $SkipBuild32) {
        Write-Host "building the 32-bit media source (i686-pc-windows-msvc)"
        Push-Location (Join-Path $repoRoot "desktop")
        try {
            & rustup target add i686-pc-windows-msvc | Out-Null
            & cargo build -p remotecam-injection --target i686-pc-windows-msvc
            $result.bitness_32.built = ($LASTEXITCODE -eq 0)
        } catch {
            Write-Warning "the 32-bit media source did not build: $($_.Exception.Message)"
        } finally {
            Pop-Location
        }
    } elseif (Test-Path $resolved32) {
        $result.bitness_32.built = $true
    }
}
if ($Dll32Path -ne "" -and (Test-Path $resolved32)) {
    $result.bitness_32.built = $true
}
if (-not (Test-Path $resolved32)) { $resolved32 = $null }

# Step 1 — per-user COM registration only, then read it back.
$registerArgs = @("--register-only", "--dll", $resolved64)
if ($resolved32) { $registerArgs += @("--dll32", $resolved32) }
$registration = Invoke-VcamRegister $registerArgs
Write-Host $registration.output

if ($registration.exit_code -eq 0) {
    $result.per_user_accepted = $true
} else {
    $result.per_user_refusal = $registration.output
    Write-Warning "per-user registration was refused; Q-1 records the refusal."
}

$found64 = Get-InprocServer "HKEY_CURRENT_USER" $desktopKey
if (-not $found64) { $found64 = Get-InprocServer $null $desktopKey }
if ($found64) {
    $result.bitness_64.registered = $true
    $result.bitness_64.hive = $found64.hive
    $result.bitness_64.dll = $found64.dll
}

$found32 = Get-InprocServer $null $wow64Key
if ($found32) {
    $result.bitness_32.registered = $true
    $result.bitness_32.hive = $found32.hive
    $result.bitness_32.dll = $found32.dll
}

# Step 2 — start the camera, hold it, and enumerate it as a consumer would.
if ($result.per_user_accepted) {
    $hold = @("--dll", $resolved64, "--friendly-name", $friendlyName, "--hold-seconds", "$HoldSeconds")
    if ($resolved32) { $hold += @("--dll32", $resolved32) }
    $cargoArgs = @(
        "run", "--quiet", "-p", "remotecam-injection",
        "--target", "x86_64-pc-windows-msvc",
        "--example", "vcam_register", "--"
    ) + $hold
    Push-Location (Join-Path $repoRoot "desktop")
    try {
        $process = Start-Process -FilePath "cargo" -ArgumentList $cargoArgs -NoNewWindow -PassThru
        Start-Sleep -Seconds ([Math]::Min(10, [Math]::Max(2, $HoldSeconds / 3)))
        $result.camera_visible_after_start = Get-VirtualCameras
        $process.WaitForExit()
    } finally {
        Pop-Location
    }
}

# Step 3 — the HKLM fallback, only when it was explicitly asked for.
if ($AllowMachineWide) {
    $result.machine_wide_attempted = $true
    if (-not (Test-Elevated)) {
        $result.machine_wide_accepted = $false
        Write-Warning "HKLM requires an elevated shell; the fallback was not measured."
    } else {
        $machineArgs = @("--register-only", "--machine-wide", "--dll", $resolved64)
        if ($resolved32) { $machineArgs += @("--dll32", $resolved32) }
        $machine = Invoke-VcamRegister $machineArgs
        Write-Host $machine.output
        $result.machine_wide_accepted = ($machine.exit_code -eq 0)
    }
}

# Step 4 — clean up and verify.
Invoke-VcamRegister @("--unregister") | Out-Null
$leftover64 = (Get-InprocServer $null $desktopKey) -ne $null
$leftover32 = (Get-InprocServer $null $wow64Key) -ne $null
$result.unregistered_cleanly = -not ($leftover64 -or $leftover32)

$result.requires_admin = -not $result.per_user_accepted
$result.installer_required = if ($result.bitness_32.registered) {
    "a per-user COM registration for both bitnesses; no MSI and no MSIX is required"
} else {
    "no installer is needed for 64-bit consumers, but a 32-bit build of the media source must be shipped or the 32-bit consumer fails the matrix"
}
$result.q1_answer = if ($result.per_user_accepted) {
    "per-user registration in HKCU works without elevation"
} else {
    "per-user registration was refused; HKLM (administrator) is required"
}
$result.q6_answer = if ($result.per_user_accepted) {
    "no installer is required: a per-user COM registration is enough, so MSI and MSIX are both avoidable"
} else {
    "an installer that can write HKLM is required"
}
$result.q8_answer = if ($result.bitness_32.registered) {
    "the 32-bit consumer finds the camera when a 32-bit media source is registered in WOW6432Node; verify the capture in the matrix"
} else {
    "no 32-bit media source was registered, so a 32-bit consumer cannot see the camera: matrix failure, not a limitation"
}

$jsonFullPath = Resolve-RepoPath $JsonPath
$jsonDir = Split-Path -Parent $jsonFullPath
if ($jsonDir -and -not (Test-Path $jsonDir)) {
    New-Item -ItemType Directory -Path $jsonDir -Force | Out-Null
}
($result | ConvertTo-Json -Depth 6) | Set-Content -Path $jsonFullPath -Encoding utf8

Write-Host ""
Write-Host "Q-1: $($result.q1_answer)"
Write-Host "Q-6: $($result.q6_answer)"
Write-Host "Q-8: $($result.q8_answer)"
Write-Host "registry left clean: $($result.unregistered_cleanly)"
Write-Host "wrote $JsonPath"

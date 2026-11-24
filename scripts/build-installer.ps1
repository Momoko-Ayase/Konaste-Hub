#Requires -Version 5.1
<#
.SYNOPSIS
  Build the native GPUI Hub release executable and (optionally) its Inno Setup
  installer.

.DESCRIPTION
  The script builds with cargo, stages the release files, computes SHA-256
  digests, and can compile installer/konaste-hub.iss with a locally installed
  Inno Setup 6.3+ compiler (any newer major works). It never downloads tools, never publishes, never
  signs, and never touches game installations.

  The native Hub's release build is the workspace binary target
  `konaste-hub` from crates/konaste-hub. Use `-SkipBuild` to package an existing
  target/release/konaste-hub.exe.

.PARAMETER Version
  Overrides the workspace version from Cargo.toml.

.PARAMETER Output
  Output folder for staged files. Default: dist/.

.PARAMETER SkipBuild
  Do not run cargo; only stage and package.

.PARAMETER Exe
  Release executable to package. Default: target/release/konaste-hub.exe.

.PARAMETER Installer
  Also compile the Inno Setup installer with ISCC.exe.

.PARAMETER Iscc
  Explicit path to ISCC.exe. Default: search common install folders and PATH.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Installer -Iscc "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Output = 'dist',
    [switch]$SkipBuild,
    [string]$Exe,
    [switch]$Installer,
    [string]$Iscc
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$release = Join-Path $repo 'target\release'
if (-not $Exe) { $Exe = Join-Path $release 'konaste-hub.exe' }
elseif (-not [System.IO.Path]::IsPathRooted($Exe)) { $Exe = Join-Path $repo $Exe }
$dist = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $repo $Output }

function Get-WorkspaceVersion {
    $toml = Get-Content -Raw (Join-Path $repo 'Cargo.toml')
    $section = [regex]::Match($toml, '(?m)^\[workspace\.package\]\s*$')
    if (-not $section.Success) { throw 'Cargo.toml has no [workspace.package] section.' }
    $after = $toml.Substring($section.Index + $section.Length)
    $next = [regex]::Match($after, '(?m)^\[')
    if ($next.Success) { $after = $after.Substring(0, $next.Index) }
    $match = [regex]::Match($after, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'Cargo.toml has no workspace version.' }
    return $match.Groups[1].Value
}

if (-not $Version) { $Version = Get-WorkspaceVersion }
# Inno Setup VersionInfoVersion takes up to four numeric components. Pad the
# workspace triple; reject anything else rather than guessing a meaning.
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version '$Version' is not numeric (expected MAJOR.MINOR.PATCH, for example 0.1.0)."
}
$VersionInfoVersion = "$Version.0"

if (-not $SkipBuild) {
    Write-Host "Building konaste-hub $Version (release, x64)..."
    Push-Location $repo
    try {
        # Name the Hub binary explicitly.
        # --locked refuses an unrecorded dependency graph.
        & cargo build --release --locked -j 2 -p konaste-hub --bin konaste-hub
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE." }
    }
    finally { Pop-Location }
}

if (-not (Test-Path $Exe)) {
    throw "Missing $Exe. Run without -SkipBuild, pass -Exe, or build the native Hub first."
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null

$files = @(
    @{ Source = $Exe; Name = 'konaste-hub.exe' },
    @{ Source = (Join-Path $repo 'LICENSE'); Name = 'LICENSE' },
    @{ Source = (Join-Path $repo 'THIRD_PARTY_NOTICES.md'); Name = 'THIRD_PARTY_NOTICES.md' }
)

foreach ($file in $files) {
    if (-not (Test-Path $file.Source)) { throw "Missing release input $($file.Source)." }
    $target = Join-Path $dist $file.Name
    # The installer job stages an already-staged binary with -Exe dist/...
    # Copy-Item refuses to copy a file onto itself.
    $sourcePath = (Resolve-Path $file.Source).ProviderPath
    $targetPath = [System.IO.Path]::GetFullPath($target)
    if (-not [string]::Equals($sourcePath, $targetPath, [System.StringComparison]::OrdinalIgnoreCase)) {
        Copy-Item -Force $sourcePath $targetPath
    }
}

# Get-FileHash lives in a PowerShell module that is absent on some trimmed
# Windows images. .NET hashing works everywhere this script can run.
function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try { return ([System.BitConverter]::ToString($sha.ComputeHash($stream)) -replace '-', '').ToLowerInvariant() }
        finally { $sha.Dispose() }
    }
    finally { $stream.Dispose() }
}

# The executable must not depend on a sidecar WebView2 loader: the crate links
# WebView2LoaderStatic. Fail loudly if a stray DLL appears next to the binary
# being packaged, not in some unrelated target profile.
$stray = Get-ChildItem (Split-Path -Parent $Exe) -Filter 'WebView2Loader.dll' -ErrorAction SilentlyContinue
if ($stray) { throw "Unexpected sidecar $($stray.FullName); the expected build links WebView2LoaderStatic." }

$manifest = [ordered]@{
    product = 'Konaste Hub'
    version = $Version
    builtAtUtc = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    target = 'x86_64-pc-windows-msvc'
    files = @()
}
foreach ($file in $files) {
    $path = Join-Path $dist $file.Name
    $hash = Get-Sha256 $path
    $manifest.files += [ordered]@{ name = $file.Name; sha256 = $hash; bytes = (Get-Item $path).Length }
    Write-Host ("{0,-24} {1}  {2:N0} bytes" -f $file.Name, $hash, (Get-Item $path).Length)
}
# ASCII without a BOM: BOM-prefixed checksum files break simple verifiers.
function Write-Ascii([string]$Path, [string]$Text) {
    [System.IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding($false)))
}
function Write-Manifests {
    Write-Ascii (Join-Path $dist 'SHA256SUMS.json') ($manifest | ConvertTo-Json -Depth 5)
    $lines = $manifest.files | ForEach-Object { "$($_.sha256)  $($_.name)" }
    Write-Ascii (Join-Path $dist 'SHA256SUMS.txt') (($lines -join "`n") + "`n")
}
Write-Manifests

if (-not $Installer) {
    Write-Host "Staged release files in $dist. Installer not requested."
    exit 0
}

if (-not $Iscc) {
    # Newest "Inno Setup <major>" folder first, then PATH.
    $roots = @(${env:ProgramFiles(x86)}, $env:ProgramFiles, (Join-Path $env:LOCALAPPDATA 'Programs')) | Where-Object { $_ }
    $Iscc = $roots |
        ForEach-Object { Get-ChildItem -Path $_ -Directory -Filter 'Inno Setup *' -ErrorAction SilentlyContinue } |
        Sort-Object { [int]($_.Name -replace '\D', '') } -Descending |
        ForEach-Object { Join-Path $_.FullName 'ISCC.exe' } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1
    if (-not $Iscc) { $Iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
}
if (-not $Iscc -or -not (Test-Path $Iscc)) {
    throw 'ISCC.exe not found. Install Inno Setup 6.3+ or pass -Iscc. No download is attempted.'
}

Write-Host "Compiling installer with $Iscc..."
$isp = Join-Path $repo 'installer\konaste-hub.iss'
& $Iscc "/DAppVersion=$Version" "/DVersionInfoVersion=$VersionInfoVersion" "/DSourceExe=$Exe" "/DOutputDir=$dist" "/DRepoRoot=$repo" $isp
if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE." }

$setup = Get-ChildItem $dist -Filter "KonasteHub-$Version-setup.exe" | Select-Object -First 1
if (-not $setup) { throw 'ISCC reported success but no setup executable was found.' }
$setupHash = Get-Sha256 $setup.FullName
$manifest.files += [ordered]@{ name = $setup.Name; sha256 = $setupHash; bytes = $setup.Length }
Write-Manifests
Write-Host ("{0,-40} {1}  {2:N0} bytes" -f $setup.Name, $setupHash, $setup.Length)
Write-Host "Installer written to $($setup.FullName). It is unsigned."

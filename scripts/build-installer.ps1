# Builds the Zepi installer: stages the repo + build + payload into dist, then runs Inno Setup.
# Nothing is taken from a working bundle: dist\staging is rebuilt from its sources every time.
# -Zip packs the same staging as dist\zepi-<ver>-portable.zip for machines without admin rights.
param(
    [string]$Version = '',
    [string]$Repo = '',
    [string]$Exe = '',
    [string]$Payload = '',
    [switch]$SkipStaging,
    [switch]$KeepStaging,
    [switch]$Zip
)
$ErrorActionPreference = 'Stop'
# Paths are derived from this script's location, so nothing machine-specific is committed.
if (-not $Repo) { $Repo = Split-Path $PSScriptRoot -Parent }
if (-not $Version) {
    # Single source of truth: PRODUCT_VERSION in zepi_runtime.rs (same value shown in About).
    $rt = Get-Content (Join-Path $Repo 'native\overlay\crates\zed\src\zepi_runtime.rs') -Raw
    $m = [regex]::Match($rt, 'PRODUCT_VERSION:\s*&str\s*=\s*"([^"]+)"')
    if ($m.Success) { $Version = $m.Groups[1].Value }
    else { throw 'cannot read PRODUCT_VERSION from zepi_runtime.rs; pass -Version explicitly' }
}
$buildRoot = Join-Path (Split-Path $Repo -Parent) 'zepi-build'
if (-not $Exe) {
    # Prefer the compact installer profile when built; dev release-fast stays full-debug.
    $stripped = Join-Path $buildRoot 'release-installer\zepi.exe'
    $Exe = if (Test-Path $stripped) { $stripped } else { Join-Path $buildRoot 'release-fast\zepi.exe' }
}
if (-not $Payload) { $Payload = Join-Path $Repo '.local\payload' }
$staging = Join-Path $Repo 'dist\staging'
$dist = Join-Path $Repo 'dist'

# Sources: build output, the repository itself, and the runtime payload (node/python).
# The agent is a single bun binary (agent.exe); node_modules are build-time only.
$fromRepo = @('packages')
# native/overlay holds 635 MB of fork sources; the installer only needs the runtime defaults
$nativeFiles = @('pi-defaults')
$fromPayload = @('python')
if (-not (Test-Path $Exe)) { throw ('no build output: ' + $Exe + ' (run win-build.cmd first)') }
$agent = Join-Path $buildRoot 'release-fast\agent.exe'
if (-not (Test-Path $agent)) { throw ('no agent binary: ' + $agent + ' (run scripts\build-agent.cmd first)') }
foreach ($item in $fromRepo) { if (-not (Test-Path (Join-Path $Repo $item))) { throw ('repo is missing ' + $item) } }
if (-not (Test-Path (Join-Path $Repo 'native\pi-defaults'))) { throw 'repo is missing native/pi-defaults' }
foreach ($item in $fromPayload) { if (-not (Test-Path (Join-Path $Payload $item))) { throw ('payload is missing ' + (Join-Path $Payload $item) + ' (scripts/prepare-python.ps1)') } }
if (-not (Test-Path (Join-Path $Repo 'native\pi-defaults\skills'))) { throw 'repo is missing native/pi-defaults/skills' }

function Copy-Tree([string]$From, [string]$To) {
    New-Item -ItemType Directory (Split-Path $To -Parent) -Force | Out-Null
    if (Test-Path $From -PathType Leaf) { Copy-Item $From $To -Force; return }
    robocopy $From $To /E /XJ /NFL /NDL /NJH /NJS /R:1 /W:1 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw ('robocopy failed for ' + $From + ' (' + $LASTEXITCODE + ')') }
}

if (-not $SkipStaging) {
    if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
    New-Item -ItemType Directory $staging -Force | Out-Null
    Copy-Tree $Exe (Join-Path $staging 'zepi.exe')
    Copy-Item $agent (Join-Path $staging 'agent.exe') -Force
    foreach ($item in $fromRepo) { Copy-Tree (Join-Path $Repo $item) (Join-Path $staging $item) }
    Get-ChildItem (Join-Path $staging 'packages') -Recurse -Directory -Filter 'node_modules' | Remove-Item -Recurse -Force
    # Dev-only trees and bins replaced by agent.exe subcommands are not shipped.
    $pkg = Join-Path $staging 'packages'
    Remove-Item (Join-Path $pkg 'acp\adapter'), (Join-Path $pkg 'acp\test'), (Join-Path $pkg 'runtime\test'), (Join-Path $pkg 'tools'), (Join-Path $pkg 'runtime\bin\pi-settings.mjs'), (Join-Path $pkg 'runtime\bin\pi-extensions.mjs') -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($item in $nativeFiles) { Copy-Tree (Join-Path $Repo "native\$item") (Join-Path $staging "native\$item") }
    New-Item -ItemType Directory (Join-Path $staging 'native') -Force | Out-Null
    Copy-Item (Join-Path $Repo 'native\*.json') (Join-Path $staging 'native') -Force
    # pi-tui native prebuild for the standalone agent TUI
    $tui = Join-Path $Repo 'packages\acp\node_modules\@earendil-works\pi-tui\native\win32\prebuilds\win32-x64\win32-platform.node'
    $tuiDest = Join-Path $staging 'native\win32\prebuilds\win32-x64'
    New-Item -ItemType Directory $tuiDest -Force | Out-Null
    Copy-Item $tui $tuiDest -Force
    # pi assets in subfolders next to the exe (PI_PACKAGE_DIR / photon fallback)
    $piDest = Join-Path $staging 'pi'; New-Item -ItemType Directory $piDest -Force | Out-Null
    Copy-Item (Join-Path $Repo 'packages\acp\node_modules\@earendil-works\pi-coding-agent\package.json') $piDest -Force
    $phDest = Join-Path $staging 'photon'; New-Item -ItemType Directory $phDest -Force | Out-Null
    Copy-Item (Join-Path $Repo 'packages\acp\node_modules\@earendil-works\pi-coding-agent\node_modules\@silvia-odwyer\photon-node\photon_rs_bg.wasm') $phDest -Force
    # pi logo shown for the bundled agent (read from disk by the UI)
    $icoDest = Join-Path $staging 'icons'; New-Item -ItemType Directory $icoDest -Force | Out-Null
    Copy-Item (Join-Path $Repo 'native\overlay\assets\icons\pi.svg') $icoDest -Force
    foreach ($item in $fromPayload) { Copy-Tree (Join-Path $Payload $item) (Join-Path $staging "deps\$item") }
    # Node and Python build leftovers add nothing to the install
    Get-ChildItem (Join-Path $staging 'deps\python') -Filter __pycache__ -Directory -Recurse -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
    $size = [int](((Get-ChildItem $staging -Recurse -Force -File | Measure-Object Length -Sum).Sum) / 1MB)
    Write-Host ('staged ' + $size + ' MB from ' + $Repo + ' + ' + $Exe + ' + ' + $Payload)
}

if ($Zip) {
    $zipPath = Join-Path $dist ('zepi-' + $Version + '-portable.zip')
    Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $zipPath -Force
    Get-Item $zipPath | ForEach-Object { Write-Host ('built ' + $_.FullName + ' (' + [int]($_.Length / 1MB) + ' MB)') }
} else {
    $iscc = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        'C:\Program Files (x86)\Inno Setup 6\ISCC.exe',
        'C:\Program Files\Inno Setup 6\ISCC.exe'
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $iscc) { throw 'Inno Setup not found. Install it with: winget install -e --id JRSoftware.InnoSetup' }
    & $iscc ('/DAppVersion=' + $Version) (Join-Path $Repo 'installer\zepi.iss')
    if ($LASTEXITCODE -ne 0) { throw ('ISCC failed with exit code ' + $LASTEXITCODE) }
    $setup = Join-Path $dist ('zepi-' + $Version + '-setup.exe')
    if (-not (Test-Path $setup)) { throw ('installer not produced: ' + $setup) }
    Get-Item $setup | ForEach-Object { Write-Host ('built ' + $_.FullName + ' (' + [int]($_.Length / 1MB) + ' MB)') }
}
# staging is only packaging input: 1.4 GB of duplicates that ISCC already compressed
if (-not $KeepStaging -and (Test-Path $staging)) {
    Remove-Item $staging -Recurse -Force
    Write-Host 'staging removed'
}

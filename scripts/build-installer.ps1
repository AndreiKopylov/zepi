# Builds the Zepi installer: stages the repo + build + payload into dist, then runs Inno Setup.
# Nothing is taken from a working bundle: dist\staging is rebuilt from its sources every time.
param(
    [string]$Version = '0.0.1-beta1',
    [string]$Repo = '',
    [string]$Exe = '',
    [string]$Payload = '',
    [switch]$SkipStaging,
    [switch]$KeepStaging
)
$ErrorActionPreference = 'Stop'
# Paths are derived from this script's location, so nothing machine-specific is committed.
if (-not $Repo) { $Repo = Split-Path $PSScriptRoot -Parent }
if (-not $Exe) { $Exe = Join-Path (Split-Path $Repo -Parent) 'zepi-build\release-fast\zepi.exe' }
if (-not $Payload) { $Payload = Join-Path $Repo '.local\payload' }
$staging = Join-Path $Repo 'dist\staging'
$dist = Join-Path $Repo 'dist'

$iscc = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
    'C:\Program Files (x86)\Inno Setup 6\ISCC.exe',
    'C:\Program Files\Inno Setup 6\ISCC.exe'
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup not found. Install it with: winget install -e --id JRSoftware.InnoSetup' }

# Sources: build output, the repository itself, and the runtime payload (node/python/browsers).
$fromRepo = @('packages', 'node_modules')
# native/overlay holds 635 MB of fork sources; the installer only needs the runtime defaults
$nativeFiles = @('pi-defaults')
$fromPayload = @('node', 'python', 'browsers')
if (-not (Test-Path $Exe)) { throw ('no build output: ' + $Exe + ' (run win-build.cmd first)') }
foreach ($item in $fromRepo) { if (-not (Test-Path (Join-Path $Repo $item))) { throw ('repo is missing ' + $item) } }
if (-not (Test-Path (Join-Path $Repo 'native\pi-defaults'))) { throw 'repo is missing native/pi-defaults' }
foreach ($item in $fromPayload) { if (-not (Test-Path (Join-Path $Payload $item))) { throw ('payload is missing ' + (Join-Path $Payload $item) + ' (scripts/prepare-python.ps1, node zip, scripts/setup/browser.mjs)') } }
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
    foreach ($item in $fromRepo) { Copy-Tree (Join-Path $Repo $item) (Join-Path $staging $item) }
    foreach ($item in $nativeFiles) { Copy-Tree (Join-Path $Repo "native\$item") (Join-Path $staging "native\$item") }
    New-Item -ItemType Directory (Join-Path $staging 'native') -Force | Out-Null
    Copy-Item (Join-Path $Repo 'native\*.json') (Join-Path $staging 'native') -Force
    foreach ($item in $fromPayload) { Copy-Tree (Join-Path $Payload $item) (Join-Path $staging "deps\$item") }
    # Node and Python build leftovers add nothing to the install
    Get-ChildItem (Join-Path $staging 'deps\python') -Filter __pycache__ -Directory -Recurse -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
    $size = [int](((Get-ChildItem $staging -Recurse -Force -File | Measure-Object Length -Sum).Sum) / 1MB)
    Write-Host ('staged ' + $size + ' MB from ' + $Repo + ' + ' + $Exe + ' + ' + $Payload)
}

& $iscc ('/DAppVersion=' + $Version) (Join-Path $Repo 'installer\zepi.iss')
if ($LASTEXITCODE -ne 0) { throw ('ISCC failed with exit code ' + $LASTEXITCODE) }
$setup = Join-Path $dist ('zepi-' + $Version + '-setup.exe')
if (-not (Test-Path $setup)) { throw ('installer not produced: ' + $setup) }
Get-Item $setup | ForEach-Object { Write-Host ('built ' + $_.FullName + ' (' + [int]($_.Length / 1MB) + ' MB)') }
# staging is only packaging input: 1.4 GB of duplicates that ISCC already compressed
if (-not $KeepStaging -and (Test-Path $staging)) {
    Remove-Item $staging -Recurse -Force
    Write-Host 'staging removed'
}

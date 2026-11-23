# Trims the cargo target dir: stale artifacts pile up across zed.json pin bumps and profile
# switches and cargo never deletes them (Nov 2026: 197 GB in zepi-build).
# Anything the current build still needs was written by the last build, so age is a safe filter:
# a removed .rlib only costs a rebuild of that unit.
# Usage: prune-deps.ps1 [-Days 7] [-DryRun] [-Target <dir>]
param(
    [int]$Days = 7,
    [switch]$DryRun,
    [string]$Target = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (-not $Target) { $Target = Join-Path (Split-Path $repo -Parent) 'zepi-build' }
if (-not (Test-Path $Target)) { throw "target dir not found: $Target" }

$cutoff = (Get-Date).AddDays(-$Days)
$profiles = Get-ChildItem $Target -Directory | Where-Object { $_.Name -ne 'tmp' -and $_.Name -ne 'cxxbridge' }

$files = foreach ($profile in $profiles) {
    $path = Join-Path $profile.FullName 'deps'
    if (Test-Path $path) { Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue | Where-Object LastWriteTime -lt $cutoff }
}
# Build dirs go whole with their .fingerprint twin: cargo skips a build script whose fingerprint is
# fresh, so files deleted out of a live build dir break the next compile on missing OUT_DIR output.
$buildDirs = foreach ($profile in $profiles) {
    $path = Join-Path $profile.FullName 'build'
    if (-not (Test-Path $path)) { continue }
    foreach ($d in Get-ChildItem $path -Directory -ErrorAction SilentlyContinue) {
        $newest = Get-ChildItem $d.FullName -Recurse -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($newest -and $newest.LastWriteTime -ge $cutoff) { continue }
        $d
        $fp = Join-Path $profile.FullName (Join-Path '.fingerprint' $d.Name)
        if (Test-Path $fp) { Get-Item $fp }
    }
}
$dirs = @(foreach ($profile in $profiles) {
    $path = Join-Path $profile.FullName 'incremental'
    if (Test-Path $path) { Get-ChildItem $path -Directory -ErrorAction SilentlyContinue | Where-Object LastWriteTime -lt $cutoff }
    $timings = Join-Path $Target 'cargo-timings'
    if (Test-Path $timings) { Get-ChildItem $timings -File -ErrorAction SilentlyContinue | Where-Object LastWriteTime -lt $cutoff }
}) + @($buildDirs)

$fileBytes = ($files | Measure-Object Length -Sum).Sum
$dirBytes = 0
foreach ($dir in $dirs) {
    if ($dir.Name -like 'cargo-timing-*') { $dirBytes += $dir.Length; continue }
    $dirBytes += (Get-ChildItem $dir.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
}
$totalMb = [math]::Round((($fileBytes + $dirBytes) / 1MB), 0)
$count = @($files).Count + @($dirs).Count
$mode = if ($DryRun) { 'would delete' } else { 'deleting' }
"$mode $count items, $totalMb MB older than $Days days in $Target"
if ($DryRun) { return }

foreach ($file in $files) { Remove-Item $file.FullName -Force -ErrorAction SilentlyContinue }
foreach ($dir in $dirs) { Remove-Item $dir.FullName -Recurse -Force -ErrorAction SilentlyContinue }
"done"

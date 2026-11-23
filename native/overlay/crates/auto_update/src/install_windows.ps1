$ErrorActionPreference = 'Stop'
$mutex = [Threading.Mutex]::new($false, 'Local\Zepi-Update-Installer')
$locked = $false
try {
    try {
        $locked = $mutex.WaitOne(0)
    } catch [Threading.AbandonedMutexException] {
        $locked = $true
    }
    if (-not $locked) { exit 20 }

    # Wait for Inno Setup's loader, not for apps launched after installation.
    $start = [Diagnostics.ProcessStartInfo]::new($env:ZEPI_UPDATE_INSTALLER)
    $start.UseShellExecute = $true
    $start.Verb = 'runas'
    $setup = [Diagnostics.Process]::Start($start)
    $setup.WaitForExit()
    exit $setup.ExitCode
} catch {
    $cause = $_.Exception.GetBaseException()
    if ($cause -is [ComponentModel.Win32Exception] -and $cause.NativeErrorCode -eq 1223) {
        exit 2
    }
    [Console]::Error.WriteLine($cause.Message)
    exit 21
} finally {
    if ($locked) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}

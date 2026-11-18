@echo off
rem Background full build: pins the cargo/rustc chain to a core subset with below-normal
rem priority, so a rebuild does not freeze the desktop. Detached; log to .local\zbuild.log.
rem ZEPI_BG_AFFINITY  hex core mask, default 0x3FF (10 of 16 logical cores)
rem ZEPI_BG_JOBS       parallel rustc inside the pool, default 10
rem Keep this file ASCII-only: cmd.exe reads .bat in OEM codepage and non-ASCII breaks parsing.
setlocal
for %%I in ("%~dp0..") do set "REPO=%%~fI"
if "%ZEPI_BG_AFFINITY%"=="" set ZEPI_BG_AFFINITY=0x3FF
if "%ZEPI_BG_JOBS%"=="" set ZEPI_BG_JOBS=10
set ZEPI_JOBS=%ZEPI_BG_JOBS%
cd /d %REPO%
start "" /affinity %ZEPI_BG_AFFINITY% /belownormal /b cmd /c "win-build.cmd > .local\zbuild.log 2>&1"
echo BG_BUILD_STARTED affinity=%ZEPI_BG_AFFINITY% jobs=%ZEPI_BG_JOBS% log=%REPO%\.local\zbuild.log

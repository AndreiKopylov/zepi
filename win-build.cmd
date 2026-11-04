@echo off
rem Zepi Windows build: target lives outside the repo, profile release-fast.
rem INCLUDE/LIB/PATH are set manually because the C++ workload (vcvars64) is not installed.
rem Keep this file ASCII-only: cmd.exe reads .bat in OEM codepage and non-ASCII breaks parsing.
setlocal
set REPO=%~dp0
set REPO=%REPO:~0,-1%
set MSVC=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207
set SDK=C:\Program Files (x86)\Windows Kits\10
set RUSTUP_TOOLCHAIN=1.98.1-x86_64-pc-windows-msvc
if "%CARGO_TARGET_DIR%"=="" set CARGO_TARGET_DIR=%REPO%\..\zepi-build
set CARGO_NET_OFFLINE=true
set CARGO_NET_GIT_FETCH_WITH_CLI=true
rem Low parallelism: 12 rustc with debug=full exhausted RAM (OOM -> rustc 0xC0000409).
if "%ZEPI_JOBS%"=="" set ZEPI_JOBS=6
set CARGO_BUILD_JOBS=%ZEPI_JOBS%
rem ZEPI_DEBUG=1 switches debug info to line-tables-only (smaller target, full rebuild).
if "%ZEPI_DEBUG%"=="1" set CARGO_PROFILE_RELEASE_FAST_DEBUG=line-tables-only
set ZED_COMMIT_SHA=c4273271092b0287f7d181f616bd5171e0a2f70c-zepi-local
set LK_CUSTOM_WEBRTC=%REPO%\.local\upstream\zed\.webrtc-sys\webrtc-0001d84-4\win-x64-release
set INCLUDE=%MSVC%\include;%SDK%\Include\10.0.26100.0\ucrt;%SDK%\Include\10.0.26100.0\um;%SDK%\Include\10.0.26100.0\shared;%SDK%\Include\10.0.26100.0\winrt;%SDK%\Include\10.0.26100.0\cppwinrt;%REPO%\.local\msvc-shim
set LIB=%MSVC%\lib\x64;%SDK%\Lib\10.0.26100.0\ucrt\x64;%SDK%\Lib\10.0.26100.0\um\x64
if "%ZEPI_EXTRA_PATH%"=="" set ZEPI_EXTRA_PATH=%REPO%\..\tools\cmake-3.30.5-windows-x86_64\bin;%REPO%\..\tools\nasm-2.16.03
set PATH=%MSVC%\bin\Hostx64\x64;%ZEPI_EXTRA_PATH%;%PATH%
cd /d %REPO%\.local\upstream\zed
cargo build --target-dir "%CARGO_TARGET_DIR%" -p zed --bin zepi --profile release-fast --features gpui_platform/runtime_shaders %*
if errorlevel 1 exit /b 1
echo|set /p="%REPO%"> "%CARGO_TARGET_DIR%\release-fast\zepi-root.txt"
echo BUILD_OK binary=%CARGO_TARGET_DIR%\release-fast\zepi.exe

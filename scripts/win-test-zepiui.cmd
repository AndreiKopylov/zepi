@echo off
setlocal
for %%I in ("%~dp0..") do set "REPO=%%~fI"
set MSVC=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207
set SDK=C:\Program Files (x86)\Windows Kits\10
set RUSTUP_TOOLCHAIN=1.98.1-x86_64-pc-windows-msvc
if "%CARGO_TARGET_DIR%"=="" set CARGO_TARGET_DIR=%REPO%\..\zepi-build
set CARGO_NET_OFFLINE=false
set CARGO_BUILD_JOBS=12
set ZED_COMMIT_SHA=c4273271092b0287f7d181f616bd5171e0a2f70c-zepi-local
set LK_CUSTOM_WEBRTC=%REPO%\.local\upstream\zed\.webrtc-sys\webrtc-0001d84-4\win-x64-release
set INCLUDE=%MSVC%\include;%SDK%\Include\10.0.26100.0\ucrt;%SDK%\Include\10.0.26100.0\um;%SDK%\Include\10.0.26100.0\shared;%SDK%\Include\10.0.26100.0\winrt;%SDK%\Include\10.0.26100.0\cppwinrt;%REPO%\.local\msvc-shim
set LIB=%MSVC%\lib\x64;%SDK%\Lib\10.0.26100.0\ucrt\x64;%SDK%\Lib\10.0.26100.0\um\x64
if "%ZEPI_EXTRA_PATH%"=="" set ZEPI_EXTRA_PATH=%REPO%\..\tools\cmake-3.30.5-windows-x86_64\bin;%REPO%\..\tools\nasm-2.16.03
set PATH=%MSVC%\bin\Hostx64\x64;%ZEPI_EXTRA_PATH%;%PATH%
cd /d %REPO%\.local\upstream\zed
cargo test -p zepi_ui --lib 2>&1
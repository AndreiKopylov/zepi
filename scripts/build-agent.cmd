@echo off
rem Compile the ACP adapter + pi into a single bun binary (no Node at runtime).
rem Uses the pinned bun (native/bun.json) so the build is machine-independent.
set "BUN=%~dp0..\.local\bun\bun.exe"
if not exist "%BUN%" powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0prepare-bun.ps1"
cd /d "%~dp0..\packages\acp"
rem npm pi lacks the monorepo src worker entrypoints; shim them so the embedded key
rem matches the sandbox's relative resolution (see HOST below for codemode).
set "PI=node_modules/@earendil-works/pi-coding-agent"
mkdir "src\extensions\codemode" 2>nul
> "src\extensions\codemode\worker.ts" echo // @ts-nocheck
>> "src\extensions\codemode\worker.ts" echo import "../../../%PI%/dist/extensions/codemode/worker.js";
mkdir "src\utils" 2>nul
> "src\utils\image-resize-worker.ts" echo // @ts-nocheck
>> "src\utils\image-resize-worker.ts" echo import "../../%PI%/dist/utils/image-resize-worker.js";
rem The sandbox resolves ./src/... from two bases (binary root and host.js dir); embed both.
set "HOST=node_modules\@earendil-works\pi-codemode\dist\runtime"
mkdir "%HOST%\src\extensions\codemode" 2>nul
> "%HOST%\src\extensions\codemode\worker.ts" echo // @ts-nocheck
>> "%HOST%\src\extensions\codemode\worker.ts" echo import "../../../../../../../@earendil-works/pi-coding-agent/dist/extensions/codemode/worker.js";
"%BUN%" build --compile --no-compile-autoload-bunfig --no-compile-autoload-dotenv src/cli.ts src/extensions/codemode/worker.ts src/utils/image-resize-worker.ts "%HOST%\src\extensions\codemode\worker.ts" --outfile "%~dp0..\..\zepi-build\release-fast\agent.exe"
rem pi-tui native prebuild for the standalone interactive TUI (same layout as pi.exe).
set "OUT=%~dp0..\..\zepi-build\release-fast"
mkdir "%OUT%\native\win32\prebuilds\win32-x64" 2>nul
copy /Y "node_modules\@earendil-works\pi-tui\native\win32\prebuilds\win32-x64\win32-platform.node" "%OUT%\native\win32\prebuilds\win32-x64\" >nul
rem pi assets in subfolders next to the exe (PI_PACKAGE_DIR / photon fallback).
mkdir "%OUT%\pi" 2>nul
copy /Y "node_modules\@earendil-works\pi-coding-agent\package.json" "%OUT%\pi\package.json" >nul
mkdir "%OUT%\photon" 2>nul
copy /Y "node_modules\@earendil-works\pi-coding-agent\node_modules\@silvia-odwyer\photon-node\photon_rs_bg.wasm" "%OUT%\photon\photon_rs_bg.wasm" >nul
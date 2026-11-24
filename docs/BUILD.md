# Building Zepi on Windows

## Prerequisites

- Windows 10/11 x64 and Git with `core.longpaths=true`.
- VS 2022 Build Tools: **MSVC v143 x64/x86**, its **Spectre libraries**, and **Windows SDK 10.0.26100**.
  The full C++ workload is not required. The Windows scripts set `INCLUDE`, `LIB`, and `PATH`
  directly; check their MSVC path if your toolset version differs.
- Rust **1.98.1** (the toolchain pin is read from `native/zed.json`), Node **22.19+**, Python
  **3.10+**, CMake, and NASM. Bun is needed on `PATH` for tests; `build-agent.cmd` uses its own
  pinned Bun from `native/bun.json`.

## First build

From the repository root:

```powershell
npm ci
npm ci --prefix packages/acp --ignore-scripts
npm ci --prefix packages/runtime
python scripts\native.py fetch
```

`fetch` downloads the Zed archive pinned in `native/zed.json`, checks its SHA-256, prepares the
source tree, and fills Cargo's dependency cache. `win-build.cmd` builds offline. Use the direct
`npm ci` commands above if `npm run setup:acp` fails with `spawn npm ENOENT` on Windows.

Before building:

- Extract `webrtc-win-x64-release.zip` from the `webrtc-0001d84-4` release of
  `zed-industries/livekit-rust-sdks` into
  `.local\upstream\zed\.webrtc-sys\webrtc-0001d84-4\win-x64-release` (the directory should contain
  `include` and `lib`). `native/assets.json` describes the macOS asset, not this Windows archive.
- For the C11 header used by `wasmtime-c-api`, create `.local/msvc-shim/stdalign.h`. The Windows
  scripts already include this directory in `INCLUDE`:

  ```c
  #pragma once
  #ifndef __cplusplus
  #define alignas _Alignas
  #define alignof _Alignof
  #define __alignas_is_defined 1
  #define __alignof_is_defined 1
  #endif
  ```

## Build and run

```powershell
python scripts\native.py prepare
.\win-build.cmd
.\scripts\build-agent.cmd
python scripts\win-run.py
```

- Run `prepare` after changing `native/overlay` or `native/removed-paths.json`. Cargo builds from
  `.local\upstream\zed`; `win-build.cmd` and `scripts\win-check.cmd` refuse a stale overlay.
- `win-build.cmd` builds `zepi.exe` with `release-fast` into `..\zepi-build\release-fast` by
  default. It uses line-tables-only debug info, incremental compilation for workspace crates, and
  sccache when available. `ZEPI_DEBUG=full` restores full debug info at the cost of a full rebuild.
  `ZEPI_JOBS` defaults to 6; lower it if the machine struggles. `scripts\win-build-bg.cmd` is the
  lower-priority background option (log: `.local\zbuild.log`).
- `scripts\build-agent.cmd` builds `agent.exe` beside the default `release-fast` executable.
  Rebuild it when changing ACP or runtime code embedded in the agent; a Rust rebuild is not needed
  for those changes alone. `win-run.py` also builds the JavaScript ACP entry for development and
  starts Zepi with the prepared Zed tree as its working directory (log: `.local\run.log`). If you
  override the Cargo target directory, note that the launcher uses `ZEPI_BUILD_DIR` and the agent
  script still writes to the default directory.

## Runtime and dev bundles

- The packaged editor launches `agent.exe` (Bun + pi + ACP), not a Node process. `pi/package.json`
  and `photon/photon_rs_bg.wasm` are shipped beside it in their respective directories. System Node
  is still useful for optional features such as language servers and preview helpers.
- The browser is not bundled. Install **browse** from **Settings → AI → Browser use** to connect
  a real Chromium browser through its extension. There is no bundled Chromium to prepare.
- Native Office viewers don't need Python. For Office agent skills, run
  `powershell -File scripts\prepare-python.ps1 -Deps .\deps` in the checkout to populate
  `deps\python` for development. Without `-Deps`, the script builds `.local\payload\python` for
  packaging instead.
- A dev build with `zepi-root.txt` beside the executable keeps `.zed` and `.pi\agent` there.
  Without that marker, the installed, portable, and dev bundle layouts use `%LOCALAPPDATA%\Zepi`.

If you use the local, untracked `scripts\deploy-bundle.ps1`, pass a dedicated dev bundle directory
with its runtime assets in place. Save and close Zepi first. The script refreshes the executables,
ACP/runtime sources and their `node_modules`; it refuses a directory with an Inno uninstaller.
Packaged copies intentionally omit `node_modules` and ACP tests; pi settings and extensions run as
`agent.exe` subcommands.

## Checks

```powershell
npm run check
npm run test:acp        # Bun; unset OPENCODE_API_KEY first for a clean test environment
npm run test:runtime
.\scripts\win-check.cmd # link-free Rust check; optionally pass a crate name
```

On Windows, use `npm run test:acp:node` only to diagnose ACP cases skipped on Bun.
It runs the affected test files one at a time at BelowNormal priority and is not part of CI
or deployment. Do not run it alongside a build.

For a built dev bundle, use `npm run smoke -- --root <dev-bundle>` (`--full` adds preview/shell
checks). An installed copy or a portable ZIP is not a smoke-test target: it lacks the dev modules
and fixtures. Run `prepare` before checking or testing modified Rust overlay code.

If linking fails with `os error 5`, close the running `zepi.exe`. For `LNK1318 PDB LIMIT`, check
free space in the target directory. `powershell -File scripts\prune-deps.ps1 -DryRun` estimates how
much space stale build artifacts occupy before you decide whether to remove them.

## Packaging

`scripts\build-installer.ps1` stages a Windows bundle from the repository, the built editor and
agent, and `.local\payload\python`. It reads the version from `PRODUCT_VERSION` and prefers the
`release-installer` build (`win-build.cmd installer`), falling back to `release-fast`. Use the
`stable` channel for a release so the executable's AppUserModelID matches the shortcuts. Inno
Setup produces `dist\zepi-<version>-setup.exe`; `-Zip` builds a portable archive instead.

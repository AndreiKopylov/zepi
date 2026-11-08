# Building Zepi on Windows

## Prerequisites

- Windows 10/11 x64, git with `core.longpaths true`.
- **MSVC v143** (Visual Studio Build Tools 2022) plus Windows SDK 10.0.26100, including the Spectre
  libraries. The C++ workload is not required: `win-build.cmd` sets `INCLUDE`/`LIB`/`PATH` by hand.
- **Rust 1.98.1** (`RUSTUP_TOOLCHAIN` in `win-build.cmd`).
- **Node >= 22.19**, **Python 3.10+** (its pip downloads the Python layer wheels), **cmake** and
  **nasm** in `PATH`.

## Pipeline

```powershell
npm ci                                   # tsx/typescript for the adapter
npm ci --prefix packages/acp --ignore-scripts
python scripts\native.py prepare         # Zed tarball pinned by native/zed.json + native/overlay
win-build.cmd                            # cargo build -p zed --bin zepi --profile release-fast
```

- `native.py prepare` unpacks Zed into `.local\upstream\zed`, copies `native/overlay/**` over it
  and removes the paths listed in `native/removed-paths.json`. **Run prepare after any overlay
  change** — cargo builds from `.local\upstream\zed`.
- `win-build.cmd` builds from `.local\upstream\zed` with the target directory `..\zepi-build`
  (outside the repository). The `release-fast` profile disables LTO and keeps `debug=full`, hence a
  ~131 GB target and a ~374 MB executable.
- The WebRTC win-x64 asset is downloaded manually, pinned in `native/assets.json`, into
  `.local\upstream\zed\.webrtc-sys\webrtc-0001d84-4\win-x64-release`.

## Run and deploy

```powershell
python scripts\win-run.py                    # run with cwd = upstream (tree-sitter grammars)
powershell -File scripts\deploy-bundle.ps1   # copy the build into the bundle and restart
```

The bundle is a folder such as `C:\Llm\zepi` holding `zepi.exe`, `packages`, `node_modules`,
`native` and `deps\{node,python}` (browsers are not bundled: the agent uses system Chrome/Edge or installs the pinned Chromium on demand into the state directory). It carries no state: that lives in
`%LOCALAPPDATA%\Zepi`.

## Payload (deps)

```powershell
# Python 3.13 + python-docx/openpyxl/python-pptx/pypdf/Pillow
powershell -File scripts\prepare-python.ps1            # -> .local\payload\python

# Node 24 (node.exe + npm.cmd): unpack the zip from nodejs.org into .local\payload\node
# Chromium is not shipped: system Chrome/Edge or on-demand install into the state directory.
# scripts\setup\browser.mjs (-> .local\browsers) remains for dev use only and is not copied to the payload.
```

## Installer

```powershell
powershell -File scripts\build-installer.ps1 -Version 0.0.1-beta1
# dist\zepi-0.0.1-beta1-setup.exe
```

The script stages three sources (the repository, `zepi-build\release-fast\zepi.exe` and
`.local\payload`) and runs Inno Setup (`installer/zepi.iss`). No machine-specific paths are
committed: everything derives from the repository location. Staging is removed after the build
(`-KeepStaging` keeps it).

## Tests

```powershell
npm run test:acp        # ACP adapter
npm run test:runtime    # pi runtime: extensions, settings, browser
scripts\win-check.cmd   # cargo check -p zed --bin zepi
```

## Common pitfalls

- Editing `native/overlay` without `prepare` ships the old copy inside the exe.
- `LNK1318 PDB LIMIT` means the disk is full; `os error 5` during linking means `zepi.exe` is running.
- A clean build of the vendor crates with 6 parallel rustc processes can run out of memory: set
  `ZEPI_JOBS=3`.
- `deploy-bundle.ps1` will not start the new build while an old process is alive (single-instance
  hands the window to it) — close it manually.

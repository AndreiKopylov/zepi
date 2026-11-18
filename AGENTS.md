# AGENTS.md — zepi

Windows port of zepi: an AI workspace on top of Zed v1.28.1 (pin in `native/zed.json`) with the
pi 1.5.2 agent (ACP) and office document viewers (xlsx with editing, docx, pptx) on vendored
crafts. Rust (gpui) + TypeScript (pi layer). See README.md for the human-readable intro.

## Repository layout

```
zepi/
├── native/
│   ├── zed.json               # Zed pin: tag, commit, tarball sha256, Rust version
│   ├── appearance.json        # appearance defaults (dark theme, docks)
│   ├── pi-defaults/           # pi seeds: models.json, settings.json, skills (no secrets)
│   └── overlay/               # overlay applied on top of Zed sources (as in upstream zepi)
│       ├── crates/            # modified/new Zed crates (zepi_ui, agent_ui, workspace, zed, ...)
│       │   └── zepi_ui/src/   # sheet_viewer (xlsx), word_viewer (docx), slides_viewer (pptx)
│       ├── vendor/            # gridcraft (xlsx), deckcraft (pptx), wordcraft (docx)
│       └── assets/keymaps/    # default-windows.json (JSONC!) with office viewer bindings
├── packages/
│   ├── acp/                   # ACP adapter: pi as the editor agent (TypeScript)
│   ├── runtime/               # pi runtime: settings, extensions, browser config
│   └── tools/                 # doctor/probe utilities
├── utils/sheetprobe/      # dev-only xlsx->PNG renderer to calibrate Excel view vs Excel (no Zed rebuild)
├── scripts/
│   ├── native.py              # prepare: Zed tarball per pin + overlay + removals
│   ├── win-run.py             # run: seeds configs, launches the release-fast exe
│   ├── build-agent.cmd        # bun build --compile cli.ts -> agent.exe (pinned bun 1.4.4)
│   ├── build-installer.ps1    # staging + Inno Setup; version read from PRODUCT_VERSION
│   ├── zed-update.py          # price a new Zed version in a scratch tree, overlay untouched
│   ├── zed-port-matrix.py     # port cost per overlay file: PORT/KEEP/NEW/DROP
│   └── zed-port-merge.py      # 3-way merge of an overlay file (ours/base/theirs)
├── installer/zepi.iss         # Inno Setup script (per-machine, AppMutex, /DAppVersion)
├── win-build.cmd              # cargo build -p zed --bin zepi --profile release-fast (or `installer`,
│                                or `check <crate>` for a link-free type-check of one overlay crate)
├── win-run.sh                 # launcher: win-build.cmd outside MSYS path conversion
└── .local/                    # ignored: tarballs, upstream zed tree, build logs
```

## Stack

- Native: Rust 1.98.1, Zed v1.28.1 (tarball by pin, sha256 verified), gpui; `release-fast` cargo
  profile (debug is unusable: laggy UI); `release-installer` (strip, thin-LTO) for the installer only;
  target dir outside the repo. Auto-update rides Zed's mechanism (GitHub releases/latest redirect,
  detached non-silent Inno installer). Version single source: `PRODUCT_VERSION` in `zepi_runtime.rs`.
- Agent: single bun binary (agent.exe), TypeScript, ACP; browser on **browse** (opencode,
  MIT): a Rust relay + Chromium extension driving the user's real browser (logins/SSO),
  installed on demand from Settings > Browser.
- Build tools: MSVC v143 + Windows SDK 10.0.26100 + Spectre-mitigated libs v143 (individual Build
  Tools components, not the C++ workload), cmake and nasm in PATH, `.local/msvc-shim/stdalign.h`
  shim for wasmtime-c-api.

## Build, run, test

```bat
npm ci
python scripts\native.py prepare   :: REQUIRED after any native/overlay change
win-build.cmd                       :: ZEPI_JOBS=6 keeps the desktop responsive; raise for
                                    :: unattended builds only (10+ freezes the UI on 16 cores)
                                    :: dev builds emit line-tables-only PDB (fast rustc/link);
                                    :: ZEPI_DEBUG=full restores full debug info (one-off full rebuild);
                                    :: workspace crates build incremental, deps go through sccache
                                    :: (auto-on; ZEPI_INCREMENTAL=0 / ZEPI_NO_SCCACHE=1 to disable);
                                    :: warm rebuild after a small overlay edit: ~3-4 min (project included);
                                    :: per-build timing report lands in <target>\cargo-timings\cargo-timing.html
scripts\win-build-bg.cmd             :: background full build: affinity 0x3FF + below-normal priority
                                    :: (desktop stays responsive; log in .local\zbuild.log)
scripts\win-check.cmd [crate]       :: fast inner loop: link-free type-check (defaults to whole zed)
scripts\build-agent.cmd             :: agent.exe (bun); needed after packages/acp|runtime changes
scripts\win-run.py                 :: run with seeded configs
scripts\build-installer.ps1        :: dist\zepi-<ver>-setup.exe (version from PRODUCT_VERSION)
```

- TS tests: `npm run test:acp` (clean env: unset OPENCODE_API_KEY), types: `npm run check`.
- Vendor tests: `cargo test` inside `native/overlay/vendor/gridcraft` (own workspace, fast).
- zepi_ui has `#[cfg(test)]` units; first run builds the Zed dev chain (~40 min), run on demand.
- After medium or large changes, offer to run `npm run smoke -- --root <bundle>` (`--full` for
  preview/shell or `packages/acp/helpers`). Propose only; never before a build or without a go-ahead.

## Conventions

- Code, identifiers and code comments are English. Existing Russian comments were translated or
  removed; keep it that way.
- User-facing docs: README.md (English) and README.ru.md (Russian); AGENTS.md is English.
- Commit messages are English, matching the repository history.
- No secrets or internal endpoints in the repo: `native/pi-defaults` ships a neutral example
  (key via `$PROVIDER_API_KEY`); corp deployments keep their own seeds inside the bundle,
  never sync them back from the repo.
- Do not commit or push without an explicit user request.

## Gotchas (verified the hard way)

- Without `native.py prepare`, cargo builds the STALE overlay from `.local/upstream/zed`.
- ZEPI_JOBS=6 is a desktop-responsiveness limit, not a memory one (line-tables-only uses much
  less RAM). 12 was fatal under debug=full; 10+ saturates all cores and freezes Windows.
- zepi-build/release-fast/deps grows without bound across zed.json pin bumps and profile
  switches (cargo never deletes superseded artifacts). When disk runs low, delete deps files
  older than a week; the next build recreates anything still needed (Nov 2026: ~35 GB reclaimed).
- `default-windows.json` is JSONC with comments; `json.load` is not a validity check.
- A new office hotkey touches three places: overlay keymap, the seed in
  `zepi_runtime::prepare_settings`, and the live user keymap.json of existing profiles.
- Bundle sync only via `robocopy /XJ` (symlink packages/runtime/node_modules/zepi recurses);
  in Git Bash double the slashes (`//MIR //XJ`).
- Installed bundle state lives in `%LOCALAPPDATA%\Zepi\{zed,pi}`; a dev build with an
  `zepi-root.txt` marker next to the exe keeps state next to the exe.
- Excel holds open xlsx files with a sharing lock: writes fail with os error 32 and surface as the
  stock "Failed to save" prompt.
- gpui caches textures globally by `Image.id`: slides/word rasters carry a per-view `image_tag`,
  otherwise two open decks/documents swap pictures.
- pi defers session journal creation: fork/clone of an empty session must materialize the journal
  via `appendZepiEntry`, else ENOENT in detachHistoricalAgents.
- xlsx row resize persists only with `LineInfo.custom = true` (writer emits `customHeight`).
- The gridcraft streaming parser must not charge nodes inside split (streamed) elements against
  its kept-node budget, or big sheets get flagged damaged and lose `<cols>`.
- xlsx font sizes are points: render at pt * 4/3 px, else text fits differently than in Excel.
- wordcraft vendor carries a local patch (scripts/wordcraft-local-patches/wordcraft-local.patch):
  reapply it after any vendor update; rationale and Word measurements live in the team wiki
  (7-zepi/патчи-wordcraft).
- browse relay bakes OPENCODE_BROWSER_TOOLS at startup: changing tools mode needs `browse restart`
  with the env (ensureRelayMode); a dead relay is revived on session start with a restart backoff.
- Updating browse while running hits EBUSY on browse.exe: stopBrowse (taskkill) before copying.
- photon_rs_bg.wasm must sit next to agent.exe (photon/): the bun binary bakes the build-machine
  path, pi redirects the read to the exe-dir fallback.

## Documentation

README.md — intro, distribution layout, licenses; `native/zed.json` — Zed pin;
`vendor/*/VENDOR.md`, `NOTICE` — vendored code provenance;
`LICENSE` — GPL-3.0-or-later.

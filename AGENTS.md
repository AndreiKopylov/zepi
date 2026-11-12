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
│   ├── runtime/               # pi runtime: settings, extensions, browser agent
│   └── tools/                 # doctor/probe utilities
├── scripts/
│   ├── native.py              # prepare: Zed tarball per pin + overlay + removals
│   ├── win-run.py             # run: seeds configs, launches the release-fast exe
│   ├── zed-update.py          # price a new Zed version in a scratch tree, overlay untouched
│   ├── zed-port-matrix.py     # port cost per overlay file: PORT/KEEP/NEW/DROP
│   └── zed-port-merge.py      # 3-way merge of an overlay file (ours/base/theirs)
├── win-build.cmd              # cargo build -p zed --bin zepi --profile release-fast
├── win-run.sh                 # launcher: win-build.cmd outside MSYS path conversion
└── .local/                    # ignored: tarballs, upstream zed tree, build logs
```

## Stack

- Native: Rust 1.98.1, Zed v1.28.1 (tarball by pin, sha256 verified), gpui; `release-fast` cargo
  profile (debug is unusable: laggy UI); target dir outside the repo.
- Agent: bundled Node (v24 LTS in the bundle), TypeScript, ACP; browser agent on Playwright Chromium.
- Build tools: MSVC v143 + Windows SDK 10.0.26100 + Spectre-mitigated libs v143 (individual Build
  Tools components, not the C++ workload), cmake and nasm in PATH, `.local/msvc-shim/stdalign.h`
  shim for wasmtime-c-api.

## Build, run, test

```bat
npm ci
python scripts\native.py prepare   :: REQUIRED after any native/overlay change
win-build.cmd                       :: set ZEPI_JOBS=3 when memory is tight
scripts\win-run.py                 :: run with seeded configs
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
- Full rebuilds at high job counts hit rustc OOM/stack overrun: use `ZEPI_JOBS=3`.
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

## Documentation

README.md — intro, distribution layout, licenses; `native/zed.json` — Zed pin;
`vendor/*/VENDOR.md`, `ATTRIBUTION.md`, `NOTICE` — vendored code provenance;
`LICENSE` — GPL-3.0-or-later.

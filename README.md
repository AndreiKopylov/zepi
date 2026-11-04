# Zepi

[English](README.md) | [Русский](README.ru.md)

**A Windows workspace for code and office documents — with an agent built in.**

Zepi wires the [Zed](https://github.com/zed-industries/zed) editor to the
[pi](https://github.com/badlogic/pi-mono) agent over ACP. One window holds code, terminal, git and a
chat with your model — plus viewing and editing `.pptx`, `.docx` and `.xlsx` without Microsoft Office.

> Status: **beta (0.0.1)**. Built on Zed v1.26.2 and pi 1.5.0.

## What it does

| Area | Capabilities |
| :--- | :--- |
| **Code** | Full Zed editor: LSP, git, terminal, panels, themes, multi-cursor, command palette |
| **Agent** | Chat with your provider's models, tools (files, search, PowerShell, codemode), subagents, sessions (`/fork`, `/export`, `/compact`), MCP, skills |
| **Presentations** | `.pptx`: slide strip with zoom and search, text editing, send a slide or a region to the agent |
| **Documents** | `.docx`: paginated view, search, editing, send a fragment to the agent |
| **Spreadsheets** | `.xlsx`: sheets, formulas, cell editing saved straight to disk |
| **Rendering** | `zepi office render` turns pptx/docx into PNGs with the same engine the viewer draws with — no LibreOffice, no Office |

The agent handles documents on its own: bundled skills `zepi-office-docs`, `zepi-docx`, `zepi-xlsx`,
`zepi-deck-build`, `zepi-deck-design`, `zepi-office-qa` plus a bundled Python 3.13 with python-docx,
openpyxl, python-pptx, pypdf and Pillow. Nothing to install.

## Install

1. Download the latest installer, `zepi-0.0.1-beta1-setup.exe`, from [Releases](../../releases).
2. Run it and accept the UAC prompt. Zepi installs to `C:\Program Files\Zepi` and appears under
   Settings → Apps.
3. Pick components:
   - **Zepi** — editor and agent (required);
   - **Python for Office documents** — needed for `.docx`/`.xlsx`/`.pptx` (56 MB);
   - **Bundled browser** — Chromium for the agent's browser tools (702 MB, optional).
4. Configure a model provider in the agent settings (`Settings → Agent`) or in
   `%LOCALAPPDATA%\Zepi\pi\models.json`. No providers ship with the build: bring your own endpoint
   and key.

Settings, sessions and skills live in `%LOCALAPPDATA%\Zepi`; updates never touch them.

### Requirements

- Windows 10/11 x64.
- A model provider (any OpenAI-compatible endpoint, or one of pi's built-in providers).
- No system Node.js, Python or git required — everything is bundled.

## How it is built

```
zepi.exe                 editor + agent host (Rust: Zed plus the Zepi overlay)
packages/, node_modules/ ACP adapter and pi
deps/node/               Node 24 for the agent
deps/python/             CPython 3.13 with document libraries
deps/browsers/           Chromium (Playwright) for browser tools
native/                  appearance, pi defaults, skills
```

State lives in `%LOCALAPPDATA%\Zepi\{zed,pi}`; viewer images and renders go to
`%LOCALAPPDATA%\Zepi\{temp,render}` and expire after 7 days.

## Building from source

Short version (details in [`docs/BUILD.md`](docs/BUILD.md)):

```powershell
npm ci
python scripts\native.py prepare   # Zed tarball pinned by native/zed.json + the Zepi overlay
win-build.cmd                      # cargo build -p zed --bin zepi --profile release-fast
python scripts\win-run.py          # run the freshly built exe
```

You need MSVC v143 + Windows SDK, Rust 1.98.1, Node >= 22, Python 3.10+, cmake and nasm.

## Beta limitations

- The installer is unsigned, so SmartScreen warns about an unknown publisher.
- No auto-update: install a newer build over the old one, data is preserved.
- `.xlsx` has no rasteriser: no visual check for spreadsheets, values are verified programmatically.
- No models are bundled; without a key the agent stays silent.

## Credits

- [Zed Industries](https://github.com/zed-industries/zed) — editor and the gpui platform.
- [tony3liu](https://github.com/tony3liu/eido) — eido: the first Zed + pi pairing on Windows, from
  which Zepi descends.
- [Mario Zechner](https://github.com/badlogic/pi-mono) — the pi agent and the ACP protocol.
- [storytold](https://github.com/storytold) — gridcraft, deckcraft, wordcraft (document engines).
- The Node.js, Playwright and crate/npm dependency communities.

## License

Code in this repository is **GPL-3.0-or-later** (`LICENSE`): the overlay is derived from Zed
(Apache-2.0 or GPL-3.0-or-later, GPL chosen) and from eido. pi packages are GPL-3.0-or-later.
Vendored gridcraft/deckcraft/wordcraft keep MIT OR Apache-2.0; their texts and attribution live in
their directories, summarised in `NOTICE`. Trademarks belong to their owners.

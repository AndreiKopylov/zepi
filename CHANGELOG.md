# Changelog

All notable changes to Zepi are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions use
[Semantic Versioning](https://semver.org/).

## [0.0.1-beta2] - 2026-11-05

### Added

- Agent panel follows the pi interactive theme: user message cards, thread markdown, code
  highlighting and tool cards take their colors from the pi theme chosen in /settings -> Theme;
  editing the theme file reskins the panel within a second.
- Bundled pi themes `zepi-dark` and `zepi-light` (JetBrains Dark/Light aligned palettes), seeded
  into the profile themes directory on start and refreshed when the bundled copies change;
  user-created themes are never touched.
- `zepi.threadLineHeight` in Zed settings (default 1.5): line spacing of agent thread markdown,
  hot-reloaded on settings edits.
- Settings schema now declares the `zepi` section, so Zepi no longer reports Zepi-specific
  settings keys as unknown.
- `/trust` sets pi's project trust for the current folder: Trust, Trust parent folder or Do not
  trust, saved to `trust.json` in the pi config directory and applied by reloading the task, so
  a project's own `.pi` settings, extensions and packages can load.

- System prompt guidance: when a reply mentions a project file, the agent renders it as a
  markdown link (project-relative path, or a file:/// URL outside the project) so the file
  opens from the agent panel in one click.
- Deterministic file links: a code span outside fenced blocks that resolves to an existing file
  (relative to the task folder, or absolute) renders as a markdown link even when the model
  leaves it as plain code; identifiers, missing paths, URLs and fenced code stay untouched.

### Changed

- Agent startup is about twice as fast: MCP servers whose tools are codemode-only now connect
  in the background, in parallel, after the session opens, instead of blocking it. Servers with
  direct tools still connect before the session is published, and the first prompt waits for the
  background connections at the turn boundary, so a tool catalog is never half-built. In ACP
  probes, opening a 250 KB thread with five MCP servers went from 7.5 s to 3.3 s.
- The ACP entry is compiled to plain JS (`npm run build:acp`, esbuild): the runtime loads
  `packages/acp/src/cli.js` instead of paying the tsx loader cost on every start. Dev launch
  (`win-run.py`) and bundle deploy build it automatically.

### Fixed

- Composer expand/collapse button works in empty threads; the expanded empty-thread composer
  gets an 80%-viewport box instead of overflowing into the panel footer rows.
- Expanded state no longer resets itself immediately in empty threads, so the toggle sticks
  until collapsed again.
- The pi interactive `custom` component no longer errors on clients that cannot host it
  or when the view dies mid-turn: data-shaped extension fallbacks (guardrail prompts, wizards)
  degrade to plain select/confirm forms.
- Composer no longer clips the last line of a long paste in threads with messages: the composer
  container keeps its natural height when panel space is tight, and the drag-box line cap is
  computed with the same line height the editor paints with.
- Composer line spacing follows `zepi.threadLineHeight` (the thread markdown knob, default 1.5)
  and hot-reloads, including an already dragged composer height.
- Reading a file outside the project no longer fails with `File is outside this workspace`:
  reads follow the product full-access rule, so screenshots and documents handed by path open
  directly instead of being copied into the project. Writes and private workspace storage
  (`.git`, `.pi`, review temps) stay restricted.
- File links also resolve code spans written relative to a subfolder or nested repo: a unique
  suffix match against the project file index becomes a clickable link; ambiguous matches stay
  plain code.

### Removed

- Tool access guards and the Agent Access toggle: Zepi always runs pi in full access now.
  Permission prompts, the footer access selector and the old Agent Access /trust command are
  gone; policy extensions such as pi-guardrails can be layered on top instead.

## [0.0.1-beta1] - 2026-11-04

First public beta. Windows port of the Zed + pi workspace, rebuilt as Zepi.

### Added

- Office documents in the editor: `.pptx` slide strip with zoom and search, paginated `.docx`
  view, `.xlsx` sheets with formulas and cell editing.
- `zepi office render`: built-in renderer turning pptx/docx into PNGs with the deckcraft/wordcraft
  engines, no LibreOffice and no Microsoft Office required.
- Bundled Python 3.13 (python-docx, openpyxl, python-pptx, pypdf, Pillow) so the agent can work
  with documents offline.
- Agent skills: `zepi-office-docs`, `zepi-docx`, `zepi-xlsx`, `zepi-deck-build`,
  `zepi-deck-design`, `zepi-office-qa`, copied into the user profile on start.
- Windows installer (Inno Setup): per-machine install, selectable components, English and Russian,
  standard uninstall with an option to keep user data.
- Sending a slide, a slide region or a document page from the viewer to the agent.
- Agent panel dock on the left or right, drag and drop of files into the chat.
- Sidebar toggle button, Zed-style "+" button.
- Project rebranded from eido to Zepi: app name and paths, `Zepi Dark`/`Zepi Light` themes,
  icons, `@zepi/*` npm packages, single-instance mutex.

### Changed

- Zed updated to v1.26.2, pi to 1.5.0.
- Application state moved to `%LOCALAPPDATA%\Zepi\{zed,pi}`; the runtime payload lives in
  `deps\{node,python,browsers}` next to the executable.
- Build, deploy and packaging scripts no longer contain machine-specific paths.

### Fixed

- Hung child processes (agent, browser) survived session shutdown.
- Slide and Word rendering on repeated loads: unique image ids, re-read after an external edit.
- Model and provider selection: model icons, custom providers, duplicate entries.
- Tool call failures caused by invented paths: wrong Python interpreter, missing LibreOffice or
  PyMuPDF, `pip install` attempts inside the bundled interpreter.
- Reading files outside the project: skills, viewer images (`%LOCALAPPDATA%\Zepi\temp`) and renders
  (`%LOCALAPPDATA%\Zepi\render`).
- pi `export` and `fork` commands.

### Removed

- Built-in model providers: no credentials ship with the build, users configure their own.
- Unused "Running / Run finished" labels above tool calls.

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

- System prompt guidance: when a reply mentions a project file, the agent renders it as a
  markdown link (project-relative path, or a file:/// URL outside the project) so the file
  opens from the agent panel in one click.

### Fixed

- Composer expand/collapse button works in empty threads; the expanded empty-thread composer
  gets an 80%-viewport box instead of overflowing into the panel footer rows.
- Expanded state no longer resets itself immediately in empty threads, so the toggle sticks
  until collapsed again.

### Removed

- Tool access guards and the Agent Access toggle: Zepi always runs pi in full access now.
  Permission prompts, the footer access selector and the /trust command are gone; policy
  extensions such as pi-guardrails can be layered on top instead.

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

# Changelog

All notable changes to Zepi are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions use
[Semantic Versioning](https://semver.org/).

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

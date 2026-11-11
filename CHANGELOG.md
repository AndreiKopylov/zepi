# Changelog

All notable changes to Zepi are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions use
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- The agent is asked to point at lines, not just files: file links in its replies carry
  `#L<line>` or `#L<first>-L<last>` (a `:<line>` suffix works too), and Zepi opens the file
  right at that line. Both spellings were already resolved on click; the system prompt simply
  never asked for them, so replies linked bare files.

- Tool rows in the agent panel read like dsh: the header carries the tool's target - a model-written
  `description` for shell commands (the adapter adds the parameter to the `bash`/`pwsh` schemas at
  session creation, with dsh's wording), the file basename for `read`, the pattern for search tools.
  The raw JSON input collapses behind a "Raw input" disclosure instead of an always-visible block,
  and nested codemode calls are captioned "via <parent tool>" instead of the raw parent call id.
- Tool text outputs render as tight monospace code blocks with a copy button; `read` output renders
  as a numbered code block with pi-theme syntax highlighting, the language resolved from the file
  path and the grammar loaded in the background when missing.
- Screenshots in tool output keep their aspect ratio (PNG and JPEG dimensions parsed from the bytes)
  capped at 640px wide, and show a note when the temporary file no longer exists; file-type icons on
  read/edit rows keep their native colors.

### Fixed

- "Open File" on a tool row silently did nothing for pi sessions: it required a resolved editor
  location that pi never provides. It now opens the raw location path (relative paths resolved
  against the worktrees) and is hidden when the location is a directory, as for search roots.
- The nested-call placeholder ("Nested call completed. Detailed output is recorded in the parent
  tool result.") no longer renders as a nested tool's output.

## [0.0.1-beta4] - 2026-11-09

### Added

- Zepi refreshes its model catalog from pi.dev on startup, in the background and at most once
  every four hours. The catalog is cached in `ZEPI_PI_CONFIG_DIR/models-store.json`, so
  offline launches still see the latest models, and `PI_OFFLINE` suppresses network access.
  Picking up renamed or newly released models (such as `hy4`) no longer requires a
  separately installed pi: Zepi owns its own config directory and bundled catalog.
- The pi settings page button is now `Reload & update models`: it re-reads the settings
  page and forces a catalog refresh regardless of the four-hour interval.
- Selecting text in an agent message and choosing `Copy selection into composer` (or
  pressing `Ctrl-Shift-.`) inserts that selection into the message composer and focuses it.
  Both previously dispatched `AddSelectionToThread`, which only reads editor and terminal
  selections, so they silently did nothing in message views.
- Code spans with a line range (`path.cs:82-94`) and a trailing parenthesized note now resolve
  to a file mention covering the whole range, matching the `:5-9` suffix Zepi emits for
  selections. Previously those spans stayed plain text because the position parser only
  understood a single `:row`.

### Changed

- `/reload` in the composer is a pure pi reload again: extensions, skills, templates,
  themes and settings, followed by a republish of the command and config-option lists.
  It no longer refreshes the model runtime, model scope or the available-model list.
- Catalog refresh is shared by startup and the settings button. The OpenRouter-only
  helper is gone; all built-in providers are refreshed from the same pi.dev catalog.
- `Ctrl-Shift-.` still works for editor and terminal selections: when a message has no
  markdown selection, the new handler propagates the action to the original Zed handler.
- `write` and `read` now reach outside the workspace like pi: external paths go straight to
  disk, while files inside the workspace keep the native editor buffers and review flow.
  Private workspace storage (`.git`, `.local`, `.pi`, review temps) stays restricted.
- `PRODUCT_VERSION` reports `0.0.1-beta4` in the About window.
- The composer height belongs to Zed again: the pixel drag box that capped it is gone, so the
  editor line-based auto height owns the box and the expand toggle switches to full mode.
  `zepi.composerMinLines` (default 5) sets the initial height and `zepi.composerMaxLines`
  (default 12) how many lines it grows to before scrolling inside. Lines survive font, theme
  and line-spacing changes, the persisted pixel height did not.

### Fixed

- The composer no longer resizes itself. The drag box wrote one auto-height cap into the editor
  while `sync_editor_mode` wrote another, so the box flipped between the two whenever entries
  streamed in, the line-height knob changed or a drag released, and the thread scrollbar
  resized with it.
- The composer no longer measures its ceiling against the window (80% of the viewport minus a
  fixed 220px), which ignored the title bar, the status bar and any open bottom dock and could
  squeeze the thread below its minimum. Only the expand toggle sizes against the window now,
  and it shrinks to keep the thread list visible.
- The agent message context menu no longer advertises `Add Selection to Thread`, which
  could not act on message selections.
- Reopening a task no longer stays stuck on `This task is already open in another Zepi
  window or process`. Each agent process records which Zepi instance spawned it, so a
  connection the same instance replaces takes over its own abandoned lease instead of
  being refused by a stale heartbeat forever.
- The last turn tokens and duration survive reopening a task: they are stored with the task
  state instead of living only in the view, so the footer no longer loses them on restore.
- The composer expand toggle uses Zed full-height mode again (`EditorMode::Full` inside an
  eighty-percent box that shrinks to the panel), so a long draft scrolls inside instead of
  losing its last lines behind the window edge.
- The context usage circle survives reopening a task: the agent publishes current usage when
  it replays a session, and the last known value is also kept with the task state.

### Tests

- A unit test covers the offline, forced-refresh and provider-failure paths of the shared
  catalog refresh.
- ACP tests cover external writes and private-path rejection; gpui asserts cover line-range
  file links.

## [0.0.1-beta3] - 2026-11-07

### Added

- `npm run smoke`: a P0 bundle check (seeded defaults, freshness of the compiled ACP entry,
  session open, tools, subagents, browser, optional preview) that also runs against a deployed
  bundle via `--root`, masks credentials in its output and fails only on newly red tests.
- The task browser honors a window mode: the Browser use page gains a Window setting
  (Fullscreen by default, plus Maximized and Normal); headed Chromium starts in that mode
  and the page fills the window instead of a fixed 1120x780 viewport. Headless verification
  keeps the fixed viewport, so screenshots and tests measure the same geometry as before.
  The window state is additionally applied over CDP, because Chrome drops the startup
  switches whenever it restores previous session bounds. The page's dropdowns open
  left-aligned under their trigger, and the save notice appears in the section header
  instead of shifting the whole page.
- Closing the task browser window by hand no longer poisons the next browser call: the
  service drops the dead handle on disconnect and relaunches once inside the failing call.
- A headed browser in fullscreen or maximized mode no longer shows the page at half width
  for a moment: the 1120x780 device-metrics override is skipped when the page follows the
  window, because the override died with its CDP session right after launch and Chrome
  snapped to the window size mid-paint.
- Code spans with an elided path (`core/.../Events/ApplicantDto.cs`) become clickable file
  mentions when exactly one worktree file matches the head and tail around `...`. Paths with
  multiple elisions or several matches stay plain, so a reply cannot link to the wrong file;
  the lookup filters by file name before walking worktree entries. Covered by a gpui test.
- The agent message context menu gains `Add Selection to Thread`, reusing Zed's existing
  action instead of introducing a new command.

### Fixed

- Browser cleanup on session dispose no longer raises an unhandled rejection:
  browser.close errors during forced dispose are caught and logged
  (Zepi browser cleanup failed) instead of surfacing as an unhandled promise
  rejection.
- Search outside the project falls back to stock pi disk search: find/grep with a path
  outside the workspace no longer fail; read/ls behavior and the native editor index
  inside the project are unchanged.
- browser_open and browser_agent_command accept the page address as url or uri
  (a missing address now reports a clear error instead of an MCP validation failure).
- Creating and reading unsaved new files through the agent works again: native file tools tolerate paths that exist only in editor buffers, not yet on disk.
- Shell commands on Windows no longer leave background jobs behind: the command's shell reaps its
  own jobs on exit, so a finished command cannot stall its supervisor or leak a process.
- Project commands on Windows run in the private copy as intended: the supervisor resolves the
  captured-inputs link, so the project's own `node_modules` can no longer satisfy a dependency
  that was never captured.
- Replacing a file that a reader or a virus scanner still holds open no longer fails the whole
  operation on Windows: atomic writes back off and retry. Before this, the MCP status in
  Extensions could stay stale, a healthy project verification could fail with a rename error,
  and the preview supervisor could survive its run without a cleanup acknowledgement. The MCP
  status now also republishes itself while a task is open, so a lost write heals on its own.
- An editor left open for more than six hours no longer loses the agent's Extensions state: the
  runtime lease renews itself, so the MCP panel keeps seeing a live runtime instead of expiring
  it and going blank until restart.
- A finished turn no longer leaves the last lines of a reply unreachable behind the
  status row: the message list remeasures its tail and returns to the bottom when the
  turn ends, instead of keeping stale row heights until the thread is reopened.
- `PRODUCT_VERSION` reports `0.0.1-beta3` in the About window, matching the changelog.
- `npm run test:acp` no longer fails on load: the removed Computer Use suite was still listed
  in `package.json`.
- `AGENTS.md` matches the pinned stack (pi 1.5.0, English and Russian READMEs) and asks the
  agent to offer `npm run smoke` after medium or large changes, never as a pre-build gate.

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
- `Browser use` settings page under the AI section: the detected Chromium-family browser,
  a pinned executable chosen through the file picker, keeping the task browser open between
  turns, and on-demand installation of the pinned Chromium into the Zepi browsers directory
  with the bundled playwright. Browser input text model settings moved here from
  `pi Models & Credentials`.

### Changed

- Agent startup is about twice as fast: MCP servers whose tools are codemode-only now connect
  in the background, in parallel, after the session opens, instead of blocking it. Servers with
  direct tools still connect before the session is published, and the first prompt waits for the
  background connections at the turn boundary, so a tool catalog is never half-built. In ACP
  probes, opening a 250 KB thread with five MCP servers went from 7.5 s to 3.3 s.
- The ACP entry is compiled to plain JS (`npm run build:acp`, esbuild): the runtime loads
  `packages/acp/src/cli.js` instead of paying the tsx loader cost on every start. Dev launch
  (`win-run.py`) and bundle deploy build it automatically.
- Browser executable resolution order: pinned path, Zepi browsers directory, playwright cache,
  then system Chrome or Edge; re-resolved on every browser launch, so settings changes apply
  inside a running task.
- Deep links from the agent panel open the AI settings with the `#ai` filter, showing both
  the `pi` and `Browser use` sections.

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
  directly instead of being copied into the project. Private workspace storage
  (`.git`, `.local`, `.pi`, review temps) stays restricted.
- File links also resolve code spans written relative to a subfolder or nested repo: a unique
  suffix match against the project file index becomes a clickable link; ambiguous matches stay
  plain code.

### Removed

- Tool access guards and the Agent Access toggle: Zepi always runs pi in full access now.
  Permission prompts, the footer access selector and the old Agent Access /trust command are
  gone; policy extensions such as pi-guardrails can be layered on top instead.
- Bundled Chromium from the installer and the bundle (about 700 MB): a system Chrome or Edge
  is used when present, and the pinned Chromium installs on demand from the `Browser use`
  page.

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

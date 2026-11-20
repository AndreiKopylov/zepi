# Changelog

All notable changes to Zepi are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions use
[Semantic Versioning](https://semver.org/).

## [0.0.5] - 2026-11-21

### Added

- Zepi's own brand mark across the app: new artwork for the Windows icon set (a per-size
  vector render, 7 ICO frames 16-256 px instead of one downscaled bitmap), the app-icon PNG
  pair per channel (1x 512, @2x 1024, as upstream Zed ships them instead of one file copied
  into all eight names), and the welcome mark. The mark appears where the product shows
  itself: window icon, taskbar, shortcuts, the welcome screen and its watermark.

### Changed

- The application names itself: `display_name()` says Zepi / Zepi Dev / Zepi Preview / Zepi
  Nightly, so the window title, Alt-Tab, About and update notifications stop saying "Zed".
  The executable's own metadata follows: `windows_resources` is vendored into the overlay and
  reports FileDescription/ProductName "Zepi Dev" and CompanyName "Zepi contributors", keeping
  the upstream attribution ("Based on Zed by Zed Industries, Inc.").
- `zepi-mark.svg` and `zed_logo.svg` carry the same mark as a flat monochrome glyph. These two
  are drawn through `gpui::svg()`, which rasterizes the file and tints it with the element's
  text color, so a mark with its own background filled the whole slot instead of drawing the
  glyph.
- Fresh profiles seed `auto_update: false`. The seeded `true` only made sense for an installed copy;
  a portable unpack runs the per-machine installer into a UAC wall. Update manually (reinstall or
  re-unpack), or flip the setting if you want the updater to run.
- Dependency refresh: pi 1.5.2 -> 1.6.0 (coding-agent, ai, tui and the package overrides). The
  one breaking change that touches Zepi is extension components' `render()` returning
  `readonly string[]`; passive component rendering (`pi-decorations`) copies the lines before
  publishing them to the native surface. Version guards (the bundled-runtime check in runtime
  settings, the `zepi-runtime.json` manifest check, ACP agent info) and test expectations now
  say 1.6.0. Notable behavior riding along: undici security updates, find tool path patterns
  relative to the search directory, new default models for some providers.
- README (EN/RU) no longer pins the Zed base and the bundled pi version inline (each release
  lists them), documents the existing GitHub-Releases auto-update instead of the stale
  "no auto-update" limitation, and links pi at its current home (earendil-works/pi).

## [0.0.4] - 2026-11-20

### Removed

- Keep/Reject review for agent edits: the captured-copy shell workflow, binary backup
  reviews, text-review journal replay and the review rows are gone. `bash`/`powershell`
  are pi's built-in in-place shells (like stock pi and Zed's terminal); write/edit land
  in editor buffers and on disk with plain editor undo as the safety net, and git remains
  the review tool for project changes. The tracking temp dirs (`commands`, `commands-pwsh`,
  `text-reviews`) are no longer created.
- The delegated-agent card body: the task line, the inline transcript preview, the
  in-card scrollbar with its drag state and the embedded pending tool call. A subagent's
  transcript stays one click away in its own view; the task moved to the header tooltip.

### Changed

- Windows shell fallback is pi's built-in `powershell` (pi ships no `bash` builtin on
  Windows); the defaultTools seed keeps `-bash`, `+powershell`.
- Delegated agents are one-bar cards: title, `role · model`, then a single state line that
  keeps its slot in every state - `Queued · waiting to start`, the live step while running
  (`Working · 5 steps · powershell · …`), `Completed · 60.8s · … tokens · $…`. The role/model
  line no longer changes when a run ends, so cards stop resizing; Open (full screen) sits in
  the header next to Stop, a child that is waiting for permission still shows that prompt in
  its card, and failures keep their error under the header.

### Fixed

- Stopping a delegated agent left its card spinning in `Stopping` forever. The run's own
  cleanup cancel re-armed the row after it had already reported `Cancelled`, so the card
  never left `in_progress`.
- A `bash` entry in a role's tool list selected nothing on Windows, where pi ships the
  `powershell` tool instead: `worker` agents ran without any shell. The `bash` category now
  resolves to the platform shell.
- Elicitation forms collapsed multi-line text fields (skill bodies and other values with
  line breaks) into a single-line editor, so the answers lost their lines.

## [0.0.3] - 2026-11-19

### Added

- Keep/Reject review for binary shell outputs (xlsx, pptx, images): they apply to the project
  with a pre-command backup and get review rows styled like code rows (file-tree icon, same
  background, hover-only buttons). Reject restores the backup, created files are deleted.
  Reviews survive session reloads and drop out of the list when the target file was deleted.
- `powershell` tool: the captured-copy + review workflow with native PowerShell syntax.
- Text review replay after a reload: both sides of the diff travel in the agent journal
  (write tool and shell merges), covering created files, autosaved intermediates and CRLF saves.

### Changed

- Much faster Rust dev builds: `release-fast` defaults to line-tables-only debug info
  (`ZEPI_DEBUG=full` restores full), workspace crates compile incrementally (a warm rebuild
  after an overlay edit is ~3.5 min instead of ~40), and sccache caches registry deps and
  C/C++ build scripts when installed under `tools/`. New `scripts/win-build-bg.cmd` builds
  detached on a core mask (default 10 of 16) at below-normal priority, so a full rebuild does
  not freeze the desktop; single-crate type-checks live in `scripts/win-check.cmd [crate]`.
- The `preview` development tool is removed; the `bash` tool keeps the captured-copy +
  keep/review workflow but runs commands in-process (no supervisor/runner helper chain, no
  auto-started Node). The injected browser usage "guidelines" and no-op lifecycle stubs are
  gone from the system prompt.
- Auto-update is diagnosable and more robust: it logs the resolved release tag and the
  newer/up-to-date decision, busts edge/proxy caches on `releases/latest`, and launches the
  installer via `Start-Process -Verb RunAs` instead of `explorer.exe`.
- Auto-update staging moved from the install dir (Program Files, not writable without
  elevation) to `%LOCALAPPDATA%\Zepi\updates`; the `auto_update_helper` crate and its
  quit-time finalize hook are gone (the Inno flow never used them). The installer runs
  outside the app's kill-on-exit job so it outlives the app it updates, and closes running
  instances at install time (`CloseApplications=force`; unregistered apps cannot be closed
  gracefully, so unsaved editor buffers are lost — the pi journal survives). A completed
  download is cached and reused instead of re-downloaded on every check. The app keeps an
  honest `Installing` status until the external installer finishes.
- Shell capture: 16 MiB per binary input and 32 MiB total; workspace office/image binaries are
  captured even outside the `files` subset (an empty list captures all) so Reject always has a
  pre-command backup; writes that bypass the copy are detected by a workspace metadata diff.
- Keep All / Reject All settle binary and text reviews together; the edits list auto-expands
  for new reviews unless the user collapsed it.
- Startup retention sweeps: shell run directories and text-review snapshots older than 14 days,
  and runtime leases of dead processes.

### Fixed

- Fresh installs enable auto-update: installed builds are detected by the absent dev marker
  (`zepi-root.txt` next to the exe), not the `ZEPI_INSTALLED_RUNTIME` env var real installs
  never had.
- Auto-update no longer fails with "access denied" on non-elevated installs (staging under
  Program Files); a still-running previous installer or a missing data dir no longer aborts
  the check, and a declined UAC prompt is reported as an error instead of a false "updated".
- `win-build.cmd installer` consumes its argument instead of forwarding `installer` to cargo.
- Spreadsheet cell editing accepts the space bar (gpui reports it as the `space` keystroke).
- No flashing npm/cmd console at session start: Zed's language-server version check runs npm
  as `node npm-cli.js` instead of a visible `npm.cmd`.
- Rejecting an agent-created file now deletes it. Such buffers were tracked as a modification of
  an empty base, so reject wrote back a blank file; they are tracked as created (before the edit,
  with a scheduled review diff so the keep/reject rows still appear).
- Outside-capture inspection failures (locked or uncaptured files) no longer abort the shell
  call or drop reviews of already-applied binaries.
- Reject All keeps journal reviews of preserved buffers (external overlap) so their edits
  survive a reload; one unreadable file no longer aborts replay of the remaining reviews.
- Rejecting an already-deleted created binary settles cleanly; swept backups hide Reject
  instead of failing forever; superseded text-review snapshots are removed on settle.
- Workspace metadata scans batch their lstat calls, keeping shell calls fast on Windows.

## [0.0.2] - 2026-11-17

### Changed

- Browser engine replaced: the native Rust/CDP `zepi_browser` is removed in favour of
  **browse** (opencode, MIT) — a Rust relay + Chromium extension driving the user's real
  browser (logins/SSO). Installed/updated on demand from Settings > Browser (folder choice,
  pairing code, extension link, behavior and tools-mode settings); the MCP entry self-heals
  if deleted.
- Browser tools default to **execute** (code) mode: the agent drives the page with
  Playwright-style JavaScript in one call instead of many discrete tool calls; plain mode
  remains selectable. Relay tools-mode is enforced on session start.
- Installer version is derived from `PRODUCT_VERSION` (single source of truth, same value as
  About); `build-installer.ps1 -Version` is now an optional override.
- Faster session start: the local model-catalog rebuild is skipped when models/settings/auth are
  unchanged, and browse self-heal / relay-mode enforcement moved off the new-session path
  (runs at startup and on settings change).
- Stability: a failing MCP server now backs off (circuit breaker with cooldown) instead of being
  retried in a tight loop, and recovers once the cooldown passes; a dead browse relay is revived on
  session start (endpoint probe) with a restart backoff, no polling.
- The bundled agent is presented as **pi** with the pi logo (agent menu and thread header) instead
  of the internal `zepi-pi` id and a generic sparkle; the gray-to-black gradient fade in the agent
  panel header was removed to match Zed.

### Removed

- `zepi_browser` crate, binary and its installer/bundle references; native browser settings
  (executable path, keep-open, window mode) and native browser tests.
- Dead `npm run native:build` / `native:run` and the macOS-era code behind them (the `Zepi.app`
  bundling, the node+tsx dev-agent launch, the `native-build.json` build record): they never
  worked on Windows, which expects `zepi.exe` and builds via `win-build.cmd`. `npm run dev` now
  runs `scripts/win-run.py` directly.

### Fixed

- Office viewers' "send to agent" now appends to the **displayed** workspace and its active thread.
  Previously it resolved the first workspace and the root thread, so with several projects open the
  selection landed in a different (higher) session than the one being viewed.
- Corrected the `slides_viewer` `emoji_offsets` test expectation: a single emoji in the haystack
  yields one match, not two.
- Spreadsheet save is now atomic: the workbook is written to a hidden `~$…` temp file
  (Excel-style, random suffix) next to the target and renamed over it, so a crash or full disk
  mid-save no longer truncates the file.
- First-start config writes (pi `settings.json`, the stale node-path patch, the keymap and theme
  seeds) now go through temp-file + rename, so a crash mid-write can no longer corrupt them.
- `win-build.cmd`, `win-check.cmd` and `win-test-zepiui.cmd` read the Zed pin (commit and Rust
  toolchain) from `native/zed.json` instead of hardcoding it — the check/test scripts had
  already drifted to a stale commit. Their job count now honors `ZEPI_JOBS`.
- Test repair: the packages/runtime suite silently ran zero tests on Windows (cmd does not
  expand the glob in its test script); it now uses bun auto-discovery. Stale tests of the
  removed native browser (fullAccess, window/keepOpen, the exe ladder, the `access` entry
  point) were deleted or rewritten against current operations, plus a new test covers the
  browse settings validation. The dead `test:browser` script (its files left with `zepi_browser`)
  is removed and `test:preview` no longer names a deleted file.
- Line-ending policy is now explicit in `.gitattributes`: `.cmd`/`.bat`/`.ps1` always check
  out CRLF, `.sh` LF, and vendored code, patch files and test fixtures stay byte-exact so
  `wordcraft-local.patch` keeps matching its LF targets on every machine. Vendored sources were
  left CRLF-free in the working tree to match the (already LF) index.

### Security

- browse components are now downloaded with integrity verification: the bun zip against a
  sha256 pinned in the repo, the relay/cli npm tarballs against the registry's SRI hash. A
  corrupted or tampered download is deleted and never installed.

## [0.0.1] - 2026-11-16

### Added

- Command palette `pi: open AGENTS.md` and `pi: open APPEND_SYSTEM.md`.

### Changed

- MCP stdio servers (incl. browser and direct-exposure) are lazy: they spawn on first tool
  use, with an on-disk schema cache; idle servers close after 15 min and process trees are
  killed on close, so idle threads hold no MCP processes and no orphans remain.
- The agent binary is spawned directly (no pwsh wrapper / conhost per agent).
- Word/slides raster cache evicts least-recently-used instead of oldest-inserted.

### Fixed

- Follow-tail uses a tolerance so fast small output no longer jitters the thread scroll.

## [0.0.1-beta9] - 2026-11-16

### Added

- Command palette `pi:` commands: open settings / mcp / models / models-store files, and
  `pi: update models` (provider refresh + store regen) with a git-pull-style status toast.

### Fixed

- Agent thread scrollbar is stable: accurate total via measure_all; during a resize the
  scrollbar is frozen and heights re-measure once at the end (no oscillation, no bottom wall).
- Launch/relaunch is stable: single-instance with a handshake timeout and a startup watchdog,
  so a stale windowless process can no longer block new windows; Ctrl+Shift+N opens a second
  window in the same process (Zed model) without hangs on real projects.
- browser-ladder test cleans up its temp stub dirs (no more be-*/bp-* litter in %TEMP%).

### Changed

- Installer prefers a compact release-installer profile (line-tables-only debug) when built
  and now ships zepi_browser.exe; dev builds keep full debug.

## [0.0.1-beta8] - 2026-11-15

### Changed

- Browser tools aligned with Playwright MCP: browser_navigate / browser_click / browser_type /
  browser_select_option / browser_press_key / browser_wait_for / browser_take_screenshot /
  browser_snapshot / browser_close; removed the ad-hoc browser_act / browser_batch / browser_status.
- Element targeting by live name/label (element) in click/type/select_option, immune to stale
  indices from SPA re-renders; node ids remain as fallback.

### Fixed

- Dropdowns (Quasar QSelect, MUI Select, native select): browser_select_option opens, scrolls
  virtualized lists, types-to-filter for searchable/async address pickers, waits for options,
  clicks the match and verifies the value; native selects set value directly.
- Field accessible names read from the Quasar field label / ancestor aria-labelledby (were generic
  "Выбрать"); option names keep their own text.
- Clicking a visible menu option selects it directly; stale options route through the select flow.

## [0.0.1-beta7] - 2026-11-15

### Changed

- Browser on native Rust/CDP (`zepi_browser`, chromiumoxide); the agent model drives it directly.
- Agent is a single bun binary (`agent.exe`); run standalone it behaves like the pi CLI.

### Added

- Browser tools: element targeting, press_key, window modes, screenshot-to-file.
- Pinned bun 1.4.4 for reproducible agent builds; `test:lifecycle`, `test:codemode`.

### Removed

- Node/Playwright/jev browser and the agent's Node runtime + node_modules (~300 MB).
  MCP uses system bun; language servers/preview use system Node.

### Fixed

- Browser window/screenshot conflicts; closing no longer leaks processes (OS watchdog);
  codemode worker resolves inside the bun binary.

## [0.0.1-beta6] - 2026-11-15

### Added

- Excel search like Word/PowerPoint: match counter, prev/next, F3/Shift+F3/Enter/Esc, and a
  per-character highlight that does not shift the text.
- Excel in-cell editing keeps rich formatting (color/bold/italic/size) and cell alignment;
  caret and selection track the text, selection persists, and long text grows downward like Excel.
- utils/sheetprobe: dev-only xlsx-to-PNG renderer to calibrate the Excel view against Excel
  without rebuilding Zed.
- Local gridcraft patch (scripts/gridcraft-local-patches/): rich-text runs survive read/save
  round-trips.
- Local wordcraft patch (scripts/wordcraft-local-patches/): Office cloud fonts and a legacy
  .doc (Word 97-2003) reader.
- ui scrollbar thumb_color override for a track-less, editor-style thumb.

### Changed

- Excel rendering parity: stored row heights, blank-line gaps, pt-to-px 4/3, hyphen wrap, text
  padding and integer glyph advances; merged cells, borders and per-run rich text.
- Office viewers (Excel/Word/PowerPoint) share one editor-style autohiding scrollbar.
- Settings grouped into zepi.agent / zepi.office with a sheet.fontScale knob; row/column
  resize marks the sheet dirty without auto-saving.
- Word parity: Word's font-substitution table, .doc paragraph spacing and table margins, and
  per-page (landscape) geometry.

### Fixed

- Headings with an empty numbering label no longer show a stray bullet dot.
- Agent tool rows collapse multi-line shell commands to a single-line summary.

## [0.0.1-beta5] - 2026-11-12

### Changed

- Zed base updated from v1.26.2 to v1.28.1 (same Rust 1.98.1): the overlay was ported with
  the 3-way merge tooling (40 touched upstream files, 30 conflict hunks), adopting the
  agent-client-protocol v5 module layout, the new keystroke parsing API and the upstream
  subagent card / tool-call state, while keeping all zepi behavior.
- The right-hand cluster of a tool row is packed instead of fixed columns: the duration
  shows only for calls that took at least a second (compact, no "Took"), Open File appears
  on hover, and the elements sit 3px apart with no reserved empty space.

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

- Dependency refresh: pi 1.5.0 -> 1.5.2 (coding-agent, ai, tui and the package overrides),
  ACP SDK 1.5.0 -> 1.9.0, MCP SDK 1.32.2 in both manifests, typebox 1.4.6, undici 8.12.4,
  jev-browser 0.2.1, @types/node 24.19.2. The ACP SDK 2.x line was evaluated and left out:
  its handler-context API would require rewriting the adapter. pi 1.5.2 takes the MCP
  refresh lock inside `signInMcpServer`, so the runtime passes the lock through the checked
  credential store instead of wrapping it (the file lock is not reentrant). Version guards,
  agent info and test expectations now say 1.5.2.

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

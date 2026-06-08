# Changelog

All notable changes to this project will be documented in this file.

## [0.1.68] - 2026-06-07

### Changed
- **deps: bump abk to 0.5.49** — `trustee resume` now uses case-insensitive path matching
  on Windows so the current project's sessions appear under "Current Project" instead of
  "Other Projects". Linux/macOS behavior is unchanged (case-sensitive).

## [0.1.68] - 2026-06-07

### Changed
- **deps: bump abk to 0.5.49** — `trustee resume` now correctly identifies the current
  project on Windows with case-insensitive path matching (`C:\Projects\Tanbal` matches
  `C:\projects\tanbal`). On Linux/macOS, paths remain case-sensitive.

## [0.1.67] - 2026-06-07

### Fixed
- **fix(config): use USERPROFILE on Windows when HOME is not set** — `get_config_paths()`
  now falls back `HOME` → `USERPROFILE` → `"."` so the config file
  `~/.trustee/config/trustee.toml` is found correctly when opening a terminal directly
  on Windows (where HOME is typically unset). Previously it fell back to `"."`, looking
  for `.\.trustee\config\trustee.toml` in the current directory and failing.

### Changed
- **deps: bump abk to 0.5.47** — all 9 HOME lookups in abk now fall back to USERPROFILE
  on Windows, fixing checkpoint storage, resume tracker, config, and provider factory.

## [0.1.65] - 2026-06-07

### Changed
- **deps: bump abk to 0.5.46** — bash tool (cats) and executor now use PowerShell
  instead of CMD on Windows, fixing `%` expansion, quote mangling, and single-quote
  issues. Linux/macOS behavior is unchanged.

## [0.1.63] - 2026-06-06

### Added
- **feat(tui): configurable auto-handoff when context exceeds threshold** — new
  `[tui.auto_handoff]` section in `trustee.toml` with `enabled` (default: false) and
  `context_threshold` (default: 170000). When enabled, the TUI monitors context token
  counts from ABK API calls and immediately cancels the workflow + triggers a session
  handoff (fresh context with LLM-generated briefing) once the threshold is exceeded.
  No manual Ctrl+H needed. The cancellation prevents wasted API calls past the limit.

### Changed
- **deps: bump abk to 0.5.44** — replaces unix-only `uname` crate with cross-platform
  `std::env::consts` + `hostname` crate. Trustee-tui now compiles on Linux, macOS, and
  Windows.

### Fixed
- **fix: cross-platform build (Windows/macOS/Linux)** — gate Unix-only SIGTERM
  handler with `#[cfg(unix)]`, suppress unused `TuiSink::shared` warning.
- **fix(init): prefer existing `~/.trustee/config/trustee.toml`** — `trustee init`
  now reads the user's installed config first, only falling back to the project's
  `config/trustee.toml` if no installed config exists. Prevents `init --force` from
  overwriting user customizations.

## [0.1.62] - 2026-06-06

## [0.1.61] - 2026-05-22

### Changed
- **deps: bump abk to 0.5.44** — cross-platform checkpoint (removes unix-only uname).

## [0.1.60] - 2026-05-19

### Fixed
- **fix(tui): restore terminal cleanly on panic, SIGTERM, and abnormal exit** — panic hook
  now sends `DisableMouseCapture`, `DisableBracketedPaste`, and `cursor::Show` in addition
  to `LeaveAlternateScreen`; a SIGTERM handler restores the terminal and exits cleanly;
  bumps trustee-tui to 0.1.35.

## [0.1.59] - 2026-05-18

## [0.1.58] - 2026-05-18

### Changed
- **deps: bump abk to 0.5.42** — fix trustee resume --list.

## [0.1.57] - 2026-05-18

### Changed
- **deps: bump abk to 0.5.41** — fix trustee resume.

## [0.1.56] - 2026-05-11

### Fixed
- **fix(tui/handoff): restore original session after mistake-ENTER+ESC** — bumps
  trustee-tui to 0.1.33. Pressing ENTER by mistake, then ESC, then Ctrl+H now correctly
  performs a handoff using the original session context instead of starting a new
  history-less session.

## [0.1.55] - 2026-05-11

### Fixed
- **fix(tui/handoff): cancel briefing on tool calls, fix instruction order** — bumps
  trustee-tui to 0.1.30 which fixes Ctrl+H session handoff: briefing is cancelled
  immediately if the LLM makes a tool call, and the hardcoded instruction is now
  prepended before any user hint text to prevent LLM confusion.

## [0.1.51] - 2026-05-04

### Added
- **feat(tui): session handoff (Ctrl+H)** — pressing Ctrl+H in trustee-tui
  generates an LLM-authored briefing (up to 50 lines) from the current session
  and immediately starts a brand-new session with the briefing as the first user
  message. Provides a clean context handoff when conversations grow large.
  - Ctrl+H while idle: triggers handoff immediately
  - Ctrl+H while running: cancels the workflow first, then hands off
  - Ctrl+H with no prior session: shows "Nothing to hand off" and does nothing
  - Briefing generation uses same provider/config as the running session
  - New session is created under the same project hash (preserving continuity)
  - Old session checkpoint files are never deleted (non-destructive)
  Bumps trustee-tui to 0.1.29.

## [0.1.50] - 2026-05-04

### Fixed
- **checkpoint: eliminate all `_final_` duplicate checkpoint files** —
  `create_final_checkpoint_and_get_resume_info` no longer writes any checkpoint
  file. It now exclusively returns `ResumeInfo` built from the latest existing
  checkpoint written by the workflow loop, completely eliminating `_final_` files.
  All checkpoint writes happen only via `create_checkpoint` (one `_analyze_` file
  per iteration). Bumps abk to 0.5.39, trustee-tui to 0.1.28.

## [0.1.49] - 2026-05-04

### Changed
- *(published in error — same code as 0.1.48, no functional changes)*

## [0.1.48] - 2026-05-04

### Fixed
- **checkpoint: reduce duplicate `_analyze_` / `_final_` file pairs** —
  `create_final_checkpoint_and_get_resume_info` now skips writing a `_final_`
  file when a checkpoint for the current iteration already exists. Bumps abk
  to 0.5.38, trustee-tui to 0.1.27.

## [0.1.47] - 2026-05-03

### Changed
- **deps: bump trustee-tui to 0.1.25** — `list` tool now shows directory path
  in spinner hints alongside read/edit/write/multiedit (abk 0.5.37 via tui).

## [0.1.46] - 2026-05-03

### Changed
- *(pre-existing release — no changelog entry)*

## [0.1.45] - 2026-05-03

### Fixed
- **fix(tui): `✓ read <file>` regression** — `ToolDone` in `tui_sink.rs` was
  sending `hint = Some("<file>")` from parsing cats tool output (whose first
  line is literally `<file>`), shadowing the correct path hint captured at
  `ToolPending` time. Removed `extract_path_from_content` — ToolDone now only
  passes `description` (bash tools) as hint; file tools fall back to the pending
  hint in `app.rs`. Bumps trustee-tui to 0.1.24.

## [0.1.44] - 2026-05-03

### Changed
- **chore: restore normal trustee-tui publishing** — reverted the short-lived
  `publish = false` experiment on trustee-tui. Both crates continue to publish
  to crates.io as before. Bumps trustee-tui dep to 0.1.23. No functional changes.

## [0.1.43] - 2026-05-03

### Changed
- **chore: trustee-tui is now a private workspace crate** — removed from crates.io
  (`publish = false`). It has no standalone value outside trustee and is versioned
  together with the main binary. No functional changes.

### Fixed
- **fix(tui): duplicate spinner lines for parallel same-name tool calls** — switched
  `pending_tool_lines` from `HashMap` to `Vec<(name, idx, hint)>` so parallel calls
  (e.g. 3× `nghr_get_task`) each get their own spinner line instead of overwriting
  each other's index, which left orphan spinners that never cleared.
- **fix(tui): `✓ read <file>` now shows the actual file path** — carried the hint
  captured at `ToolPending` time (from `extract_hint` in abk) forward through the
  Vec entry so `ToolDone` can reuse it without re-parsing content.

## [0.1.42] - 2026-05-03

### Fixed
- **fix(tui): duplicate spinner lines for parallel same-name tool calls** —
  switched `pending_tool_lines` from `HashMap` to `Vec<(name, idx, hint)>` so
  parallel calls (e.g. 3× `nghr_get_task`) each get their own spinner line
  instead of overwriting each other's index, leaving orphan spinners.
- **fix(tui): `✓ read <file>` now shows the actual file path** — hint captured
  at `ToolPending` time is carried forward to `ToolDone` via the Vec entry.
- **deps: bump trustee-tui to 0.1.22**

## [0.1.41] - 2026-05-03

### Changed
- **deps: bump abk to 0.5.36, trustee-tui to 0.1.21** — animated spinner tool
  display: each native tool call shows a live braille spinner that resolves to
  ✓/✗ in-place; file path shown for read/edit/write tools, command/description
  for bash. The old `🔧 Executing N tools: [...]` header is removed.

## [0.1.40] - 2026-05-02

### Changed
- **deps: bump abk to 0.5.35, trustee-tui to 0.1.20** — picks up cats 0.1.21:
  `edit` multiple-matches error now shows exact line numbers and surrounding
  context, preventing the loop of retries and bash-append duplicates.

## [0.1.39] - 2026-04-29

### Changed
- **deps: bump abk to 0.5.34, trustee-tui to 0.1.19** — picks up cats 0.1.20 with
  clearer EOF truncation errors, multiedit atomicity messages, and compact log
  truncation markers.

## [0.1.38] - 2026-04-29

### Fixed
- **config: hardcoded version in trustee_default.toml** — removed `[cli].version` from the
  default config file; both `[agent].version` and `[cli].version` are now injected at runtime
  from `CARGO_PKG_VERSION` so they always match the binary without manual updates.

## [0.1.37] - 2026-04-29

### Changed
- Updated `abk` dependency to 0.5.33
- Updated `trustee-tui` dependency to 0.1.18

## [0.1.36] - 2026-04-22

### Changed
- Updated `abk` dependency to 0.5.28
- Updated `umf` dependency to 0.2.6

## [0.1.35] - 2026-04-15

### Changed
- Updated abk dependency to 0.5.27

### Fixed
- Fixed infinite retry cascade on slow LLM providers — streaming workflow now has a retry counter (max 3) with exponential backoff instead of infinite retries
- Increased connection pool idle timeout from 60s to 600s to prevent slow streaming connections from being killed mid-response
- `pool_idle_timeout` is now configurable via `LLM_POOL_IDLE_SECONDS` env var

## [0.1.34] - 2026-04-15

### Changed
- Switched abk and umf dependencies from local paths to published crates (abk 0.5.26, umf 0.2.5)
- Updated trustee-tui to 0.1.16

## [0.1.33] - 2026-03-30

### Added
- TUI: Mouse passthrough toggle (`Ctrl+O`) — temporarily disables mouse capture to allow native terminal text selection and copy. Any keypress returns to normal TUI mode. Visual banner shown during passthrough.

### Changed
- Updated trustee-tui to 0.1.15

## [0.1.32] - 2026-03-24

### Changed
- Updated to abk 0.5.23 (fixes TUI session continuity iteration counter reset bug)
- Updated trustee-tui to 0.1.14

## [0.1.31] - 2026-03-24

### Added
- TUI: Tab-based focus cycling across Output, Todo, and Input panels with visual border indicators
- TUI: Visible block cursor in input box for clear editing position
- TUI: Mouse click-to-focus support — click on any panel to switch focus
- TUI: Mouse scroll wheel support — scroll the panel under the cursor

### Fixed
- TUI: Improved scroll behavior, input scrolling, and layout width calculations
- TUI: Fixed last line of output/input cut off from view (scroll estimation buffer)

### Changed
- Updated to abk 0.5.22
- Updated trustee-tui to 0.1.13

## [0.1.30] - 2026-03-23

### Added
- TUI: Vertical todo list panel (20% width, right side) that shows the LLM's task list from `todowrite` tool calls
- TUI: Tool call descriptions in output — bash commands show `🔧 bash — <description>` instead of raw output
- TUI: Context token count in API call info line (`Context=<n>` between call number and mode)

### Changed
- Updated to abk 0.5.20 (ToolCompleted events emitted, description field on ToolExecutionResult)
- Updated trustee-tui to 0.1.12 (todo panel, tool descriptions, context tokens)

## [0.1.29] - 2026-03-22

### Added
- TUI session continuity: multiple commands in the TUI now continue the same session instead of starting fresh each time — ABK returns `ResumeInfo` after each task and the TUI passes it back on the next command

### Changed
- Updated to abk 0.5.19 (in-memory ResumeInfo for session continuity)
- Updated trustee-tui to 0.1.11 (session continuity integration)

## [0.1.28] - 2026-03-22

### Changed
- Updated to abk 0.5.18 (ReasoningChunk output events)
- Updated trustee-tui to 0.1.10 (grey reasoning display)

## [0.1.27] - 2026-03-21

### Fixed
- CLI: Fixed streaming text printed one word/token per line — `StdoutSink` now uses `print!` (no newline) + flush for `StreamingChunk` events instead of `println!`
- TUI/CLI: Fixed duplicate LLM response — when response was already streamed chunk-by-chunk, `handle_content_response` no longer emits a second `LlmResponse` event

### Changed
- Updated to abk 0.5.17 (streaming-aware content response, StdoutSink inline streaming)
- Updated trustee-tui to 0.1.9

## [0.1.26] - 2026-03-21

### Fixed
- TUI: Fixed top box appearing empty — scroll position `u16::MAX` (65535) pushed viewport past all content. Now properly clamps scroll to `content_height - viewport_height` so the last visible page fills the entire box
- TUI: Fixed streaming text rendered line-by-line (one word/token per line) — streaming SSE deltas now append to the last line (print-style) instead of creating new lines (println-style)
- TUI: Added `TuiMessage::StreamDelta` variant for continuous streaming display

### Changed
- Updated to abk 0.5.16 (StreamingChunk and LlmResponse events emitted to OutputSink)
- Updated trustee-tui to 0.1.8
- Updated default max_tokens from 4000 to 16000 and max_history from 100 to 200

## [0.1.25] - 2026-03-15

### Fixed
- TUI: Added text wrapping to output box — long lines now wrap instead of being truncated/disoriented
- TUI: Fixed auto-scroll to reliably show latest output (uses u16::MAX for scroll-to-bottom)
- TUI: Scroll up/down/page keys now work correctly from auto-scrolled position
- ABK: All raw `println!`/`eprintln!` calls in agent, checkpoint, and provider modules now respect TUI mode — prevents text corruption in TUI display

### Changed
- Updated to abk 0.5.15

## [0.1.24] - 2026-03-15

### Fixed
- TUI: Removed process-global `dup2` stdout/stderr redirect that prevented ratatui from rendering during workflows
- TUI: Uses `abk::observability::set_tui_mode` to suppress console output at the source instead of fd-level redirect
- TUI: Output box now updates in real-time as workflow content is tailed from the log file
- TUI: Removed `libc` dependency (no longer needed without `dup2`)

### Changed
- Updated to abk 0.5.14 (TUI mode flag, console output suppression)

## [0.1.23] - 2026-03-15

### Fixed
- TUI: Clear welcome text when a new task starts (output box now starts clean)
- TUI: Log file tailer now reads raw bytes instead of line-by-line, capturing streaming reasoning tokens that have no trailing newlines
- TUI: Flush partial content (>80 chars) accumulated without newlines so reasoning appears progressively

### Changed
- Updated to abk 0.5.13 (ANSI-free log files, reasoning inline fix)

## [0.1.22] - 2026-03-14

### Fixed
- Fixed silent streaming failure — actual errors now logged and displayed with full error chain
- Streaming retries on transient errors before giving up
- Extended streaming timeout from 120s to 600s for complex tasks

### Changed
- Updated to abk 0.5.12

## [0.1.21] - 2026-03-14

### Fixed
- Fixed premature session termination on `finish_reason: "network_error"` from LLM SSE streams

### Changed
- Updated to abk 0.5.11 (streaming retry on network errors, stream error logging)

## [0.1.20] - 2026-03-14

### Changed
- Updated to abk 0.5.10 (consolidated logging, reasoning in log files, removed log_file backward compat)

## [0.1.19] - 2026-03-13

### Added
- Async TUI loop with `tokio::select!` for concurrent workflow execution (Task 52)
- TUI wired to ABK's `run_from_raw_config` for real workflow execution (Task 50)
- TUI loads config/secrets same as CLI mode for consistent behavior

### Changed
- Updated to abk 0.5.8 (tee-write logging, duplicate log fix)
- Log file renamed from `trustee_logs.log` to `trustee.log`
- Non-TUI mode verified working (Task 51)

## [0.1.18] - 2026-03-12

### Changed
- Updated to abk 0.5.7
- Updated to umf 0.2.4

## [0.1.17] - 2026-02-28

### Changed
- Updated to abk 0.5.6

## [0.1.16] - 2026-02-27

### Changed
- Updated to abk 0.5.5 (rustls TLS backend)
- Updated to getmyconfig 0.1.1
- All dependencies now use rustls instead of native-tls
- Enables static linking with musl for portable cross-platform binaries

## [0.1.15] - 2026-02-19

### Changed
- Updated ABK with registry-mcp feature
- Improved MCP tool aggregation

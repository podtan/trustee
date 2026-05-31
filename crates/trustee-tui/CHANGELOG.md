# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.35] - 2026-05-19

### Fixed
- **fix(cleanup): all terminal-restore steps now run on abnormal exit** — cleanup code
  switched from `?` to `let _ =` so `DisableMouseCapture`, `DisableBracketedPaste`, and
  `show_cursor` all execute even if an earlier step fails. Eliminates garbage X10 mouse
  tracking escape sequences (`56M35;83;...`) left in the terminal after a forced quit.

## [0.1.34] - 2026-05-11

## [0.1.33] - 2026-05-11

### Fixed
- **fix(handoff): restore original session after mistake-ENTER+ESC** — `App` now saves a
  `backup_resume_info` snapshot before `execute_command` consumes the live `resume_info`.
  If the task is cancelled (ESC) before producing a real checkpoint (i.e. `ResumeInfo` comes
  back as `None`), the original session's `resume_info` is automatically restored. This means
  pressing ENTER by mistake, then ESC, then Ctrl+H correctly hands off the original session
  instead of starting a new clean (history-less) one.

## [0.1.32] - 2026-05-11

### Fixed
- **fix(handoff): cancel briefing on tool calls, fix instruction order** — `HandoffCaptureSink`
  now cancels the token immediately when the LLM makes a tool call (disobeying "Do NOT use
  any tools"), preventing the old session from being polluted. The hardcoded briefing
  instruction is now placed first so the LLM sees constraints before any user hint text;
  an optional hint from the input box is appended only if non-empty.

## [0.1.25] - 2026-05-03

### Changed
- **deps: bump abk to 0.5.37** — `list` tool now shows directory path in spinner
  hints alongside read/edit/write/multiedit.

## [0.1.24] - 2026-05-03

### Fixed
- **fix: `✓ read <file>` regression** — `ToolDone` was sending `hint = Some("<file>")`
  (parsed from the first line of cats `read` output) which shadowed the correct
  path hint already captured at `ToolPending` time from the call arguments.
  Fixed by removing `extract_path_from_content` entirely from `tui_sink.rs` —
  `ToolDone` now sends only `description` (present for bash tools only), and
  `app.rs` falls back to the pending-side hint for file tools.
- **chore: remove unused `serde_json` dep** — no longer needed after the above fix.

## [0.1.23] - 2026-05-03

### Changed
- **chore: restore normal crates.io publishing** — reverted the short-lived
  `publish = false` experiment. trustee-tui continues to be published alongside
  trustee as before. No functional changes.

## [0.1.22] - 2026-05-03

### Fixed
- **fix: duplicate spinner lines for parallel same-name tool calls** — switched
  `pending_tool_lines` from `HashMap<String, usize>` to
  `Vec<(String, usize, Option<String>)>` so parallel calls with the same name
  (e.g. 3× `nghr_get_task`) each get their own spinner line instead of
  overwriting each other's index, which left orphan spinners that never cleared.
- **fix: `✓ read <file>` now shows the actual file path** — the hint captured
  at `ToolPending` time (file path via `extract_hint` in abk) is now carried
  forward in the Vec entry and reused by `ToolDone`, so the completion line
  shows the real path instead of `<file>`.

## [0.1.21] - 2026-05-03

### Changed
- **Spinner tool display** — native tool calls (read/edit/bash/…) no longer show
  a batch `🔧 Executing N tools: [...]` header. Instead each tool gets its own
  animated braille spinner line that updates in-place to ✓/✗ when done.
- **Useful context on every tool line** — read/edit/write/multiedit show the
  relevant file path; bash shows the description or truncated command.
- **deps: bump abk to 0.5.36** — picks up the `ToolsExecuting.hints` field.

## [0.1.20] - 2026-05-02

### Changed
- **deps: bump abk to 0.5.35** — picks up cats 0.1.21: `edit` multiple-matches
  error now shows line numbers and context, preventing repeated retries and bash
  append fallback that caused duplicate sections.

## [0.1.19] - 2026-04-29

### Changed
- **deps: bump abk to 0.5.34** — picks up cats 0.1.20 with clearer EOF truncation
  errors, multiedit atomicity messages, and compact log truncation markers.

## [0.1.18] - 2026-04-29

### Changed
- Updated `abk` dependency to 0.5.33

## [0.1.17] - 2026-04-22

### Changed
- Updated `abk` dependency to 0.5.28

## [0.1.16] - 2026-04-15

### Changed
- Switched abk dependency from local path to published crate (abk 0.5.26)

## [0.1.15] - 2026-03-30

### Added
- Mouse passthrough toggle (`Ctrl+O`) — temporarily disables mouse capture to allow native terminal text selection and copy

## [0.1.14] - 2026-03-24

### Changed
- Updated to abk 0.5.23 (fixes session continuity iteration counter reset bug)

## [0.1.13] - 2026-03-24

### Added
- Tab-based focus cycling across Output, Todo, and Input panels with bright border on focused panel
- Visible block cursor in input box (SetCursorStyle::SteadyBlock) for clear editing position
- Mouse click-to-focus support — click on any panel to switch focus via cached Rects
- Mouse scroll wheel support — scroll the panel under the cursor (output/todo/input)
- Panel Rect caching during render for mouse hit-testing

### Fixed
- Improved scroll behavior, input scrolling, and layout width calculations
- Fixed last line of output/input cut off from view — added +1 buffer in `estimate_visual_lines()` to compensate for ratatui word-wrapping

### Changed
- Updated to abk 0.5.22

## [0.1.12] - 2026-03-23

### Added
- Todo panel: vertical todo list (20% width, right side) showing LLM's task list from `todowrite` tool calls
- Tool call descriptions in output: bash commands show `🔧 bash — <description>` instead of raw output
- Context token count in API call info line (`Context=<n>` between call number and mode)

### Changed
- Updated to abk 0.5.20 (ToolCompleted events, description field on ToolExecutionResult)

## [0.1.11] - 2026-03-22

### Added
- TUI session continuity: stores `ResumeInfo` from completed workflows and passes it to the next command, enabling seamless multi-turn conversations without losing context
- `TuiMessage::ResumeInfo(Option<ResumeInfo>)` variant for receiving resume info from workflow runner
- `resume_info` field on `App` struct for persisting session state between commands
- Status indicator: "🔄 Session preserved — next command will continue this session"

### Changed
- `run_task_from_raw_config` now returns `TaskResult` (with success/error/resume_info) instead of `Result<(), String>`
- Updated to abk 0.5.19 (in-memory ResumeInfo types, session continuity support)

## [0.1.10] - 2026-03-22

### Added
- Reasoning chunks now render in grey (`Color::DarkGray`) in TUI, matching CLI behavior
- New `TuiMessage::ReasoningDelta` variant for styled reasoning display
- Reasoning starts on a new line (not appended to API Call info)
- Newline separator between reasoning and content sections

### Changed
- Updated to abk 0.5.18 (`OutputEvent::ReasoningChunk` support)

## [0.1.9] - 2026-03-21

### Changed
- Updated to abk 0.5.17 (duplicate LLM response fix, streaming-aware output events)

## [0.1.8] - 2026-03-21

### Fixed
- Fixed scroll clamping: output box no longer shows empty space at bottom when scrolled to end — viewport now fills with content
- Fixed streaming text display: SSE chunks now append continuously (print-style) instead of one-word-per-line (println-style) via new `StreamDelta` message type
- Added `TuiMessage::StreamDelta(String)` variant for continuous streaming text display
- `TuiSink` now maps `StreamingChunk` events to `StreamDelta` instead of `OutputLine`

### Changed
- Updated to abk 0.5.16 (StreamingChunk and LlmResponse output events)

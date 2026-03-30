# Changelog

All notable changes to this project will be documented in this file.

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

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

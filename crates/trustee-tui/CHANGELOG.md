# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

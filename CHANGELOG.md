# Changelog

All notable changes to this project will be documented in this file.

## [0.1.96] - 2026-07-17

### Fixed
- **fix(tui): orphan characters during streaming scroll (no blinking)** — Replaced `terminal.clear()` on every `StreamDelta`/`ReasoningDelta` with a targeted `Clear` widget rendered to the output panel area before the `Paragraph` widget in both `render()` and `render_zoomed()`. This forces every cell in the output region to be marked dirty within the same frame, ensuring the diff-based renderer writes spaces for cells that previously held content from longer/wrapped lines — without causing the full-screen blinking that `terminal.clear()` introduced (trustee-tui 0.1.51).
- This is a known ratatui 0.29 bug ([ratatui#2213](https://github.com/ratatui-org/ratatui/issues/2213), [ratatui#2186](https://github.com/ratatui-org/ratatui/issues/2186)) where `Paragraph` + `.scroll()` + `Wrap { trim: false }` leaves orphan/stale characters during dynamic content changes. Confirmed fixed in ratatui 0.30-beta.
- **deps: bump trustee-tui to 0.1.51.**

## [0.1.95] - 2026-07-17

### Fixed
- **fix(tui): orphan characters during streaming scroll** — Ratatui's diff-based renderer left orphan characters when shorter lines replaced longer ones during scroll. The TUI now forces `terminal.clear()` (full repaint) whenever streaming deltas or reasoning deltas arrive, ensuring every cell is repainted.
- **deps: bump trustee-tui to 0.1.50.**

## [0.1.94] - 2026-07-17

### Added
- **feat(tui): configurable reasoning colors** — Reasoning/thinking text color is now configurable via `[tui.colors]` in trustee.toml. Defaults to `gray` + `dim` (visible on all terminals including Linux VT where `darkgray`/SGR 90 was invisible).

### Fixed
- **fix(tui): reasoning invisible on Linux virtual console** — `Color::DarkGray` (ANSI SGR 90) is unreliable on the Linux kernel VT (`fbcon`/`vgacon`) in raw mode. Default changed to `Color::Gray` (SGR 37) + `Modifier::DIM`.
- **deps: bump trustee-tui to 0.1.49.**

## [0.1.93] - 2026-07-17

### Fixed
- **fix(tui): orphan characters and jagged border boxes during streaming** — Raw `println!`/`eprintln!` calls in abk's `AgentRuntime` and `CleanupManager` bypassed the TUI mode flag and wrote directly to stdout while ratatui held the terminal in raw/alternate-screen mode. All occurrences now route through `tee_println()` or check `is_tui_mode()`.
- **fix(tui): handle terminal resize events** — `Event::Resize` was silently dropped, leaving stale buffer dimensions. The TUI now calls `terminal.clear()` before the next draw when a resize is detected.
- **deps: bump abk to 0.7.9.**
- **deps: bump trustee-tui to 0.1.48.**

## [0.1.92] - 2026-07-17

### Fixed
- **deps: bump abk to 0.7.8** — fixes critical bug where all tool outputs (bash, read,
  write) were sent to the LLM as empty strings. The native OpenAI provider now correctly
  extracts content from `ContentBlock::ToolResult` blocks in tool-role messages.
- **deps: bump trustee-tui to 0.1.47.**

## [0.1.91] - 2026-07-17

### Changed
- **feat: make WASM fully optional** — `cargo build --features tui` now produces a
  native-only build with no wasmtime dependency. Use `cargo build --features tui,wasm`
  to enable WASM extensions (provider + lifecycle). Removed `extension` from default
  abk features in trustee and trustee-tui; added `wasm` feature to forward to `abk/wasm`.
- **deps: bump abk to 0.7.7** — WASM is now opt-in via abk's `wasm` feature.
- **deps: bump trustee-tui to 0.1.46** — removes `extension` from abk features.

## [0.1.90] - 2026-07-17

### Changed
- **deps: bump abk to 0.7.6** — adds native Rust OpenAI provider (`OpenAIProvider`)
  that works without wasmtime. `LLM_PROVIDER=openai-unofficial` (or unset) now uses
  the native provider; `LLM_PROVIDER=openai-unofficial-wasm` uses the WASM extension.
  The `provider` feature no longer requires wasmtime; a new `provider-wasm` feature
  gates it. Also bumps trustee-tui to 0.1.45.
- **refactor(extensions): rename `openai-unofficial` to `openai-unofficial-wasm`** —
  directory and extension ID updated to reflect WASM-based nature.

## [0.1.89] - 2026-07-08

### Changed
- **deps: bump abk to 0.7.5** — checkpoint storage optimization: eliminates
  per-iteration `_agent.json` and `_metadata.json` duplicate files. Agent state
  is now written once as `session_agent.json`; metadata lives in `checkpoints.json`
  index. Reduces a 99-iteration session from 299 files to 101. Fully backward
  compatible with old sessions and all storage modes (Local, DocumentDB, Mirror)
  (task #a1465c3d).

## [0.1.88] - 2026-07-05

### Fixed
- **deps: bump trustee-upgrade to 0.1.2** — adds `aarch64-pc-windows-msvc` target
  triple to `current_target_triple()`, fixing `compile_error!` on Windows ARM64
  builds (issue #46eeec6b).

## [0.1.87] - 2026-07-05

### Fixed
- **deps: bump trustee-tui to 0.1.43** — `HandoffCaptureSink` now captures
  `ReasoningChunk` events. Thinking-capable models that deliver their entire
  briefing through reasoning/thinking tokens no longer produce "briefing
  unavailable". Text chunks take priority; reasoning is used as fallback
  (issue #63ad71c8).

## [0.1.86] - 2026-07-05

### Fixed
- **fix(resume): `resume -i` hang on Windows** — added defensive
  `crossterm::terminal::disable_raw_mode()` on the CLI path to handle
  terminals left in raw mode by improperly terminated TUI sessions
  (issue #2dd0cbb2).
- **deps: bump abk to 0.7.4** — `read_line` now performs blocking stdin
  read in a dedicated OS thread to avoid tokio/IOCP conflict on Windows.
  `tee_println` now flushes stdout explicitly for reliable console output
  on Windows ConPTY.

## [0.1.85] - 2026-06-30

### Added
- **feat(upgrade): `trustee upgrade` subcommand** — new `trustee-upgrade` crate
  that checks GitHub releases, downloads the correct platform binary, verifies
  SHA-256, and performs an atomic binary replacement. Supports `--check`,
  `--force`, `--dry-run`, `--version-target`, `--repo`, and `--prerelease` flags.
  Configuration is driven by `upgrade.toml` (binary name, repo, symlink paths,
  user-agent) with user overrides at `~/.trustee/upgrade.toml`.
- **feat(config): add `upgrade` command to default config** with all CLI args.

### Changed
- **deps: add `trustee-upgrade` (path), `clap` 4.6** — upgrade tool is always
  compiled in (no feature flag needed). `trustee upgrade` is intercepted in
  `main.rs` before ABK CLI dispatch.
- **deps: reqwest 0.13 with rustls** (no native-tls/openssl dependency).

## [0.1.84] - 2026-06-30

### Changed
- **deps: bump abk to 0.7.3** — fixes MCP status panel showing `0/0 (none)` when all
  MCP servers fail. The `McpToolLoader` is now kept even when `has_tools()` returns
  false, preserving `server_statuses` so failed servers with error details are emitted
  to the TUI. Also adds a no-op stub for `emit_mcp_server_statuses()` when
  `registry-mcp` feature is disabled.

## [0.1.83] - 2026-06-28

### Added
- **feat(tui): MCP Server Status Panel** — a dedicated panel in the right column
  (below Todos) showing ✓/✗ status, server name, tool count, and truncated error
  messages for each configured MCP server. Panel height is dynamic (scales with
  server count, caps at 50% of the right column). Data flows through ABK's
  `OutputEvent::McpServerStatus` — ABK stays TUI-agnostic.

### Changed
- **deps: bump abk to 0.7.1** — adds `OutputEvent::McpServerStatus` variant and
  `emit_mcp_server_statuses()` on Agent.

## [0.1.82] - 2026-06-07

### Changed
- **deps: bump abk to 0.7.0** — all raw `eprintln!` calls in abk now route through
  `tee_eprintln()` which suppresses console output in TUI mode. Fixes TUI corruption
  when MCP servers timeout or authentication fails.

## [0.1.81] - 2026-06-07

### Changed
- **deps: bump abk to 0.6.3, cats to 0.1.28** — removes interactive command detector
  (false-positive kills on commands containing `password:`, `Permission denied`, etc.)

## [0.1.80] - 2026-06-07

### Changed
- **deps: bump abk to 0.6.2** — updates cats to 0.1.28, which removes the interactive
  command detector entirely. The bash tool no longer kills commands based on pattern
  matching (e.g. `password:`, `Permission denied`, `[Y/n]`). This eliminates false
  positives where legitimate commands were blocked because their output happened to
  contain these words.

## [0.1.79] - 2026-06-07

### Added
- **feat(mcp): interactive OAuth browser login (PKCE)** — `trustee mcp auth <name>` now
  supports browser-based login with stored tokens and automatic refresh. New `interactive`
  credential type for MCP servers.

## [0.1.69] - 2026-06-08

### Changed
- **deps: bump abk to 0.5.51** — strips Windows UNC prefix (`\\?\`) from
  canonicalized paths before storing and comparing. Fixes `trustee resume`
  not recognizing the current project when the checkpoint was created on
  Windows. Also handles existing checkpoints that already have the prefix.

## [0.1.67] - 2026-06-07

### Fixed
- **fix(config): use USERPROFILE on Windows when HOME is not set** — `get_config_paths()`
  now falls back `HOME` → `USERPROFILE` → `"."` so the config file
  `~/.trustee/config/trustee.toml` is found correctly when opening a terminal directly
  on Windows (where HOME is typically unset). Previously it fell back to `"."`, looking
  for `.\\.trustee\\config\\trustee.toml` in the current directory and failing.

### Changed
- **deps: bump abk to 0.5.48** — all 9 HOME lookups in abk now fall back to USERPROFILE
  on Windows, fixing checkpoint storage, resume tracker, config, and provider factory.

## [0.1.65] - 2026-06-07

### Changed
- **deps: bump abk to 0.5.46** — bash tool (cats) and executor now use PowerShell
  instead of CMD on Windows, fixing `%` expansion, quote mangling, and single-quote
  issues. Linux/macOS behavior is unchanged.

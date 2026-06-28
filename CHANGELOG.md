# Changelog

All notable changes to this project will be documented in this file.

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

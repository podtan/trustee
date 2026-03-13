# Changelog

All notable changes to this project will be documented in this file.

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

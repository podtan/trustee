# Changelog

All notable changes to Trustee will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3] - 2026-02-05

### Changed
- Updated to ABK 0.4.0 with cleaner CLI entry points:
  - `run_from_config_path()` replaces `run_configured_cli_from_config_with_build_info()`
  - `run_from_raw_config()` replaces `run_with_raw_config_and_build_info()`

### Dependencies
- ABK bumped to 0.4.0

## [0.1.2] - 2026-02-05

### Added
- **Version command with build info**: `trustee version` now shows git commit SHA,
  build date, rustc version, and build profile
- **build.rs**: Embeds compile-time metadata (GIT_SHA, BUILD_DATE, RUSTC_VERSION,
  BUILD_PROFILE) into the binary
- Added `version` to enabled CLI commands

### Dependencies
- ABK bumped to 0.3.2

## [0.1.1] - 2026-02-05

### Changed
- Trustee now reads config and secrets itself and passes them to ABK
- Uses `abk::cli::run_with_raw_config()` instead of `run_configured_cli_from_config()`
- Config loaded from `~/.trustee/config/trustee.toml`, secrets from `~/.trustee/.env`
- Environment variables override secrets from .env file
- Prepares architecture for future S3-based configuration loading

### Dependencies
- ABK bumped to 0.3.1

## [0.1.0] - 2025-11-06

### Added
- Initial release of Trustee, a general-purpose morphable agent
- WASM lifecycle plugin system for agent morphing capabilities
- Modular architecture using ABK, CATS, and UMF crates
- CLI interface with comprehensive command structure
- Session management and checkpointing support
- Multiple LLM provider support (OpenAI, GitHub Copilot, Anthropic)
- Configuration system with TOML-based settings
- Tool integration via CATS (Code Agent Tool System)
- Streaming response support via UMF (Universal Message Format)
- Comprehensive logging and observability features

### Features
- **Morphable Agent Framework**: Load different lifecycle plugins to change agent behavior
- **Terminal-First Design**: Optimized for command-line usage and automation
- **Plugin Architecture**: Secure WASM-based plugins for extensibility
- **Session Persistence**: Checkpointing and resume capabilities for long-running tasks
- **Multi-Provider LLM Support**: Support for major LLM providers through WASM abstraction
- **Modular Tool System**: Extensible tool registry for various agent capabilities

### Technical Details
- Built with Rust 2021 edition
- Uses ABK v0.1.24 for core agent functionality
- Integrates CATS v0.1.2 for tool management
- Uses UMF v0.1.3 for message formatting and streaming
- WebAssembly runtime via Wasmtime 25
- TOML configuration with environment variable override support

### Infrastructure
- Comprehensive CLI with subcommands for all major operations
- Configuration validation and management
- Cross-platform support (Linux, macOS, Windows)
- Proper error handling and logging throughout
- Test infrastructure for integration testing

### Documentation
- Complete AGENTS.md development guidelines
- Comprehensive README with usage examples
- Inline code documentation
- Configuration examples and best practices

### Known Limitations
- Initial release focuses on core morphing framework
- Lifecycle plugins need to be developed separately
- Provider plugins distributed separately
- No GUI interface (terminal-first design)

### Future Plans
- Additional lifecycle plugins for different agent types
- Enhanced plugin discovery and management
- Performance optimizations
- Extended configuration options
- Community plugin ecosystem
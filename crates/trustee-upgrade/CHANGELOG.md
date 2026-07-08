# Changelog

All notable changes to this project will be documented in this file.

## [0.1.2] - 2026-07-05

### Fixed
- **fix: add Windows ARM64 target triple** — `current_target_triple()` was missing
  the `aarch64-pc-windows-msvc` cfg arm, causing `compile_error!` on Windows ARM64
  builds. Added the missing cfg arm and updated the `#[cfg(not(any(...)))]` guard
  (issue #46eeec6b).

## [0.1.1] - 2026-06-30

### Added
- Initial release of `trustee-upgrade` — self-upgrade tool that checks GitHub
  releases, downloads the correct platform binary, verifies SHA-256, and performs
  an atomic binary replacement.

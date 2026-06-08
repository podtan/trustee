# Changelog

All notable changes to this project will be documented in this file.

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

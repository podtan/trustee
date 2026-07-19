//! Configuration parsing for trustee-core.
//!
//! Parses `[tui.auto_handoff]` settings from merged TOML config.

use crate::types::AutoHandoffConfig;

/// Parse auto-handoff configuration from a TOML config string.
///
/// Reads `[tui.auto_handoff]` section for `enabled` and `context_threshold`.
/// Returns default values if the section is missing or malformed.
pub fn parse_auto_handoff_config(config_toml: &str) -> AutoHandoffConfig {
    let mut config = AutoHandoffConfig::default();

    if let Ok(table) = config_toml.parse::<toml::Value>() {
        if let Some(tui) = table.get("tui").and_then(|v| v.as_table()) {
            if let Some(ah) = tui.get("auto_handoff").and_then(|v| v.as_table()) {
                if let Some(enabled) = ah.get("enabled").and_then(|v| v.as_bool()) {
                    config.enabled = enabled;
                }
                if let Some(threshold) = ah.get("context_threshold").and_then(|v| v.as_integer()) {
                    config.context_threshold = threshold as usize;
                }
            }
        }
    }

    config
}

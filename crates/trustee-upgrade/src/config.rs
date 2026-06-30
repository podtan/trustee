//! Upgrade configuration — read from `upgrade.toml` (compiled-in default)
//! and optionally overridden by `~/.trustee/upgrade.toml`.

use serde::Deserialize;

/// Configuration controlling the upgrade behaviour.
#[derive(Debug, Clone, Deserialize)]
pub struct UpgradeConfig {
    pub upgrade: UpgradeSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpgradeSection {
    /// GitHub repository in `owner/repo` format.
    #[serde(default = "default_repo")]
    pub repo: String,

    /// Name of the binary inside the archive and on disk.
    #[serde(default = "default_binary_name")]
    pub binary_name: String,

    /// Subdirectory under `~/.trustee/` where the binary is installed.
    #[serde(default = "default_bin_subdir")]
    pub bin_subdir: String,

    /// Name of the symlink in `~/.local/bin/`.
    #[serde(default = "default_symlink_name")]
    pub symlink_name: String,

    /// User-agent prefix sent to the GitHub API.
    #[serde(default = "default_user_agent")]
    pub user_agent: String,

    /// Preferred platform variants to try before the compile-time target triple.
    /// Example: `["x86_64-unknown-linux-musl"]` to prefer static musl binaries.
    #[serde(default = "default_preferred_variants")]
    pub preferred_variants: Vec<String>,
}

fn default_repo() -> String {
    "podtan/trustee".into()
}
fn default_binary_name() -> String {
    "trustee".into()
}
fn default_bin_subdir() -> String {
    "bin".into()
}
fn default_symlink_name() -> String {
    "trustee".into()
}
fn default_user_agent() -> String {
    "trustee-upgrade".into()
}

fn default_preferred_variants() -> Vec<String> {
    vec![
        "x86_64-unknown-linux-musl".into(),
        "aarch64-unknown-linux-musl".into(),
    ]
}

impl Default for UpgradeConfig {
    fn default() -> Self {
        // Parse the compiled-in default toml.  If that ever fails it's a
        // bug in the crate, so panic at startup is appropriate.
        let toml_str = include_str!("../upgrade.toml");
        toml::from_str(toml_str).expect("invalid bundled upgrade.toml")
    }
}

impl UpgradeConfig {
    /// Load config: start with compiled defaults, then overlay
    /// `~/.trustee/upgrade.toml` if it exists.
    pub fn load() -> Self {
        let base = Self::default();

        // Check for user override
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return base,
        };
        let user_path = home.join(".trustee/upgrade.toml");
        if !user_path.exists() {
            return base;
        }

        match std::fs::read_to_string(&user_path) {
            Ok(contents) => {
                match toml::from_str::<UpgradeConfig>(&contents) {
                    Ok(user_cfg) => Self::merge(base, user_cfg),
                    Err(e) => {
                        eprintln!(
                            "⚠️  Warning: failed to parse {}: {e}",
                            user_path.display()
                        );
                        base
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "⚠️  Warning: failed to read {}: {e}",
                    user_path.display()
                );
                base
                }
        }
    }

    /// Overlay user config on top of defaults (user values win when present).
    fn merge(mut base: Self, user: Self) -> Self {
        if user.upgrade.repo != default_repo() {
            base.upgrade.repo = user.upgrade.repo;
        }
        if user.upgrade.binary_name != default_binary_name() {
            base.upgrade.binary_name = user.upgrade.binary_name;
        }
        if user.upgrade.bin_subdir != default_bin_subdir() {
            base.upgrade.bin_subdir = user.upgrade.bin_subdir;
        }
        if user.upgrade.symlink_name != default_symlink_name() {
            base.upgrade.symlink_name = user.upgrade.symlink_name;
        }
        if user.upgrade.user_agent != default_user_agent() {
            base.upgrade.user_agent = user.upgrade.user_agent;
        }
        if !user.upgrade.preferred_variants.is_empty()
            && user.upgrade.preferred_variants != default_preferred_variants()
        {
            base.upgrade.preferred_variants = user.upgrade.preferred_variants;
        }
        base
    }
}

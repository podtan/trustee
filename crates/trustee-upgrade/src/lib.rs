//! Trustee Upgrade — Self-update tool for the Trustee agent binary.
//!
//! This crate checks GitHub releases, downloads the appropriate pre-compiled
//! binary for the current platform, and atomically replaces the running
//! installation.
//!
//! It is a standalone crate (like `trustee-tui`) so it can be:
//! - Invoked as a standalone binary: `trustee-upgrade`
//! - Called programmatically from `trustee upgrade` (via ABK CLI runner)
//! - Used in CI to install a specific version

pub mod config;
mod github;
mod updater;

pub use config::UpgradeConfig;
pub use github::{select_asset, current_target_triple, Asset, Release, ReleaseSource};
pub use updater::{is_newer, Updater, UpgradeOptions, UpgradeResult};

use clap::{Arg, ArgAction, Command};

/// Build the clap CLI command definition.
pub fn build_cli() -> Command {
    Command::new("trustee-upgrade")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Self-upgrade tool for Trustee agent")
        .arg(
            Arg::new("check")
                .long("check")
                .help("Only check for updates without installing")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .short('f')
                .help("Force upgrade even if already up-to-date")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("version")
                .long("version-target")
                .short('v')
                .help("Upgrade to a specific version (e.g. 0.1.84)")
                .value_name("VERSION"),
        )
        .arg(
            Arg::new("repo")
                .long("repo")
                .help("GitHub repository (owner/repo) to download from")
                .value_name("REPO"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("Show what would happen without making changes")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("prerelease")
                .long("prerelease")
                .help("Include pre-release versions")
                .action(ArgAction::SetTrue),
        )
}

/// Run the upgrade from parsed options.
///
/// This is the programmatic entry point for callers like `trustee upgrade`.
pub async fn run_upgrade(opts: UpgradeOptions) -> anyhow::Result<UpgradeResult> {
    let updater = Updater::new(opts);
    updater.run().await
}

/// Returns the crate version (used as a fallback for standalone binary mode).
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

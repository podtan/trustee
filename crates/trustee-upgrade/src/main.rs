//! Binary entry point for the standalone `trustee-upgrade` executable.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let matches = trustee_upgrade::build_cli().get_matches();

    let opts = trustee_upgrade::UpgradeOptions {
        check_only: matches.get_flag("check"),
        force: matches.get_flag("force"),
        dry_run: matches.get_flag("dry-run"),
        prerelease: matches.get_flag("prerelease"),
        target_version: matches.get_one::<String>("version").map(|s| s.to_string()),
        repo: matches.get_one::<String>("repo").cloned(),
        current_version: trustee_upgrade::version(),
    };

    match trustee_upgrade::run_upgrade(opts).await {
        Ok(result) => {
            println!("{}", result.summary());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("❌ Upgrade failed: {e}");
            eprintln!();
            eprintln!("Troubleshooting:");
            eprintln!("  • Check network connectivity to github.com");
            eprintln!("  • Verify the binary is writable (not running as different user)");
            eprintln!("  • Try --dry-run to diagnose without changes");
            eprintln!("  • Check ~/.trustee/logs/ for detailed error logs");
            ExitCode::FAILURE
        }
    }
}

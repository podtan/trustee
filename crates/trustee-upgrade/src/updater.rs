//! Binary download, verification, and atomic replacement logic.

use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::config::UpgradeConfig;
use crate::github::{select_asset, Release, ReleaseSource};

/// Options controlling the upgrade behaviour.
#[derive(Debug, Clone)]
pub struct UpgradeOptions {
    /// Only check for updates, don't install.
    pub check_only: bool,
    /// Force upgrade even if already on the latest version.
    pub force: bool,
    /// Simulate the upgrade without modifying any files.
    pub dry_run: bool,
    /// Include pre-release versions in the search.
    pub prerelease: bool,
    /// Upgrade to a specific version tag (e.g. "0.1.84" or "v0.1.84").
    pub target_version: Option<String>,
    /// GitHub repo (owner/name format).  Overrides config if set.
    pub repo: Option<String>,
    /// Current version of the binary being upgraded (passed by caller).
    pub current_version: String,
}

impl Default for UpgradeOptions {
    fn default() -> Self {
        Self {
            check_only: false,
            force: false,
            dry_run: false,
            prerelease: false,
            target_version: None,
            repo: None,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Result of an upgrade operation.
#[derive(Debug)]
pub struct UpgradeResult {
    /// Previous version (if known).
    pub from_version: Option<String>,
    /// New version installed.
    pub to_version: String,
    /// Path to the upgraded binary.
    pub binary_path: PathBuf,
    /// Whether an actual replacement occurred.
    pub replaced: bool,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

impl UpgradeResult {
    /// Human-readable summary of the result.
    pub fn summary(&self) -> String {
        if self.dry_run {
            return format!(
                "🔍 Dry run — would upgrade {} → {}\n   Target: {}",
                self.from_version.as_deref().unwrap_or("(unknown)"),
                self.to_version,
                self.binary_path.display()
            );
        }

        if self.replaced {
            format!(
                "✅ Trustee upgraded {} → {}\n   Installed at: {}",
                self.from_version.as_deref().unwrap_or("(unknown)"),
                self.to_version,
                self.binary_path.display()
            )
        } else {
            format!(
                "✓ Already up-to-date (v{})\n   Binary: {}",
                self.to_version,
                self.binary_path.display()
            )
        }
    }
}

/// The main upgrade engine.
pub struct Updater {
    source: ReleaseSource,
    opts: UpgradeOptions,
    config: UpgradeConfig,
}

impl Updater {
    /// Create a new updater with the default config.
    pub fn new(opts: UpgradeOptions) -> Self {
        let config = UpgradeConfig::load();

        // CLI --repo overrides config
        let repo = opts.repo.clone().unwrap_or_else(|| config.upgrade.repo.clone());

        let source = ReleaseSource {
            repo,
            token: std::env::var("GITHUB_TOKEN")
                .or_else(|_| std::env::var("GH_TOKEN"))
                .ok(),
            user_agent: config.upgrade.user_agent.clone(),
        };
        Self { source, opts, config }
    }

    /// Create a new updater with an explicit config (for testing).
    pub fn with_config(opts: UpgradeOptions, config: UpgradeConfig) -> Self {
        let repo = opts.repo.clone().unwrap_or_else(|| config.upgrade.repo.clone());
        let source = ReleaseSource {
            repo,
            token: std::env::var("GITHUB_TOKEN")
                .or_else(|_| std::env::var("GH_TOKEN"))
                .ok(),
            user_agent: config.upgrade.user_agent.clone(),
        };
        Self { source, opts, config }
    }

    /// Create a new updater with an explicit release source (for testing / custom repos).
    pub fn with_source(opts: UpgradeOptions, source: ReleaseSource) -> Self {
        Self {
            source,
            opts,
            config: UpgradeConfig::default(),
        }
    }

    /// Run the full upgrade flow.
    ///
    /// Flow:
    /// 1. Determine current version (from env! at compile time)
    /// 2. Fetch the latest (or target) release from GitHub
    /// 3. Compare versions — skip if already up-to-date (unless --force)
    /// 4. Download the correct asset for the current platform
    /// 5. Extract the binary from the archive
    /// 6. Atomically replace the running binary
    /// 7. Optionally re-run `trustee init` to update symlinks
    pub async fn run(&self) -> Result<UpgradeResult> {
        let current_version = &self.opts.current_version;
        let binary_name = &self.config.upgrade.binary_name;
        println!("📍 Current version: v{current_version}");

        // --- Step 1: Resolve target release ---
        let release = if let Some(ref target) = self.opts.target_version {
            println!("🎯 Target version: v{target}");
            self.source.release_by_tag(target).await?
        } else {
            let r = self.source.latest_release(self.opts.prerelease).await?;
            if self.opts.prerelease {
                println!("🔮 Latest release (including prereleases): {}", r.tag_name);
            } else {
                println!("🌟 Latest release: {}", r.tag_name);
            }
            r
        };

        // Normalise the release tag (strip leading 'v')
        let release_version = release.tag_name.trim_start_matches('v').to_string();

        // --- Step 2: Version comparison ---
        if !self.opts.force && !self.opts.dry_run {
            if release_version == *current_version {
                let binary_path = find_current_binary(binary_name, &self.config)?;
                return Ok(UpgradeResult {
                    from_version: Some(current_version.to_string()),
                    to_version: release_version,
                    binary_path,
                    replaced: false,
                    dry_run: false,
                });
            }

            if !is_newer(current_version, &release_version) {
                println!(
                    "⚠️  Installed version (v{current_version}) is newer than release (v{release_version})"
                );
                println!("   Use --force to downgrade anyway.");
                return Ok(UpgradeResult {
                    from_version: Some(current_version.to_string()),
                    to_version: release_version,
                    binary_path: find_current_binary(binary_name, &self.config)?,
                    replaced: false,
                    dry_run: false,
                });
            }
        }

        if self.opts.check_only {
            let binary_path = find_current_binary(binary_name, &self.config)?;
            if release_version == *current_version {
                println!("✓ Already up-to-date.");
            } else {
                println!(
                    "📦 Update available: v{current_version} → v{release_version}\n   Run `trustee upgrade` to install."
                );
                if let Some(ref body) = release.body {
                    println!();
                    println!("Release notes:");
                    println!("{}", truncate(body, 500));
                }
            }
            return Ok(UpgradeResult {
                from_version: Some(current_version.to_string()),
                to_version: release_version,
                binary_path,
                replaced: false,
                dry_run: true,
            });
        }

        // --- Step 3: Select and download the asset ---
        let asset = select_asset(&release, &self.config.upgrade.preferred_variants)?;
        println!(
            "📦 Downloading: {} ({:.1} MB)",
            asset.name,
            asset.size as f64 / 1_048_576.0
        );

        if self.opts.dry_run {
            println!("[dry-run] Would download and install to replace current binary");
            return Ok(UpgradeResult {
                from_version: Some(current_version.to_string()),
                to_version: release_version,
                binary_path: find_current_binary(binary_name, &self.config)?,
                replaced: false,
                dry_run: true,
            });
        }

        let archive_data = download_asset(&asset.browser_download_url, &self.config.upgrade.user_agent).await?;
        println!("✅ Download complete ({:.1} MB)", archive_data.len() as f64 / 1_048_576.0);

        // --- Step 4: Extract binary from archive ---
        let temp_dir = tempfile_dir()?;
        let binary_data = extract_binary(&archive_data, &asset.name, &temp_dir, binary_name)?;

        // --- Step 5: Verify SHA-256 (if checksums are published) ---
        if let Some(expected_hash) = find_checksum(&release, &asset.name) {
            verify_sha256(&binary_data, &expected_hash)?;
            println!("🔐 SHA-256 verified ✓");
        } else {
            println!(
                "⚠️  No SHA-256 checksum found for {name}, skipping verification",
                name = asset.name
            );
        }

        // --- Step 6: Atomic binary replacement ---
        let binary_path = find_current_binary(binary_name, &self.config)?;
        println!("🔧 Replacing binary: {}", binary_path.display());

        replace_binary_atomic(&binary_path, &binary_data)?;
        println!("✅ Binary replaced successfully.");

        // --- Step 7: Post-install — update symlinks if possible ---
        if let Err(e) = update_symlinks(&binary_path, &self.config) {
            eprintln!("⚠️  Warning: could not update symlinks: {e}");
            eprintln!("   Run `trustee init --force` to recreate symlinks manually.");
        }

        Ok(UpgradeResult {
            from_version: Some(current_version.to_string()),
            to_version: release_version,
            binary_path,
            replaced: true,
            dry_run: false,
        })
    }
}

// ============================================================================
// Download
// ============================================================================

/// Download an asset from a URL with a progress bar.
async fn download_asset(url: &str, user_agent: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent(format!("{}/{}", user_agent, env!("CARGO_PKG_VERSION")))
        .build()?;

    let resp = client
        .get(url)
        .header("Accept", "application/octet-stream")
        .send()
        .await
        .context("Failed to start download")?;

    if !resp.status().is_success() {
        bail!(
            "Download failed: HTTP {} from {url}",
            resp.status()
        );
    }

    let total = resp.content_length().unwrap_or(0);

    let pb = indicatif::ProgressBar::new(total);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );

    let mut data = Vec::with_capacity(total as usize);
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        pb.inc(chunk.len() as u64);
        data.extend_from_slice(&chunk);
    }
    pb.finish_with_message("Downloaded");

    Ok(data)
}

// ============================================================================
// Extraction
// ============================================================================

/// Extract the binary from a downloaded archive.
///
/// Supports `.tar.gz` and raw binaries. Future: `.zip` for Windows.
fn extract_binary(archive_data: &[u8], asset_name: &str, temp_dir: &Path, binary_name: &str) -> Result<Vec<u8>> {
    if asset_name.ends_with(".tar.gz") || asset_name.ends_with(".tgz") {
        extract_from_tar_gz(archive_data, temp_dir, binary_name)
    } else if asset_name.ends_with(".gz") {
        // Single-file gzip (binary directly)
        use flate2::read::GzDecoder;
        let mut decoder = GzDecoder::new(archive_data);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .context("Failed to decompress .gz file")?;
        Ok(out)
    } else {
        // Assume raw binary
        Ok(archive_data.to_vec())
    }
}

/// Extract binary from a `.tar.gz` archive.
fn extract_from_tar_gz(data: &[u8], _temp_dir: &Path, binary_name: &str) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    let decoder = GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);

    let mut binary_data = None;

    for entry in archive.entries()? {
        let mut entry = entry.context("Failed to read tar entry")?;
        let path = entry
            .path()
            .context("Failed to read entry path")?
            .display()
            .to_string();

        // Look for the main binary file (e.g. "trustee" or "trustee.exe")
        let file_name = Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let target_names = [
            binary_name.to_string(),
            format!("{binary_name}.exe"),
        ];

        if target_names.contains(&file_name.to_string()) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("Failed to read binary from tar")?;
            binary_data = Some(buf);
            break;
        }
    }

    binary_data.ok_or_else(|| {
        anyhow!(
            "Binary '{binary_name}' not found in archive. Expected a tar member named '{binary_name}' or '{binary_name}.exe'."
        )
    })
}

// ============================================================================
// SHA-256 Verification
// ============================================================================

fn find_checksum(_release: &Release, _asset_name: &str) -> Option<String> {
    // Placeholder: In production, download checksums.txt from release assets
    // and find the SHA-256 hash matching _asset_name.
    None
}

fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<()> {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = hasher.finalize();
    let actual_hex = hex::encode(actual);

    if actual_hex != expected_hex {
        bail!(
            "SHA-256 mismatch!\n  Expected: {expected_hex}\n  Actual:   {actual_hex}"
        );
    }
    Ok(())
}

// ============================================================================
// Binary Replacement
// ============================================================================

/// Find the path to the currently running binary.
fn find_current_binary(binary_name: &str, config: &UpgradeConfig) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("Cannot determine current binary path")?;

    // Resolve symlinks to find the real binary location
    let real = fs::canonicalize(&exe).unwrap_or_else(|_| exe.clone());

    // Check common installation paths
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot determine home directory"))?;

    let candidates = [
        // Standard install: ~/.trustee/bin/trustee
        home.join(format!(".trustee/{}/{}", config.upgrade.bin_subdir, binary_name)),
        // Local bin symlink
        home.join(format!(".local/bin/{}", binary_name)),
        // Cargo bin
        home.join(format!(".cargo/bin/{}", binary_name)),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            let canonical = fs::canonicalize(candidate).unwrap_or_else(|_| candidate.clone());
            if canonical == real || candidate == &exe {
                return Ok(canonical);
            }
        }
    }

    // Fall back to the current exe path
    Ok(real)
}

/// Atomically replace a binary file.
fn replace_binary_atomic(target: &Path, data: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("Cannot determine parent directory of {target:?}"))?;

    let temp_name = format!(
        ".{}.tmp.{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("binary"),
        std::process::id()
    );
    let temp_path = parent.join(&temp_name);

    // Write the new binary to the temp file
    {
        let mut file = fs::File::create(&temp_path)
            .with_context(|| format!("Failed to create temp file: {}", temp_path.display()))?;

        file.write_all(data)
            .context("Failed to write binary data")?;
        file.sync_all().context("Failed to sync temp file")?;
    }

    // Set executable permissions (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_path, perms)?;
    }

    // Rename temp → target (atomic on same filesystem)
    fs::rename(&temp_path, target)
        .with_context(|| format!("Failed to replace binary at {}", target.display()))?;

    Ok(())
}

/// After replacing the binary, attempt to update symlinks.
fn update_symlinks(binary_path: &Path, config: &UpgradeConfig) -> Result<()> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot determine home directory"))?;

    let local_bin = home.join(format!(".local/bin/{}", config.upgrade.symlink_name));

    #[cfg(unix)]
    {
        if local_bin.exists() || local_bin.is_symlink() {
            if let Ok(target) = fs::read_link(&local_bin) {
                if target.parent() == binary_path.parent() {
                    fs::remove_file(&local_bin).ok();
                    std::os::unix::fs::symlink(binary_path, &local_bin)?;
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Version Comparison
// ============================================================================

/// Compare two semver-like version strings.
///
/// Returns `true` if `remote` is newer than `local`.
pub fn is_newer(local: &str, remote: &str) -> bool {
    let local_parts = parse_version_parts(local);
    let remote_parts = parse_version_parts(remote);

    for (l, r) in local_parts.iter().zip(remote_parts.iter()) {
        if r > l {
            return true;
        }
        if r < l {
            return false;
        }
    }

    remote_parts.len() > local_parts.len()
}

fn parse_version_parts(v: &str) -> Vec<u32> {
    let base = v.trim_start_matches('v').split('-').next().unwrap_or("");
    base.split('.')
        .filter_map(|p| p.parse().ok())
        .collect()
}

// ============================================================================
// Helpers
// ============================================================================

fn tempfile_dir() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    let dir = base.join(format!("trustee-upgrade-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.1.83", "0.1.84"));
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("1.0.0", "2.0.0"));
        assert!(!is_newer("0.1.84", "0.1.84"));
        assert!(!is_newer("0.1.84", "0.1.83"));
        assert!(!is_newer("2.0.0", "1.0.0"));
    }

    #[test]
    fn test_parse_version_parts() {
        assert_eq!(parse_version_parts("0.1.84"), vec![0, 1, 84]);
        assert_eq!(parse_version_parts("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version_parts("1.0.0-beta.1"), vec![1, 0, 0]);
    }
}

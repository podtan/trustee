//! GitHub Releases API client.
//!
//! Talks to `https://api.github.com/repos/{owner}/{repo}/releases` to find
//! the latest release (or a specific version) and select the correct asset
//! for the current platform.

use anyhow::{anyhow, Context, Result, bail};
use serde::Deserialize;

/// Where releases are hosted.
#[derive(Debug, Clone)]
pub struct ReleaseSource {
    /// GitHub repository in `owner/repo` format, e.g. `"podtan/trustee"`.
    pub repo: String,
    /// Optional GitHub token for private repos or rate-limit relief.
    /// If `None`, unauthenticated requests are made (60/hr limit).
    pub token: Option<String>,
    /// User-agent prefix sent to GitHub API.
    pub user_agent: String,
}

impl Default for ReleaseSource {
    fn default() -> Self {
        Self {
            repo: "podtan/trustee".to_string(),
            token: None,
            user_agent: "trustee-upgrade".to_string(),
        }
    }
}

/// A GitHub release.
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub prerelease: bool,
    pub draft: bool,
    #[serde(default)]
    pub body: Option<String>,
    pub assets: Vec<Asset>,
}

/// A downloadable asset attached to a release.
#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    #[serde(default)]
    pub api_url: Option<String>,
}

/// GitHub API response for the latest release.
#[derive(Debug, Deserialize)]
struct LatestReleaseResponse {
    tag_name: String,
    name: Option<String>,
    prerelease: bool,
    draft: bool,
    #[serde(default)]
    body: Option<String>,
    assets: Vec<Asset>,
}

impl From<LatestReleaseResponse> for Release {
    fn from(r: LatestReleaseResponse) -> Self {
        Release {
            tag_name: r.tag_name,
            name: r.name,
            prerelease: r.prerelease,
            draft: r.draft,
            body: r.body,
            assets: r.assets,
        }
    }
}

/// GitHub API response for listing all releases.
#[derive(Debug, Deserialize)]
struct _ListReleasesResponse(Vec<LatestReleaseResponse>);

impl ReleaseSource {
    fn api_base(&self) -> String {
        format!("https://api.github.com/repos/{}", self.repo)
    }

    fn build_client(&self) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("{}/{}", self.user_agent, env!("CARGO_PKG_VERSION")));

        if let Some(ref token) = self.token {
            let mut headers = reqwest::header::HeaderMap::new();
            let auth = format!("Bearer {token}");
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&auth)?,
            );
            builder = builder.default_headers(headers);
        }

        Ok(builder.build()?)
    }

    /// Fetch the latest non-prerelease, non-draft release.
    pub async fn latest_release(&self, include_prerelease: bool) -> Result<Release> {
        let client = self.build_client()?;
        let url = format!("{}/releases/latest", self.api_base());

        let resp = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("Failed to connect to GitHub API")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("No releases found for {repo}", repo = self.repo);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GitHub API error {status}: {body}");
        }

        let release: LatestReleaseResponse = resp
            .json()
            .await
            .context("Failed to parse GitHub release response")?;

        if release.draft {
            bail!("Latest release is a draft — no downloadable assets");
        }

        if release.prerelease && !include_prerelease {
            // Fall back to listing all releases to find the latest stable
            return self.latest_stable_release().await;
        }

        Ok(release.into())
    }

    /// Fetch a specific release by tag name (e.g. "v0.1.84" or "0.1.84").
    pub async fn release_by_tag(&self, tag: &str) -> Result<Release> {
        let client = self.build_client()?;
        // Normalise: strip leading 'v' if present for the API path
        let tag_clean = tag.trim_start_matches('v');
        let url = format!("{}/releases/tags/v{}", self.api_base(), tag_clean);

        let resp = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("Failed to connect to GitHub API")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            // Try without 'v' prefix as well
            let url2 = format!("{}/releases/tags/{}", self.api_base(), tag_clean);
            let resp2 = client
                .get(&url2)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await?;

            if !resp2.status().is_success() {
                bail!("Release {tag} not found");
            }

            return Ok(resp2.json::<LatestReleaseResponse>().await?.into());
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GitHub API error {status}: {body}");
        }

        Ok(resp.json::<LatestReleaseResponse>().await?.into())
    }

    /// List all releases, newest first.
    pub async fn list_releases(&self) -> Result<Vec<Release>> {
        let client = self.build_client()?;
        let url = format!("{}/releases?per_page=50", self.api_base());

        let resp = client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .context("Failed to connect to GitHub API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("GitHub API error {status}: {body}");
        }

        let releases: Vec<LatestReleaseResponse> =
            resp.json().await.context("Failed to parse releases")?;

        Ok(releases.into_iter().map(Into::into).collect())
    }

    /// Find the latest non-prerelease, non-draft release by iterating.
    async fn latest_stable_release(&self) -> Result<Release> {
        let releases = self.list_releases().await?;
        releases
            .into_iter()
            .find(|r| !r.prerelease && !r.draft)
            .ok_or_else(|| anyhow!("No stable releases found for {}", self.repo))
    }
}

/// Determine the expected asset name pattern for the current platform.
pub fn current_target_triple() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    { "aarch64-unknown-linux-gnu" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "x86_64-unknown-linux-gnu" }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "aarch64-apple-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "x86_64-apple-darwin" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "x86_64-pc-windows-msvc" }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    { "aarch64-pc-windows-msvc" }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    { compile_error!("trustee-upgrade: unsupported platform — add your target triple here") }
}

/// Select the best matching asset from a release for the current platform.
///
/// Selection order:
/// 1. Preferred variants from config (e.g. `x86_64-unknown-linux-musl`)
/// 2. Exact match on compile-time target triple (e.g. `x86_64-unknown-linux-gnu`)
/// 3. Loose arch + OS match (e.g. contains both `x86_64` and `linux`)
/// 4. Fallback: any `.tar.gz` (Unix) or `.zip` (Windows)
pub fn select_asset<'a>(release: &'a Release, preferred_variants: &[String]) -> Result<&'a Asset> {
    let target = current_target_triple();

    // Strategy 0: check preferred variants first (e.g. musl over gnu)
    for variant in preferred_variants {
        if let Some(asset) = release.assets.iter().find(|a| a.name.contains(variant.as_str())) {
            return Ok(asset);
        }
    }

    // Strategy 1: exact triple match
    if let Some(asset) = release.assets.iter().find(|a| a.name.contains(target)) {
        return Ok(asset);
    }

    // Strategy 2: loose arch + OS match
    let (arch, os) = match target {
        t if t.contains("aarch64") && t.contains("linux") => ("aarch64", "linux"),
        t if t.contains("x86_64") && t.contains("linux") => ("x86_64", "linux"),
        t if t.contains("aarch64") && t.contains("darwin") => ("aarch64", "darwin"),
        t if t.contains("x86_64") && t.contains("darwin") => ("x86_64", "darwin"),
        t if t.contains("x86_64") && t.contains("windows") => ("x86_64", "windows"),
        _ => bail!("Cannot determine arch/OS from target triple: {target}"),
    };

    if let Some(asset) = release
        .assets
        .iter()
        .find(|a| a.name.contains(arch) && a.name.contains(os))
    {
        return Ok(asset);
    }

    // Strategy 3: any archive file
    #[cfg(unix)]
    if let Some(asset) = release.assets.iter().find(|a| a.name.ends_with(".tar.gz")) {
        return Ok(asset);
    }
    #[cfg(windows)]
    if let Some(asset) = release.assets.iter().find(|a| a.name.ends_with(".zip")) {
        return Ok(asset);
    }

    bail!(
        "No matching asset found for target '{target}' in release '{}'.\n\
         Available assets:\n{}",
        release.tag_name,
        release
            .assets
            .iter()
            .map(|a| format!("  - {} ({:.1} MB)", a.name, a.size as f64 / 1_048_576.0))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_target_triple_is_valid() {
        let triple = current_target_triple();
        assert!(triple.contains("-"));
        assert!(triple.len() > 10);
    }

    #[test]
    fn test_select_asset_exact_match() {
        let release = Release {
            tag_name: "v0.1.84".into(),
            name: None,
            prerelease: false,
            draft: false,
            body: None,
            assets: vec![
                Asset {
                    name: "trustee-0.1.84-x86_64-unknown-linux-gnu.tar.gz".into(),
                    size: 1000,
                    browser_download_url: "https://example.com/x86".into(),
                    api_url: None,
                },
                Asset {
                    name: "trustee-0.1.84-aarch64-unknown-linux-gnu.tar.gz".into(),
                    size: 1000,
                    browser_download_url: "https://example.com/arm".into(),
                    api_url: None,
                },
            ],
        };

        let asset = select_asset(&release, &[]).unwrap();
        assert!(asset.name.contains(current_target_triple()));
    }
}

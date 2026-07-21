//! TLS support for the Trustee API server.
//!
//! Provides self-signed certificate auto-generation and rustls server config
//! loading. Certificates are stored at `~/.trustee/certs/{cert.pem,key.pem}`
//! and generated on first run if missing.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// Resolve the default certificate directory: `~/.trustee/certs/`
pub fn default_cert_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".trustee").join("certs")
}

/// Ensure that cert.pem and key.pem exist in `cert_dir`.
///
/// If both files exist, returns their paths immediately.
/// If either is missing, generates a new self-signed certificate pair.
pub fn ensure_certs(cert_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let cert_path = cert_dir.join("cert.pem");
    let key_path = cert_dir.join("key.pem");

    if cert_path.exists() && key_path.exists() {
        tracing::debug!("Using existing certificates at {}", cert_dir.display());
        return Ok((cert_path, key_path));
    }

    fs::create_dir_all(cert_dir)?;

    // Generate self-signed certificate with rcgen
    let cert = rcgen::generate_simple_self_signed(vec![
        "localhost".into(),
        "127.0.0.1".into(),
        "::1".into(),
    ])?;

    let cert_pem = cert.cert.pem();
    let key_pem = cert.signing_key.serialize_pem();

    fs::write(&cert_path, cert_pem)?;
    fs::write(&key_path, &key_pem)?;
    // Set restrictive permissions on key file (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!(
        "Generated self-signed certificate at {}",
        cert_dir.display()
    );

    Ok((cert_path, key_path))
}

/// Load a rustls `ServerConfig` from PEM-encoded cert and key files.
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<rustls::ServerConfig> {
    let cert_bytes = fs::read(cert_path)?;
    let key_bytes = fs::read(key_path)?;

    // Parse certificates from PEM
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_bytes.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("Failed to parse certificate PEM: {}", e))?;

    if certs.is_empty() {
        return Err(anyhow!("No certificates found in {}", cert_path.display()));
    }

    // Parse private key from PEM
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_bytes.as_slice())?
        .ok_or_else(|| anyhow!("No private key found in {}", key_path.display()))?;

    // Build rustls server config with ring crypto provider
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| anyhow!("Failed to build TLS config: {}", e))?;

    Ok(config)
}

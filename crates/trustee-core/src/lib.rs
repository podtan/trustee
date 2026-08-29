//! Trustee Core — shared types, session state, and workflow logic.
//!
//! This crate is the shared foundation used by both `trustee-tui` (terminal UI)
//! and the upcoming `trustee-api` (HTTP/WebSocket server). It contains:
//!
//! - **Types**: enums and structs that model the agent's state machine
//! - **Session**: the core `Session` struct (agent state without UI concerns)
//! - **Workflow**: command execution, handoff, and message handling
//! - **Config**: auto-handoff and color parsing from TOML

pub mod config;
pub mod session;
pub mod sessions;
pub mod types;

/// Compute the per-user directory name for `~/.trustee/users/{hash}/`.
///
/// Single definition of the user→hash mapping: SHA-256 of `key`, first 8
/// bytes interpreted big-endian, formatted as 16 lowercase hex chars.
///
/// Two input domains feed this one function:
/// - **Web/API path**: the authenticated `user_key` (JWT `sub` claim, or
///   `dev:email` in dev mode) — see `trustee-api`'s `ServerState`.
/// - **CLI path**: the OS username (`$USER` / `$USERNAME`, falling back to
///   `"unknown"`) — see the `trustee` binary's `compute_user_hash`.
///
/// The two domains intentionally produce different hashes for the same
/// directory tree; consolidating the *algorithm* here guarantees they cannot
/// drift apart.
pub fn user_hash(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let hash_bytes = hasher.finalize();
    format!(
        "{:016x}",
        u64::from_be_bytes(
            hash_bytes[..8]
                .try_into()
                .expect("SHA-256 digest is ≥ 8 bytes")
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known vector: SHA-256("test") =
    /// 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08,
    /// so the first 8 bytes big-endian format to the first 16 hex chars.
    #[test]
    fn user_hash_known_vector() {
        assert_eq!(user_hash("test"), "9f86d081884c7d65");
    }

    #[test]
    fn user_hash_is_deterministic() {
        assert_eq!(
            user_hash("farzan@example.com"),
            user_hash("farzan@example.com")
        );
    }

    #[test]
    fn user_hash_is_hex16_and_input_sensitive() {
        let a = user_hash("alice");
        let b = user_hash("bob");
        assert_eq!(a.len(), 16);
        assert_eq!(b.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(b.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn user_hash_matches_full_sha256_prefix() {
        // Cross-check against an independently computed digest
        // (sha256sum of "trustee"): first 16 hex chars of the full digest.
        use sha2::{Digest, Sha256};
        let full = Sha256::digest("trustee");
        let expected = full[..8]
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        assert_eq!(user_hash("trustee"), expected);
    }
}

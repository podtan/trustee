//! Stable identity → user_key pinning (nghr ec3e0622).
//!
//! ROOT DEFECT CLASS (owner ruling 2026-09-14): the per-user namespace key
//! used to be RECOMPUTED from token claims on every request (humans:
//! `preferred_username || sub`). Any claim mutation — a routine IdP group/
//! role/scope change, or fallback flicker when a claim appears/disappears
//! (the 16D migration itself) — silently rotated `users/{user_hash(key)}`
//! and stranded config + session history in an empty new home, masked by
//! the sibling global-token shadowing bug (92a36073).
//!
//! ROOT-CAUSE FIX: derive ONCE per principal, PIN it, never recompute.
//! - first sighting of a `sub` pins the 16D-derived key;
//! - every later request resolves through the pin — claims may gate
//!   PERMISSIONS (Cedar) but never the namespace;
//! - agents are unaffected (their key is already the immutable `sub`; the
//!   carve-out lives in the auth-side resolution, not here);
//! - THQ-materialized profiles were already claim-independent (keyed by
//!   profile id); this registry brings the human personal lane to the same
//!   guarantee.
//!
//! Storage: `~/.trustee/users/identity_map.json` — `{ "pins": { sub: key } }`.
//! BTreeMap for deterministic serialization. Values NEVER logged (a user_key
//! is not secret, but the file is identity data — keep it 0600).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone)]
pub struct IdentityRegistry {
    path: PathBuf,
    map: std::sync::Arc<Mutex<BTreeMap<String, String>>>,
}

impl IdentityRegistry {
    /// Registry at the default trustee home (`~/.trustee/users/`).
    /// Missing/blank home → an in-memory-only registry (same posture as
    /// every other `dirs::home_dir()` consumer in this crate).
    pub fn from_default_home() -> Self {
        let path = dirs::home_dir()
            .map(|h| h.join(".trustee").join("users").join("identity_map.json"))
            .unwrap_or_else(|| PathBuf::from("identity_map.json"));
        Self::from_path(path)
    }

    pub fn from_path(path: PathBuf) -> Self {
        let map = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("pins").and_then(|p| p.as_object()).map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect::<BTreeMap<String, String>>()
                })
            })
            .unwrap_or_default();
        Self {
            path,
            map: std::sync::Arc::new(Mutex::new(map)),
        }
    }

    /// Current pin for `sub`, if any.
    pub fn get(&self, sub: &str) -> Option<String> {
        self.map.lock().ok()?.get(sub).cloned()
    }

    /// Resolve the namespace key for `sub`: the pinned key when one exists,
    /// otherwise pin `derived` (the one-time legacy derivation) and return it.
    /// Save failures are loud but non-fatal — the in-memory pin still holds
    /// for this process; the next boot re-pins from the same first claims.
    pub fn pin(&self, sub: &str, derived: &str) -> String {
        if let Ok(map) = self.map.lock() {
            if let Some(pinned) = map.get(sub) {
                return pinned.clone();
            }
        }
        let mut map = self.map.lock().expect("identity registry poisoned");
        // Double-check under the write lock (two first-sights racing).
        if let Some(pinned) = map.get(sub) {
            return pinned.clone();
        }
        map.insert(sub.to_string(), derived.to_string());
        self.save(&map);
        tracing::info!(
            "identity registry: pinned user_key for sub {sub} (first sighting) — \
             claims can no longer rotate this namespace"
        );
        derived.to_string()
    }

    fn save(&self, map: &BTreeMap<String, String>) {
        let dir = self
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let _ = std::fs::create_dir_all(&dir);
        let doc = serde_json::json!({ "pins": map });
        match serde_json::to_vec_pretty(&doc) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&self.path, bytes) {
                    tracing::warn!(
                        "identity registry: failed to persist {}: {e} — pin is process-local only",
                        self.path.display()
                    );
                }
            }
            Err(e) => tracing::warn!("identity registry: serialize failed: {e}"),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &self.path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(tag: &str) -> IdentityRegistry {
        let dir = std::env::temp_dir().join(format!(
            "trustee-ident-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        IdentityRegistry::from_path(dir.join("identity_map.json"))
    }

    #[test]
    fn first_sight_pins_derived_key() {
        let r = registry("pin1");
        assert_eq!(r.pin("sub-1", "farzan"), "farzan");
        assert_eq!(r.pin("sub-1", "anything-else"), "farzan");
    }

    #[test]
    fn claim_mutation_cannot_rotate_the_namespace() {
        // THE ec3e0622 PIN: preferred_username / fallback flicker must not
        // move a pinned principal.
        let r = registry("mut");
        let first = r.pin("sub-1", "farzan");
        // Simulate a claims change: derived key flips to the sub fallback.
        assert_eq!(r.pin("sub-1", "sub-1"), first, "claim mutation rotated the namespace");
        // And a totally different derived value still loses to the pin.
        assert_eq!(r.pin("sub-1", "ghost"), first);
    }

    #[test]
    fn distinct_principals_pin_independently() {
        let r = registry("multi");
        assert_eq!(r.pin("sub-a", "farzan"), "farzan");
        assert_eq!(r.pin("sub-b", "leo"), "leo");
        assert_eq!(r.pin("sub-a", "leo"), "farzan");
    }

    #[test]
    fn pins_persist_across_reload() {
        let dir = std::env::temp_dir().join(format!(
            "trustee-ident-persist-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("identity_map.json");

        let r = IdentityRegistry::from_path(path.clone());
        r.pin("sub-1", "farzan");
        drop(r);

        let r2 = IdentityRegistry::from_path(path);
        assert_eq!(r2.pin("sub-1", "different-derived"), "farzan");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_an_empty_registry() {
        let r = registry("empty");
        assert_eq!(r.get("nobody"), None);
        assert_eq!(r.pin("nobody", "default"), "default");
    }
}

//! core::trust — pinned peer public keys (ADR-0003: "pin the peer key; a changed key
//! triggers re-pair"). A persisted allowlist of trusted peer Noise statics.
//!
//! MITM protection: a connection whose Noise static is not pinned is rejected (unless
//! the host is in pair mode, or the peer is on loopback — the local trust boundary). The
//! host owns a store of trusted client keys; an active MITM presents its own key, which
//! is not pinned, so the host rejects it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A persisted set of trusted peer public keys (lowercase hex, one per line).
pub struct TrustStore {
    path: PathBuf,
    keys: BTreeSet<String>,
}

impl TrustStore {
    /// Load the store from `path` (an absent file is an empty store).
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut keys = BTreeSet::new();
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read trust store {}", path.display()))?;
            for line in text.lines() {
                let k = line.trim();
                if !k.is_empty() {
                    keys.insert(k.to_string());
                }
            }
        }
        Ok(Self { path, keys })
    }

    pub fn is_trusted(&self, public: &[u8]) -> bool {
        self.keys.contains(&hex(public))
    }

    /// Pin a peer key and persist the store. Idempotent.
    pub fn pin(&mut self, public: &[u8]) -> Result<()> {
        if self.keys.insert(hex(public)) {
            if let Some(parent) = self.path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
            }
            let body = self.keys.iter().fold(String::new(), |mut acc, k| {
                acc.push_str(k);
                acc.push('\n');
                acc
            });
            std::fs::write(&self.path, body)
                .with_context(|| format!("write trust store {}", self.path.display()))?;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Lowercase hex encoding.
pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Short human fingerprint (first 8 bytes / 16 hex chars) of a public key.
pub fn fingerprint(public: &[u8]) -> String {
    hex(public).chars().take(16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_persists_and_reloads() {
        let path = std::env::temp_dir().join(format!("wisp_trust_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let key_a = vec![1u8; 32];
        let key_b = vec![2u8; 32];

        {
            let mut t = TrustStore::load(&path).unwrap();
            assert!(t.is_empty());
            assert!(!t.is_trusted(&key_a));
            t.pin(&key_a).unwrap();
            assert!(t.is_trusted(&key_a));
            t.pin(&key_a).unwrap(); // idempotent
            assert_eq!(t.len(), 1);
        }
        // A fresh load sees the pinned key but not an unrelated one.
        let t2 = TrustStore::load(&path).unwrap();
        assert!(t2.is_trusted(&key_a));
        assert!(!t2.is_trusted(&key_b));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fingerprint_is_16_hex_chars() {
        assert_eq!(fingerprint(&[0xab; 32]), "abababababababab");
    }
}

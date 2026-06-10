//! core::identity — the device static keypair + its at-rest persistence.
//!
//! Per ADR-0009 Option A (no FIPS for the MVP), the device key is X25519 (the Noise
//! static). A *persistent* device identity is the prerequisite for ADR-0003 key
//! **pinning** (remember a peer's static to detect a changed key / MITM on reconnect)
//! and for `Noise_IK` 0-RTT reconnect.
//!
//! The private key is stored wrapped at rest by an `AtRestProtector`. The cross-platform
//! default here is `Unprotected` (bytes written as-is — spike/dev only). host-windows
//! supplies a DPAPI-backed protector — the real Option-A wrapping — in a later increment.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::crypto::{generate_static_keypair, StaticKeypair};

const KEY_LEN: usize = 32; // X25519 scalar / point

/// At-rest protection for the stored private key. The production impl wraps via the OS
/// keystore (Windows DPAPI / Apple Keychain / Android Keystore) per ADR-0009 Option A.
pub trait AtRestProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>>;
    fn unprotect(&self, wrapped: &[u8]) -> Result<Vec<u8>>;
}

/// No at-rest wrapping — key bytes are stored verbatim. SPIKE / dev only (or where the
/// filesystem is already protected). Replace with an OS-keystore protector for the MVP.
pub struct Unprotected;

impl AtRestProtector for Unprotected {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        Ok(plaintext.to_vec())
    }
    fn unprotect(&self, wrapped: &[u8]) -> Result<Vec<u8>> {
        Ok(wrapped.to_vec())
    }
}

/// Load the device keypair from `path`, or generate + persist a new one if absent. The
/// stored blob is `protect(private || public)`.
pub fn load_or_create(
    path: impl AsRef<Path>,
    protector: &dyn AtRestProtector,
) -> Result<StaticKeypair> {
    let path = path.as_ref();
    if path.exists() {
        let wrapped =
            std::fs::read(path).with_context(|| format!("read device key {}", path.display()))?;
        let raw = protector
            .unprotect(&wrapped)
            .context("unprotect device key")?;
        anyhow::ensure!(
            raw.len() == KEY_LEN * 2,
            "device key file corrupt ({} bytes, expected {})",
            raw.len(),
            KEY_LEN * 2
        );
        Ok(StaticKeypair {
            private: raw[..KEY_LEN].to_vec(),
            public: raw[KEY_LEN..].to_vec(),
        })
    } else {
        let kp = generate_static_keypair()?;
        let mut raw = Vec::with_capacity(KEY_LEN * 2);
        raw.extend_from_slice(&kp.private);
        raw.extend_from_slice(&kp.public);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
        }
        let wrapped = protector.protect(&raw).context("protect device key")?;
        std::fs::write(path, &wrapped)
            .with_context(|| format!("write device key {}", path.display()))?;
        Ok(kp)
    }
}

/// Default per-user device-key path: `%APPDATA%\wisp\device.key` (Windows), or
/// `$XDG_CONFIG_HOME/wisp/device.key`, or `~/.config/wisp/device.key`.
pub fn default_key_path() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(PathBuf::from(appdata).join("wisp").join("device.key"));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("wisp").join("device.key"));
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("wisp")
                .join("device.key"),
        );
    }
    None
}

/// Per-role device-key path (so a host and a client on the same machine get distinct
/// identities): `<default dir>/<role>-device.key`.
pub fn role_key_path(role: &str) -> Option<PathBuf> {
    default_key_path().map(|p| p.with_file_name(format!("{role}-device.key")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_create_persists_and_reloads() {
        let path =
            std::env::temp_dir().join(format!("wisp_devkey_test_{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&path);

        // First call generates + persists.
        let k1 = load_or_create(&path, &Unprotected).unwrap();
        assert_eq!(k1.private.len(), KEY_LEN);
        assert_eq!(k1.public.len(), KEY_LEN);
        assert!(path.exists());

        // Second call loads the SAME key (stable device identity).
        let k2 = load_or_create(&path, &Unprotected).unwrap();
        assert_eq!(k1.private, k2.private);
        assert_eq!(k1.public, k2.public);

        // The persisted private key is usable for a Noise handshake.
        crate::crypto::Handshake::initiator(&k2.private).unwrap();

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_key_file_is_rejected() {
        let path = std::env::temp_dir().join(format!("wisp_devkey_bad_{}.bin", std::process::id()));
        std::fs::write(&path, b"too short").unwrap();
        let r = load_or_create(&path, &Unprotected);
        assert!(r.is_err());
        let _ = std::fs::remove_file(&path);
    }
}

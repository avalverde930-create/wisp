//! wisp-core::known_hosts — the CLIENT's cache of host device static keys, keyed by the
//! target address. It is the mirror of host-side `trust` pinning: caching a host static
//! after the first XX handshake is what enables the IK 0-RTT reconnect (ADR-0003), and a
//! *changed* host key on the same address is surfaced for re-pair, not silently trusted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::trust::hex;

/// Persisted `addr -> host static public (hex)` map; one entry per line: `<addr> <hexkey>`.
pub struct KnownHosts {
    path: PathBuf,
    map: BTreeMap<String, String>,
}

impl KnownHosts {
    /// Load from `path` (an absent file is an empty cache).
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut map = BTreeMap::new();
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("read known hosts {}", path.display()))?;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some((addr, key)) = line.split_once(char::is_whitespace) {
                    map.insert(addr.trim().to_string(), key.trim().to_string());
                }
            }
        }
        Ok(Self { path, map })
    }

    /// The cached host static for `addr`, decoded from hex (`None` if unknown or malformed).
    pub fn get(&self, addr: &str) -> Option<Vec<u8>> {
        self.map.get(addr).and_then(|h| unhex(h))
    }

    /// Remember (or update) the host static for `addr` and persist. Returns the *previous,
    /// different* key when the host key for this address CHANGED (so the caller can warn
    /// about a possible MITM / key rotation); `None` when newly learned or unchanged.
    pub fn remember(&mut self, addr: &str, public: &[u8]) -> Result<Option<Vec<u8>>> {
        let new_hex = hex(public);
        let prev_hex = self.map.get(addr).cloned();
        let changed_from = match &prev_hex {
            Some(p) if *p != new_hex => unhex(p),
            _ => None,
        };
        if prev_hex.as_deref() != Some(new_hex.as_str()) {
            self.map.insert(addr.to_string(), new_hex);
            self.persist()?;
        }
        Ok(changed_from)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn persist(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
        }
        let body = self.map.iter().fold(String::new(), |mut acc, (a, k)| {
            acc.push_str(a);
            acc.push(' ');
            acc.push_str(k);
            acc.push('\n');
            acc
        });
        std::fs::write(&self.path, body)
            .with_context(|| format!("write known hosts {}", self.path.display()))
    }
}

/// Decode hex (either case) to bytes; `None` on odd length or any non-hex digit.
fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let b = s.as_bytes();
    let nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        out.push((nibble(b[i])? << 4) | nibble(b[i + 1])?);
        i += 2;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_get_and_change_detection() {
        let path =
            std::env::temp_dir().join(format!("wisp_known_hosts_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let key_a = vec![0xAAu8; 32];
        let key_b = vec![0xBBu8; 32];
        let addr = "192.168.1.10:9000";

        {
            let mut kh = KnownHosts::load(&path).unwrap();
            assert!(kh.is_empty());
            assert!(kh.get(addr).is_none());

            // First learn: no previous key => no change reported.
            assert!(kh.remember(addr, &key_a).unwrap().is_none());
            assert_eq!(kh.get(addr).unwrap(), key_a);

            // Same key again: idempotent, still no change.
            assert!(kh.remember(addr, &key_a).unwrap().is_none());
            assert_eq!(kh.len(), 1);
        }

        // A fresh load sees the persisted key.
        let mut kh2 = KnownHosts::load(&path).unwrap();
        assert_eq!(kh2.get(addr).unwrap(), key_a);

        // A different key for the same addr is reported as a change (returns the old key).
        let changed = kh2.remember(addr, &key_b).unwrap();
        assert_eq!(changed.unwrap(), key_a);
        assert_eq!(kh2.get(addr).unwrap(), key_b);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn malformed_hex_is_ignored() {
        assert!(unhex("zz").is_none()); // non-hex
        assert!(unhex("abc").is_none()); // odd length
        assert_eq!(unhex("00ff").unwrap(), vec![0x00, 0xff]);
    }
}

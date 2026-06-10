//! core::crypto — the ONLY home for crypto primitives.
//! Native E2EE: Noise_XX_25519_ChaChaPoly_BLAKE2s first contact (full mutual auth + SAS),
//! cache the peer static, Noise_IK for 0-RTT reconnect, CLEAN re-pair on IK decrypt failure.
//! XXfallback / Noise Pipes is intentionally NOT used — `snow` lacks the fallback modifier
//! and hand-rolling a handshake fallback is forbidden. See docs/adr/0003-noise-pattern.md.
//! FIPS note: if FIPS is required (decided no for the MVP, ADR-0009), swap to AES-GCM/SHA-2.
//! Fuzzed from Phase 0 (cargo-fuzz handshake target).
//!
//! THIS INCREMENT (Phase 1, step 1): the XX handshake state machine + post-handshake
//! session AEAD + SAS derivation, unit-tested in isolation. NEXT: cache the peer static
//! and add the `Noise_IK` 0-RTT reconnect, then wire this in front of the quinn streams
//! (replacing the spike's cert-skip + cleartext token).

use anyhow::Result;
use snow::{Builder, HandshakeState, TransportState};

/// First-contact suite (ADR-0003): full mutual auth, both statics transmitted. Non-FIPS
/// for the MVP (ADR-0009 Option A).
pub const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// 0-RTT reconnect suite (ADR-0003): the initiator already knows (cached) the responder
/// static from a prior XX, so reconnect is one round trip and can carry a 0-RTT payload.
pub const NOISE_PARAMS_IK: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

fn noise_params() -> Result<snow::params::NoiseParams> {
    NOISE_PARAMS
        .parse()
        .map_err(|e| anyhow::anyhow!("parse noise params: {e:?}"))
}

fn noise_params_ik() -> Result<snow::params::NoiseParams> {
    NOISE_PARAMS_IK
        .parse()
        .map_err(|e| anyhow::anyhow!("parse noise IK params: {e:?}"))
}

/// A long-term device static keypair (X25519). Key *storage* (OS-keystore wrapping per
/// ADR-0009 Option A) is the `identity` module's job; this is just the key material.
pub struct StaticKeypair {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

/// Generate a fresh X25519 static keypair for the Noise suite.
pub fn generate_static_keypair() -> Result<StaticKeypair> {
    let kp = Builder::new(noise_params()?)
        .generate_keypair()
        .map_err(|e| anyhow::anyhow!("generate keypair: {e:?}"))?;
    Ok(StaticKeypair {
        private: kp.private,
        public: kp.public,
    })
}

/// One side of an in-progress Noise handshake (XX first-contact or IK reconnect). Drive
/// it with `write`/`read` in the pattern's message order (XX: 3 messages; IK: 2), then
/// `into_session`.
pub struct Handshake {
    state: HandshakeState,
}

impl Handshake {
    pub fn initiator(static_private: &[u8]) -> Result<Self> {
        let state = Builder::new(noise_params()?)
            .local_private_key(static_private)
            .build_initiator()
            .map_err(|e| anyhow::anyhow!("build initiator: {e:?}"))?;
        Ok(Self { state })
    }

    pub fn responder(static_private: &[u8]) -> Result<Self> {
        let state = Builder::new(noise_params()?)
            .local_private_key(static_private)
            .build_responder()
            .map_err(|e| anyhow::anyhow!("build responder: {e:?}"))?;
        Ok(Self { state })
    }

    /// IK initiator: the responder's static public key is already known (cached from a
    /// prior XX), enabling a 0-RTT reconnect (ADR-0003).
    pub fn initiator_ik(static_private: &[u8], remote_public: &[u8]) -> Result<Self> {
        let state = Builder::new(noise_params_ik()?)
            .local_private_key(static_private)
            .remote_public_key(remote_public)
            .build_initiator()
            .map_err(|e| anyhow::anyhow!("build IK initiator: {e:?}"))?;
        Ok(Self { state })
    }

    /// IK responder: learns the initiator static from the first message.
    pub fn responder_ik(static_private: &[u8]) -> Result<Self> {
        let state = Builder::new(noise_params_ik()?)
            .local_private_key(static_private)
            .build_responder()
            .map_err(|e| anyhow::anyhow!("build IK responder: {e:?}"))?;
        Ok(Self { state })
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Write the next handshake message (empty payload) into a fresh buffer.
    pub fn write(&mut self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; 1024];
        let n = self
            .state
            .write_message(&[], &mut buf)
            .map_err(|e| anyhow::anyhow!("handshake write: {e:?}"))?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Read a handshake message from the peer.
    pub fn read(&mut self, msg: &[u8]) -> Result<()> {
        let mut buf = vec![0u8; 1024];
        self.state
            .read_message(msg, &mut buf)
            .map_err(|e| anyhow::anyhow!("handshake read: {e:?}"))?;
        Ok(())
    }

    /// The peer's static public key — bound by the XX handshake (mutual auth).
    pub fn remote_static(&self) -> Option<Vec<u8>> {
        self.state.get_remote_static().map(|s| s.to_vec())
    }

    /// The channel-binding hash. Identical on both peers; derive the SAS from it.
    pub fn handshake_hash(&self) -> Vec<u8> {
        self.state.get_handshake_hash().to_vec()
    }

    /// Transition to the post-handshake AEAD session (only after the handshake is done).
    pub fn into_session(self) -> Result<Session> {
        let state = self
            .state
            .into_transport_mode()
            .map_err(|e| anyhow::anyhow!("into transport mode: {e:?}"))?;
        Ok(Session { state })
    }
}

/// A post-handshake AEAD session. Directional nonces are tracked internally by `snow`.
pub struct Session {
    state: TransportState,
}

impl Session {
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; plaintext.len() + 16];
        let n = self
            .state
            .write_message(plaintext, &mut buf)
            .map_err(|e| anyhow::anyhow!("session encrypt: {e:?}"))?;
        buf.truncate(n);
        Ok(buf)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; ciphertext.len()];
        let n = self
            .state
            .read_message(ciphertext, &mut buf)
            .map_err(|e| anyhow::anyhow!("session decrypt: {e:?}"))?;
        buf.truncate(n);
        Ok(buf)
    }
}

/// Derive a 6-digit Short Authentication String from the handshake hash (ADR-0003).
/// Both peers compute the same value; the owner compares it out-of-band to defeat a
/// pairing-time MITM. (No blind trust-on-first-use.)
pub fn sas_code(handshake_hash: &[u8]) -> String {
    let mut n: u32 = 0;
    for b in handshake_hash.iter().take(4) {
        n = (n << 8) | *b as u32;
    }
    format!("{:06}", n % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run the full XX handshake between two fresh peers, confirm mutual auth + a matching
    /// SAS, then round-trip an AEAD message in both directions.
    #[test]
    fn xx_handshake_mutual_auth_and_roundtrip() {
        let ini_kp = generate_static_keypair().unwrap();
        let res_kp = generate_static_keypair().unwrap();
        let mut ini = Handshake::initiator(&ini_kp.private).unwrap();
        let mut res = Handshake::responder(&res_kp.private).unwrap();

        // XX: -> e ; <- e ee s es ; -> s se
        let m1 = ini.write().unwrap();
        res.read(&m1).unwrap();
        let m2 = res.write().unwrap();
        ini.read(&m2).unwrap();
        let m3 = ini.write().unwrap();
        res.read(&m3).unwrap();

        assert!(ini.is_finished() && res.is_finished());

        // XX binds both identities: each side learned the other's real static key.
        assert_eq!(ini.remote_static().unwrap(), res_kp.public);
        assert_eq!(res.remote_static().unwrap(), ini_kp.public);

        // Same channel-binding hash => same SAS the owner compares out-of-band.
        let sas_i = sas_code(&ini.handshake_hash());
        let sas_r = sas_code(&res.handshake_hash());
        assert_eq!(sas_i, sas_r);
        assert_eq!(sas_i.len(), 6);

        // Transport-mode AEAD round trip, both directions.
        let mut si = ini.into_session().unwrap();
        let mut sr = res.into_session().unwrap();
        let ct = si.encrypt(b"hello wisp").unwrap();
        assert_ne!(ct.as_slice(), b"hello wisp");
        assert_eq!(sr.decrypt(&ct).unwrap(), b"hello wisp");
        let ct2 = sr.encrypt(b"ack").unwrap();
        assert_eq!(si.decrypt(&ct2).unwrap(), b"ack");
    }

    /// IK 0-RTT reconnect: the initiator already knows the responder static (cached from a
    /// prior XX); the 2-message handshake completes and a message round-trips.
    #[test]
    fn ik_reconnect_handshake_and_roundtrip() {
        let res_kp = generate_static_keypair().unwrap();
        let ini_kp = generate_static_keypair().unwrap();
        let mut ini = Handshake::initiator_ik(&ini_kp.private, &res_kp.public).unwrap();
        let mut res = Handshake::responder_ik(&res_kp.private).unwrap();

        // IK: -> e es s ss ; <- e ee se
        let m1 = ini.write().unwrap();
        res.read(&m1).unwrap();
        let m2 = res.write().unwrap();
        ini.read(&m2).unwrap();

        assert!(ini.is_finished() && res.is_finished());
        // The responder learns the initiator static; the initiator already knew the responder's.
        assert_eq!(res.remote_static().unwrap(), ini_kp.public);

        let mut si = ini.into_session().unwrap();
        let mut sr = res.into_session().unwrap();
        let ct = si.encrypt(b"reconnect").unwrap();
        assert_eq!(sr.decrypt(&ct).unwrap(), b"reconnect");
    }

    /// A tampered ciphertext must fail the AEAD tag check (no silent corruption).
    #[test]
    fn tampered_ciphertext_is_rejected() {
        let ini_kp = generate_static_keypair().unwrap();
        let res_kp = generate_static_keypair().unwrap();
        let mut ini = Handshake::initiator(&ini_kp.private).unwrap();
        let mut res = Handshake::responder(&res_kp.private).unwrap();
        let m1 = ini.write().unwrap();
        res.read(&m1).unwrap();
        let m2 = res.write().unwrap();
        ini.read(&m2).unwrap();
        let m3 = ini.write().unwrap();
        res.read(&m3).unwrap();

        let mut si = ini.into_session().unwrap();
        let mut sr = res.into_session().unwrap();
        let mut ct = si.encrypt(b"secret").unwrap();
        ct[0] ^= 0xFF;
        assert!(sr.decrypt(&ct).is_err());
    }
}

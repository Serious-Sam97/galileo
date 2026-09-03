//! Provider API keys are encrypted at rest with AES-256-GCM under the server secret.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};

const NONCE_LEN: usize = 12;

pub fn encrypt(secret: &[u8; 32], plaintext: &str) -> Vec<u8> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(secret));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher.encrypt(&nonce, plaintext.as_bytes()).expect("aes-gcm encrypt");
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn decrypt(secret: &[u8; 32], blob: &[u8]) -> Option<String> {
    if blob.len() < NONCE_LEN + 16 {
        return None;
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(secret));
    let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
    let pt = cipher.decrypt(nonce, &blob[NONCE_LEN..]).ok()?;
    String::from_utf8(pt).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let k = [7u8; 32];
        let blob = encrypt(&k, "sk-ant-secret");
        assert_eq!(decrypt(&k, &blob).as_deref(), Some("sk-ant-secret"));
        let mut bad = blob.clone();
        bad[NONCE_LEN + 2] ^= 1;
        assert!(decrypt(&k, &bad).is_none());
        assert!(decrypt(&[8u8; 32], &blob).is_none());
    }
}

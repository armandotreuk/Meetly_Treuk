use anyhow::{anyhow, Result};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose, Engine};

pub const ENCRYPTION_PREFIX: &str = "enc:";

pub fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> Result<String> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce_bytes = generate_nonce();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| anyhow!("AES-GCM encryption failed: {}", e))?;
    let mut combined = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(format!(
        "{}{}",
        ENCRYPTION_PREFIX,
        general_purpose::STANDARD.encode(&combined)
    ))
}

pub fn decrypt(ciphertext: &str, key: &[u8; 32]) -> Result<String> {
    let encoded = ciphertext
        .strip_prefix(ENCRYPTION_PREFIX)
        .ok_or_else(|| anyhow!("Ciphertext missing encryption prefix"))?;
    let combined = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| anyhow!("Base64 decode failed: {}", e))?;
    if combined.len() < 12 {
        return Err(anyhow!("Ciphertext too short"));
    }
    let (nonce_bytes, ciphertext_bytes) = combined.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext_bytes)
        .map_err(|e| anyhow!("AES-GCM decryption failed: {}", e))?;
    Ok(String::from_utf8(plaintext)?)
}

pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENCRYPTION_PREFIX)
}

fn generate_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

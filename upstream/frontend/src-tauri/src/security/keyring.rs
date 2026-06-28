use anyhow::{anyhow, Result};
use rand::RngCore;

const KEY_LENGTH: usize = 32;

pub fn get_or_create_master_key(service: &str, account: &str) -> Result<[u8; KEY_LENGTH]> {
    let entry = keyring::Entry::new(service, account)?;
    match entry.get_password() {
        Ok(password) => {
            let key = base64_decode(&password)?;
            if key.len() == KEY_LENGTH {
                let mut arr = [0u8; KEY_LENGTH];
                arr.copy_from_slice(&key);
                Ok(arr)
            } else {
                Err(anyhow!(
                    "Master key in keyring has wrong length: {} (expected {})",
                    key.len(),
                    KEY_LENGTH
                ))
            }
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = [0u8; KEY_LENGTH];
            rand::thread_rng().fill_bytes(&mut key);
            let encoded = base64_encode(&key);
            entry.set_password(&encoded)?;
            log::info!("Created new master encryption key in OS keyring");
            Ok(key)
        }
        Err(e) => Err(anyhow!("Failed to read master key from keyring: {}", e)),
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose, Engine};
    general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose, Engine};
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| anyhow!("Base64 decode failed: {}", e))
}

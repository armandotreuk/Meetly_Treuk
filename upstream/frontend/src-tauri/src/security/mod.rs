pub mod aes;
pub mod keyring;

use anyhow::Result;

pub use aes::{decrypt, encrypt};
pub use keyring::get_master_key;

pub const KEYRING_SERVICE: &str = "com.meetily.ai";
pub const KEYRING_ACCOUNT: &str = "master-key";

pub fn init() -> Result<()> {
    let _key = keyring::get_or_create_master_key(KEYRING_SERVICE, KEYRING_ACCOUNT)?;
    Ok(())
}

pub fn encrypt_api_key(plaintext: &str) -> Result<String> {
    let key = keyring::get_or_create_master_key(KEYRING_SERVICE, KEYRING_ACCOUNT)?;
    let ciphertext = aes::encrypt(plaintext.as_bytes(), &key)?;
    Ok(ciphertext)
}

pub fn decrypt_api_key(ciphertext: &str) -> Result<String> {
    let key = keyring::get_or_create_master_key(KEYRING_SERVICE, KEYRING_ACCOUNT)?;
    let plaintext = aes::decrypt(ciphertext, &key)?;
    Ok(plaintext)
}

pub fn is_encrypted(value: &str) -> bool {
    aes::is_encrypted(value)
}

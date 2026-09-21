use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng},
};
use std::{io::Write, path::Path};

pub struct Vault(ChaCha20Poly1305);
impl Vault {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("secret.key");
        if !path.exists() {
            let key = ChaCha20Poly1305::generate_key(&mut OsRng);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path)?;
            file.write_all(&key)?;
            file.sync_all()?;
        }
        let key = std::fs::read(path)?;
        if key.len() != 32 {
            bail!("Invalid publishing encryption key");
        }
        Ok(Self(
            ChaCha20Poly1305::new_from_slice(&key).expect("key length checked"),
        ))
    }
    pub fn seal(&self, value: &str) -> Result<String> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .0
            .encrypt(&nonce, value.as_bytes())
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;
        Ok(STANDARD.encode([nonce.as_slice(), ciphertext.as_slice()].concat()))
    }
    pub fn open_secret(&self, value: &str) -> Result<String> {
        let bytes = STANDARD.decode(value).context("Invalid encrypted value")?;
        if bytes.len() < 28 {
            bail!("Invalid encrypted value");
        }
        let clear = self
            .0
            .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
            .map_err(|_| {
                anyhow::anyhow!(
                    "Cannot decrypt publishing credentials; restore the original secret.key"
                )
            })?;
        Ok(String::from_utf8(clear)?)
    }
}

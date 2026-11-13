//! Stateless "stay signed in": the credentials are sealed with AES-256-GCM
//! into an HttpOnly cookie. The server stores only the key, never the login.

use std::{path::PathBuf, sync::OnceLock};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngExt;
use serde::{Deserialize, Serialize};

const AAD: &[u8] = b"campusunbloat-remember-v1";
const NONCE_LEN: usize = 12;

#[derive(Serialize, Deserialize)]
struct Sealed {
    u: String,
    p: String,
    exp: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn decode_hex_key(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut key = [0_u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(key)
}

/// Keeps the generated key file readable only by its owner, since anyone who
/// reads it can decrypt every "remember me" cookie (i.e. every password).
#[cfg(unix)]
fn restrict_key_file_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        log::warn!("Could not restrict permissions on {path:?}: {error}");
    }
}

#[cfg(not(unix))]
fn restrict_key_file_permissions(_path: &std::path::Path) {}

fn key_file() -> PathBuf {
    std::env::var_os("CAMPUSUNBLOAT_DATA_DIR")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("remember.key")
}

/// `CAMPUSUNBLOAT_SECRET` (64 hex chars) for hosted setups, otherwise a
/// generated key file for local use.
fn key() -> Result<&'static [u8; 32]> {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    if let Some(key) = KEY.get() {
        return Ok(key);
    }
    let key = if let Ok(secret) = std::env::var("CAMPUSUNBLOAT_SECRET") {
        decode_hex_key(&secret).context("CAMPUSUNBLOAT_SECRET must be 64 hex characters")?
    } else {
        let path = key_file();
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| decode_hex_key(&text))
        {
            Some(key) => key,
            None => {
                let mut key = [0_u8; 32];
                rand::rng().fill(&mut key);
                let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&path, hex).context("could not persist remember key")?;
                restrict_key_file_permissions(&path);
                key
            }
        }
    };
    Ok(KEY.get_or_init(|| key))
}

pub fn seal(username: &str, password: &str, ttl_secs: u64) -> Result<String> {
    let payload = serde_json::to_vec(&Sealed {
        u: username.to_string(),
        p: password.to_string(),
        exp: now_secs() + ttl_secs,
    })?;
    let cipher = Aes256Gcm::new(key()?.into());
    let mut nonce = [0_u8; NONCE_LEN];
    rand::rng().fill(&mut nonce);
    let Ok(encrypted) = cipher.encrypt(
        (&nonce).into(),
        Payload {
            msg: &payload,
            aad: AAD,
        },
    ) else {
        bail!("encryption failed");
    };
    let mut out = nonce.to_vec();
    out.extend(encrypted);
    Ok(URL_SAFE_NO_PAD.encode(out))
}

pub fn open(token: &str) -> Option<(String, String)> {
    let bytes = URL_SAFE_NO_PAD.decode(token).ok()?;
    if bytes.len() <= NONCE_LEN {
        return None;
    }
    let (nonce, encrypted) = bytes.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(key().ok()?.into());
    let plain = cipher
        .decrypt(
            nonce.into(),
            Payload {
                msg: encrypted,
                aad: AAD,
            },
        )
        .ok()?;
    let sealed: Sealed = serde_json::from_slice(&plain).ok()?;
    (sealed.exp > now_secs()).then_some((sealed.u, sealed.p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hides_password_and_rejects_tampering() {
        // SAFETY: only this test sets the variable.
        unsafe { std::env::set_var("CAMPUSUNBLOAT_SECRET", "ab".repeat(32)) };
        let token = seal("user", "geheim-pw", 60).unwrap();
        assert!(!token.contains("geheim"));
        assert_eq!(open(&token), Some(("user".into(), "geheim-pw".into())));
        let mut tampered = token.into_bytes();
        let last = tampered.len() - 2;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        assert_eq!(open(&String::from_utf8(tampered).unwrap()), None);
        assert_eq!(open(&seal("u", "p", 0).unwrap()), None);
    }
}

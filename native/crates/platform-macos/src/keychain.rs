//! macOS Keychain-backed secret storage.
//!
//! Design: a single AES-256-GCM **master key** lives in the login keychain as one
//! generic-password item; individual secrets are encrypted with it and stored as
//! self-contained `nhk1:` ciphertext strings next to the rows that own them (env values,
//! MCP token). That keeps ciphertext portable inside our own database while the key never
//! leaves the keychain.
//!
//! Why not store every secret in the keychain directly: there would be hundreds of items,
//! each with its own prompt/latency, and a partial keychain (one item deleted) would leave
//! the database half-readable. One master key gives an all-or-nothing failure mode that the
//! UI can explain.
//!
//! Legacy `enc1:` blobs from the Electron build are **not** decryptable here — they are
//! Chromium `safeStorage` ciphertext. Phase 13 migrates them through an explicit export.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use neohub_storage::secrets::{NATIVE_PREFIX, SecretError, SecretResult, SecretStore};
use rand::RngCore;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Keychain service name for this build. Preview and stable builds must not share an item.
pub const KEYCHAIN_SERVICE: &str = "app.neohub.desktop.rustpreview.secrets";
/// Keychain account holding the master key.
pub const MASTER_KEY_ACCOUNT: &str = "master-key";

const NONCE_LEN: usize = 12;

/// Secret store backed by the login keychain.
pub struct KeychainSecretStore {
    service: String,
    account: String,
}

impl KeychainSecretStore {
    /// Production store (preview bundle identity).
    pub fn new() -> Self {
        KeychainSecretStore {
            service: KEYCHAIN_SERVICE.to_string(),
            account: MASTER_KEY_ACCOUNT.to_string(),
        }
    }

    /// Custom service/account, used by the opt-in keychain integration test so it never
    /// touches the real master key.
    pub fn with_names(service: impl Into<String>, account: impl Into<String>) -> Self {
        KeychainSecretStore {
            service: service.into(),
            account: account.into(),
        }
    }

    /// Load the master key.
    ///
    /// `create` is only true while encrypting: decrypting must never mint a fresh key,
    /// otherwise a deleted keychain item would silently produce garbage instead of a
    /// clear "the secrets are gone" error (and would leave a stray item behind).
    fn master_key(&self, create: bool) -> SecretResult<[u8; 32]> {
        match get_generic_password(&self.service, &self.account) {
            Ok(bytes) => {
                let key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                    SecretError::Corrupt(format!(
                        "master key must be 32 bytes, found {}",
                        bytes.len()
                    ))
                })?;
                Ok(key)
            }
            Err(err) if is_not_found(&err) => {
                if !create {
                    return Err(SecretError::Corrupt(
                        "master key is missing from the keychain (secrets were reset)".into(),
                    ));
                }
                let mut key = [0u8; 32];
                rand::rng().fill_bytes(&mut key);
                set_generic_password(&self.service, &self.account, &key)
                    .map_err(map_keychain_error)?;
                Ok(key)
            }
            Err(err) => Err(map_keychain_error(err)),
        }
    }
}

impl Default for KeychainSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeychainSecretStore {
    fn encrypt(&self, plaintext: &str) -> SecretResult<String> {
        let key = self.master_key(true)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| SecretError::Corrupt(e.to_string()))?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| SecretError::Corrupt("encryption failed".into()))?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ciphertext);
        Ok(format!("{NATIVE_PREFIX}{}", BASE64.encode(blob)))
    }

    fn decrypt(&self, ciphertext: &str) -> SecretResult<String> {
        let Some(body) = ciphertext.strip_prefix(NATIVE_PREFIX) else {
            return Err(SecretError::UnsupportedFormat(ciphertext.to_string()));
        };
        let blob = BASE64
            .decode(body)
            .map_err(|e| SecretError::Corrupt(e.to_string()))?;
        if blob.len() <= NONCE_LEN {
            return Err(SecretError::Corrupt("ciphertext too short".into()));
        }
        let (nonce_bytes, encrypted) = blob.split_at(NONCE_LEN);
        let key = self.master_key(false)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| SecretError::Corrupt(e.to_string()))?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), encrypted)
            .map_err(|_| SecretError::Corrupt("wrong key or tampered ciphertext".into()))?;
        String::from_utf8(plaintext).map_err(|e| SecretError::Corrupt(e.to_string()))
    }
}

/// Remove the master key (used by "reset secrets" and by the integration test cleanup).
pub fn delete_master_key(store: &KeychainSecretStore) -> SecretResult<()> {
    match delete_generic_password(&store.service, &store.account) {
        Ok(()) => Ok(()),
        Err(err) if is_not_found(&err) => Ok(()),
        Err(err) => Err(map_keychain_error(err)),
    }
}

fn is_not_found(err: &security_framework::base::Error) -> bool {
    err.code() == -25300 // errSecItemNotFound
}

fn map_keychain_error(err: security_framework::base::Error) -> SecretError {
    match err.code() {
        // errSecInteractionNotAllowed / errSecAuthFailed / errSecUserCanceled
        -25308 | -25293 | -128 => SecretError::Locked,
        -25300 => SecretError::Corrupt("keychain item not found".into()),
        code => SecretError::Unavailable.with_detail(&format!("keychain error {code} ({err})")),
    }
}

trait WithDetail {
    fn with_detail(self, detail: &str) -> Self;
}

impl WithDetail for SecretError {
    fn with_detail(self, detail: &str) -> Self {
        match self {
            SecretError::Unavailable => SecretError::Corrupt(detail.to_string()),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_foreign_prefix_is_rejected_without_touching_the_keychain() {
        let store = KeychainSecretStore::with_names("test.service", "test-account");
        assert!(matches!(
            store.decrypt("enc1:legacy"),
            Err(SecretError::UnsupportedFormat(_))
        ));
        assert!(matches!(
            store.decrypt("plain-text"),
            Err(SecretError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn a_truncated_blob_is_reported_as_corrupt_before_any_keychain_read() {
        let store = KeychainSecretStore::with_names("test.service", "test-account");
        let short = format!("{NATIVE_PREFIX}{}", BASE64.encode([0u8; 4]));
        assert!(matches!(
            store.decrypt(&short),
            Err(SecretError::Corrupt(_))
        ));
    }

    /// Opt-in: real keychain write/read/delete. Not part of the default suite because it
    /// talks to the login keychain (and may prompt).
    ///
    /// Run with: `cargo test -p neohub-platform-macos -- --ignored keychain_round_trip`
    #[test]
    #[ignore = "touches the login keychain; run explicitly"]
    fn keychain_round_trip() {
        let store = KeychainSecretStore::with_names(
            "app.neohub.desktop.rustpreview.test",
            "round-trip-account",
        );
        delete_master_key(&store).expect("clean slate");
        let ciphertext = store.encrypt("synthetic-secret-value").expect("encrypt");
        assert!(ciphertext.starts_with(NATIVE_PREFIX));
        assert!(!ciphertext.contains("synthetic-secret-value"));
        assert_eq!(
            store.decrypt(&ciphertext).expect("decrypt"),
            "synthetic-secret-value"
        );
        delete_master_key(&store).expect("cleanup");
        match store.decrypt(&ciphertext) {
            Err(SecretError::Corrupt(message)) => assert!(
                message.contains("master key") || message.contains("wrong key"),
                "unexpected message: {message}"
            ),
            other => panic!("expected an unreadable-secret error, got {other:?}"),
        }
        // Decrypting must not have recreated anything in the keychain.
        assert!(
            get_generic_password(&store.service, &store.account).is_err(),
            "a failed decrypt must not create a new master key"
        );
    }
}

//! Secret storage interface.
//!
//! Two ciphertext formats must be kept apart on purpose:
//!
//! * `enc1:` — what the Electron build wrote through Chromium's `safeStorage`
//!   (Keychain-backed, opaque to us). It is **not** directly decryptable by a Keychain API,
//!   so phase 13 migrates it through an explicit legacy export step rather than pretending
//!   the formats are interchangeable.
//! * [`NATIVE_PREFIX`] (`nhk1:`) — new ciphertext produced by this build. A distinct prefix
//!   means a `nhk1:` value can never be mistaken for a legacy one, and an unreadable value
//!   surfaces as [`SecretError`] instead of being handed to a child process as its own
//!   password.
//!
//! The trait lives here (storage) and is implemented by `neohub-platform-macos`; that keeps
//! the dependency direction intact — storage never links against platform code.

/// Prefix for ciphertext produced by the native build.
pub const NATIVE_PREFIX: &str = "nhk1:";

/// Prefix the Electron build used for `safeStorage` ciphertext.
pub const LEGACY_PREFIX: &str = "enc1:";

/// What went wrong when touching a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// The backend exists but refused access (locked keychain, denied prompt).
    #[error("the secret store is locked or access was denied")]
    Locked,

    /// No usable backend on this machine (missing Keychain service, headless test, …).
    #[error("no secret store is available")]
    Unavailable,

    /// The stored value cannot be decoded — wrong key, truncated blob, wrong format.
    #[error("stored secret is not readable: {0}")]
    Corrupt(String),

    /// The value is in a format this build cannot handle (e.g. legacy `enc1:` blobs).
    #[error("unsupported secret format: {0}")]
    UnsupportedFormat(String),
}

pub type SecretResult<T> = std::result::Result<T, SecretError>;

/// Encrypt/decrypt short strings (environment values, MCP tokens).
pub trait SecretStore: Send + Sync {
    /// Encrypt a plaintext value; the returned string includes [`NATIVE_PREFIX`].
    fn encrypt(&self, plaintext: &str) -> SecretResult<String>;

    /// Decrypt a value previously produced by [`SecretStore::encrypt`].
    fn decrypt(&self, ciphertext: &str) -> SecretResult<String>;
}

/// True when a stored value is Electron `safeStorage` ciphertext.
pub fn is_legacy_ciphertext(value: &str) -> bool {
    value.starts_with(LEGACY_PREFIX)
}

/// True when a stored value is native ciphertext.
pub fn is_native_ciphertext(value: &str) -> bool {
    value.starts_with(NATIVE_PREFIX)
}

/// Decide what to do with a stored value before handing it to a child process.
///
/// Returns the plaintext for a value that is either plain or decryptable, and
/// [`SecretError`] otherwise — never the ciphertext itself.
pub fn read_secret_value(store: &dyn SecretStore, stored: &str) -> SecretResult<String> {
    if is_native_ciphertext(stored) {
        return store.decrypt(stored);
    }
    if is_legacy_ciphertext(stored) {
        // Explicitly refused: passing `enc1:…` on as a password is exactly the silent
        // breakage phase 13 exists to prevent.
        return Err(SecretError::UnsupportedFormat(format!(
            "{LEGACY_PREFIX} ciphertext requires the legacy migration export"
        )));
    }
    Ok(stored.to_string())
}

/// Test/dev backend. Deterministic, reversible, and able to simulate the failure modes the
/// real Keychain implementation must also report.
pub struct FakeSecretStore {
    locked: bool,
    corrupt: bool,
}

impl FakeSecretStore {
    /// Working backend.
    pub fn new() -> Self {
        FakeSecretStore {
            locked: false,
            corrupt: false,
        }
    }

    /// Every operation reports [`SecretError::Locked`].
    pub fn locked() -> Self {
        FakeSecretStore {
            locked: true,
            corrupt: false,
        }
    }

    /// Encrypt works; decrypt reports [`SecretError::Corrupt`] (wrong key / truncated blob).
    pub fn corrupt() -> Self {
        FakeSecretStore {
            locked: false,
            corrupt: true,
        }
    }
}

impl Default for FakeSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for FakeSecretStore {
    fn encrypt(&self, plaintext: &str) -> SecretResult<String> {
        if self.locked {
            return Err(SecretError::Locked);
        }
        // Reversible, obviously-not-real encoding: enough to prove the round trip and that
        // ciphertext never equals plaintext.
        let encoded: String = plaintext.bytes().map(|b| format!("{b:02x}")).collect();
        Ok(format!("{NATIVE_PREFIX}fake:{encoded}"))
    }

    fn decrypt(&self, ciphertext: &str) -> SecretResult<String> {
        if self.locked {
            return Err(SecretError::Locked);
        }
        if self.corrupt {
            return Err(SecretError::Corrupt("wrong key".into()));
        }
        let Some(body) = ciphertext.strip_prefix(NATIVE_PREFIX) else {
            return Err(SecretError::UnsupportedFormat(ciphertext.to_string()));
        };
        let Some(hex) = body.strip_prefix("fake:") else {
            return Err(SecretError::Corrupt("unknown fake payload".into()));
        };
        if hex.len() % 2 != 0 {
            return Err(SecretError::Corrupt("odd-length payload".into()));
        }
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|e| SecretError::Corrupt(e.to_string()))?;
        String::from_utf8(bytes).map_err(|e| SecretError::Corrupt(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_never_returns_the_plaintext_as_ciphertext() {
        let store = FakeSecretStore::new();
        let cipher = store.encrypt("hunter2").unwrap();
        assert!(cipher.starts_with(NATIVE_PREFIX));
        assert_ne!(cipher, "hunter2");
        assert_eq!(store.decrypt(&cipher).unwrap(), "hunter2");
    }

    #[test]
    fn a_locked_store_reports_locked_for_both_directions() {
        let store = FakeSecretStore::locked();
        assert_eq!(store.encrypt("x"), Err(SecretError::Locked));
        assert_eq!(store.decrypt("nhk1:fake:78"), Err(SecretError::Locked));
    }

    #[test]
    fn a_corrupt_payload_reports_corrupt_instead_of_garbage() {
        let store = FakeSecretStore::corrupt();
        assert_eq!(
            store.decrypt("nhk1:fake:78"),
            Err(SecretError::Corrupt("wrong key".into()))
        );
    }

    #[test]
    fn a_truncated_payload_is_reported_as_corrupt() {
        let store = FakeSecretStore::new();
        assert!(matches!(
            store.decrypt("nhk1:fake:7"),
            Err(SecretError::Corrupt(_))
        ));
    }

    #[test]
    fn legacy_ciphertext_is_refused_not_passed_through_as_a_password() {
        let store = FakeSecretStore::new();
        let err = read_secret_value(&store, "enc1:v10:abcdef").unwrap_err();
        assert!(
            matches!(err, SecretError::UnsupportedFormat(_)),
            "enc1: blobs must never be handed to a child process"
        );
    }

    #[test]
    fn plain_values_and_native_ciphertext_are_read_normally() {
        let store = FakeSecretStore::new();
        assert_eq!(
            read_secret_value(&store, "plain-value").unwrap(),
            "plain-value"
        );
        let cipher = store.encrypt("secret").unwrap();
        assert_eq!(read_secret_value(&store, &cipher).unwrap(), "secret");
    }

    #[test]
    fn prefixes_are_distinguishable() {
        assert!(is_legacy_ciphertext("enc1:x"));
        assert!(!is_legacy_ciphertext("nhk1:x"));
        assert!(is_native_ciphertext("nhk1:x"));
        assert!(!is_native_ciphertext("enc1:x"));
    }
}

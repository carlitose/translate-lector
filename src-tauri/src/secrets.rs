//! Secure storage for the OpenRouter API key (SPECIFICATION.md NFR07, §3.5, UC05).
//!
//! Backed by the OS-native credential store through the `keyring` crate. On
//! Windows this is the Credential Manager, which writes silently for the
//! current user (no system prompt, no admin rights). The single secret is keyed
//! by a fixed service/account pair so the whole app shares one entry.

use keyring::{Entry, Error};

/// Service name registered in the OS credential store.
const SERVICE: &str = "translate-lector";
/// Fixed account under which the single OpenRouter API key is stored.
const ACCOUNT: &str = "openrouter-api-key";

/// Build an [`Entry`] for the given service/account against the default store.
fn entry(service: &str, account: &str) -> Result<Entry, Error> {
    Entry::new(service, account)
}

/// Store (or overwrite) the API key in the OS credential store.
pub fn set_api_key(key: &str) -> Result<(), Error> {
    set_api_key_for(SERVICE, ACCOUNT, key)
}

/// Read the API key. Returns `Ok(None)` when no key has been stored yet —
/// a missing credential is a clean absence, never an error.
pub fn get_api_key() -> Result<Option<String>, Error> {
    get_api_key_for(SERVICE, ACCOUNT)
}

/// Delete the stored API key. A missing credential is treated as success
/// (idempotent delete).
pub fn delete_api_key() -> Result<(), Error> {
    delete_api_key_for(SERVICE, ACCOUNT)
}

/// Whether an API key is present, without exposing the secret itself. Lets the
/// UI reflect key presence while the plaintext key stays inside the core.
pub fn has_api_key() -> Result<bool, Error> {
    has_api_key_for(SERVICE, ACCOUNT)
}

// --- Parametrised internals (let tests target a throwaway account) ---------

fn set_api_key_for(service: &str, account: &str, key: &str) -> Result<(), Error> {
    entry(service, account)?.set_password(key)
}

fn get_api_key_for(service: &str, account: &str) -> Result<Option<String>, Error> {
    match entry(service, account)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(e),
    }
}

fn delete_api_key_for(service: &str, account: &str) -> Result<(), Error> {
    match entry(service, account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

fn has_api_key_for(service: &str, account: &str) -> Result<bool, Error> {
    Ok(get_api_key_for(service, account)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises tests that touch the real OS credential store. The `keyring`
    /// crate initialises its default store lazily and is not safe against two
    /// threads racing that first-time init (yields spurious `NoDefaultStore`),
    /// so every real-store test acquires this lock first.
    static STORE_LOCK: Mutex<()> = Mutex::new(());

    /// Full round-trip against the REAL OS credential store (Windows
    /// Credential Manager on Win11). Uses a throwaway account so it never
    /// clobbers the app's real key, and cleans up on the way out.
    #[test]
    fn set_get_delete_roundtrip_hits_real_store() {
        let _guard = STORE_LOCK.lock().unwrap();
        const TEST_SERVICE: &str = "translate-lector-test";
        let account = format!("it-{}", std::process::id());
        let secret = "sk-or-test-0123456789";

        // Clean slate.
        delete_api_key_for(TEST_SERVICE, &account).unwrap();

        // Absent -> clean None.
        assert_eq!(get_api_key_for(TEST_SERVICE, &account).unwrap(), None);

        // Set -> get round-trips the exact value.
        set_api_key_for(TEST_SERVICE, &account, secret).unwrap();
        assert_eq!(
            get_api_key_for(TEST_SERVICE, &account).unwrap(),
            Some(secret.to_string())
        );

        // Overwrite works.
        set_api_key_for(TEST_SERVICE, &account, "sk-or-test-overwritten").unwrap();
        assert_eq!(
            get_api_key_for(TEST_SERVICE, &account).unwrap(),
            Some("sk-or-test-overwritten".to_string())
        );

        // Delete -> gone.
        delete_api_key_for(TEST_SERVICE, &account).unwrap();
        assert_eq!(get_api_key_for(TEST_SERVICE, &account).unwrap(), None);

        // Idempotent delete on a missing entry is fine.
        delete_api_key_for(TEST_SERVICE, &account).unwrap();
    }

    /// `has_api_key` reflects presence/absence without leaking the secret.
    /// Uses a throwaway account so the app's real key is never touched.
    #[test]
    fn has_api_key_reflects_store_state() {
        let _guard = STORE_LOCK.lock().unwrap();
        const TEST_SERVICE: &str = "translate-lector-test";
        let account = format!("has-{}", std::process::id());

        delete_api_key_for(TEST_SERVICE, &account).unwrap();
        assert!(!has_api_key_for(TEST_SERVICE, &account).unwrap());

        set_api_key_for(TEST_SERVICE, &account, "sk-or-test-present").unwrap();
        assert!(has_api_key_for(TEST_SERVICE, &account).unwrap());

        delete_api_key_for(TEST_SERVICE, &account).unwrap();
        assert!(!has_api_key_for(TEST_SERVICE, &account).unwrap());
    }
}

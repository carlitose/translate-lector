//! Secure storage for the OpenRouter API key (SPECIFICATION.md NFR07, §3.5, UC05).
//!
//! Backed by the OS-native credential store through the `keyring` crate. On
//! Windows this is the Credential Manager, which writes silently for the
//! current user (no system prompt, no admin rights). The single secret is keyed
//! by a fixed service/account pair so the whole app shares one entry.

use keyring::{Entry, Error};

/// Service name registered in the OS credential store.
const SERVICE: &str = "translate-lector";
/// Legacy account for OpenRouter's key. Kept as the back-compat anchor for the
/// unit test: `account_for("openrouter")` must equal this exact string so
/// existing users' keys are found without any migration (§8). Test-only because
/// the live source of truth is now [`account_for`].
#[cfg(test)]
const ACCOUNT: &str = "openrouter-api-key";

/// Keychain account for a provider's key: `"{provider_id}-api-key"`.
/// `openrouter` → `"openrouter-api-key"` (unchanged → existing keys are found
/// as-is, §3b/§8). Every provider (anche i server locali senza auth, D5) ha
/// il suo account, quindi il keychain le contiene tutte.
fn account_for(provider_id: &str) -> String {
    format!("{provider_id}-api-key")
}

/// Build an [`Entry`] for the given service/account against the default store.
fn entry(service: &str, account: &str) -> Result<Entry, Error> {
    Entry::new(service, account)
}

/// Store (or overwrite) the given provider's API key in the OS credential store.
pub fn set_api_key(provider_id: &str, key: &str) -> Result<(), Error> {
    set_api_key_for(SERVICE, &account_for(provider_id), key)
}

/// Read the given provider's API key. Returns `Ok(None)` when no key has been
/// stored yet — a missing credential is a clean absence, never an error.
pub fn get_api_key(provider_id: &str) -> Result<Option<String>, Error> {
    get_api_key_for(SERVICE, &account_for(provider_id))
}

/// Delete the given provider's stored API key. A missing credential is treated
/// as success (idempotent delete).
pub fn delete_api_key(provider_id: &str) -> Result<(), Error> {
    delete_api_key_for(SERVICE, &account_for(provider_id))
}

/// Whether the given provider has an API key, without exposing the secret
/// itself. Lets the UI reflect key presence while the plaintext key stays
/// inside the core.
pub fn has_api_key(provider_id: &str) -> Result<bool, Error> {
    has_api_key_for(SERVICE, &account_for(provider_id))
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

    /// Back-compat lock: `openrouter` must map to the account existing users
    /// already have (`openrouter-api-key`), so their key is found without any
    /// migration (§8). This assertion needs no keychain access.
    #[test]
    fn account_for_openrouter_matches_legacy_account() {
        assert_eq!(account_for("openrouter"), "openrouter-api-key");
        // Legacy constant and the derived account must agree.
        assert_eq!(account_for("openrouter"), ACCOUNT);
    }

    /// The account name is provider-scoped: `{provider_id}-api-key`.
    #[test]
    fn account_for_is_provider_scoped() {
        assert_eq!(account_for("lmstudio"), "lmstudio-api-key");
        assert_eq!(account_for("ollama"), "ollama-api-key");
    }

    /// Full round-trip through the provider-scoped PUBLIC surface, but pointed
    /// at a throwaway provider id so it never touches a real provider account.
    /// The throwaway id embeds the pid to stay unique across concurrent runs.
    #[test]
    fn provider_scoped_roundtrip_hits_real_store() {
        let _guard = STORE_LOCK.lock().unwrap();
        let provider_id = format!("it-provider-{}", std::process::id());
        let secret = "sk-or-provider-scoped-0123456789";

        // Clean slate.
        delete_api_key(&provider_id).unwrap();
        assert_eq!(get_api_key(&provider_id).unwrap(), None);
        assert!(!has_api_key(&provider_id).unwrap());

        // Set -> get round-trips the exact value; presence flips.
        set_api_key(&provider_id, secret).unwrap();
        assert_eq!(get_api_key(&provider_id).unwrap(), Some(secret.to_string()));
        assert!(has_api_key(&provider_id).unwrap());

        // The public API resolves to the provider-scoped account.
        assert_eq!(
            get_api_key_for(SERVICE, &account_for(&provider_id)).unwrap(),
            Some(secret.to_string())
        );

        // Delete -> gone; idempotent.
        delete_api_key(&provider_id).unwrap();
        assert_eq!(get_api_key(&provider_id).unwrap(), None);
        delete_api_key(&provider_id).unwrap();
    }
}

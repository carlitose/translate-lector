//! Global key/value settings (SPECIFICATION.md §3.5, §4.3).
//!
//! A thin wrapper over the `settings` table (see [`crate::db`]). Values are
//! opaque strings keyed by name; typed accessors (e.g. [`get_model`]) layer
//! defaults on top.

use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};

/// Settings key holding the OpenRouter model id.
pub const MODEL_KEY: &str = "model";
/// Default OpenRouter model when none has been chosen (decision D5). Updated
/// (July 2026) to a current model that supports both `temperature` and
/// `structured_outputs`, so the default translation call is never rejected by
/// the router for an unsupported parameter (bug #1, ticket 14).
pub const DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4.6";

/// Settings key holding the default target language for new documents (§3.5, D4).
pub const DEFAULT_TARGET_LANGUAGE_KEY: &str = "default_target_language";
/// Default target language when none is configured (decision D4: Italiano).
pub const DEFAULT_TARGET_LANGUAGE: &str = "it";

/// Settings key holding the user-chosen data directory (§3.5). An unset or blank
/// value means "use the OS app-data default".
pub const DATA_DIR_KEY: &str = "data_dir";

/// Settings key holding the rolling-summary token budget (§3.5, ticket 09).
pub const SUMMARY_TOKEN_LIMIT_KEY: &str = "summary_token_limit";
/// Default rolling-summary token budget (decision D5: ~800-1000 tokens).
pub const DEFAULT_SUMMARY_TOKEN_LIMIT: u32 = 1000;

/// Settings key holding the observed characters-per-token ratio, calibrated
/// from real `usage.prompt_tokens` after each translation (research §3).
pub const CHARS_PER_TOKEN_KEY: &str = "chars_per_token";

/// Settings key toggling background prefetch of the next page (§3.5, ticket 12).
pub const PREFETCH_ENABLED_KEY: &str = "prefetch_enabled";
/// Default prefetch state (decision D5: ON).
pub const DEFAULT_PREFETCH_ENABLED: bool = true;

/// Read a raw setting. Returns `Ok(None)` when the key is absent.
pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .optional()
}

/// Store (insert or overwrite) a raw setting.
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        (key, value),
    )?;
    Ok(())
}

/// Read the configured model, falling back to [`DEFAULT_MODEL`] when unset or
/// stored as an empty/whitespace value.
pub fn get_model(conn: &Connection) -> rusqlite::Result<String> {
    let stored = get_setting(conn, MODEL_KEY)?;
    Ok(match stored {
        Some(m) if !m.trim().is_empty() => m,
        _ => DEFAULT_MODEL.to_string(),
    })
}

/// Read the configured rolling-summary token limit, falling back to
/// [`DEFAULT_SUMMARY_TOKEN_LIMIT`] when unset or when the stored value is not a
/// positive integer.
pub fn get_summary_token_limit(conn: &Connection) -> rusqlite::Result<u32> {
    let stored = get_setting(conn, SUMMARY_TOKEN_LIMIT_KEY)?;
    Ok(match stored.as_deref().map(str::trim).map(str::parse::<u32>) {
        Some(Ok(n)) if n > 0 => n,
        _ => DEFAULT_SUMMARY_TOKEN_LIMIT,
    })
}

/// Read the default target language for new documents (§3.5, D4), falling back
/// to [`DEFAULT_TARGET_LANGUAGE`] when unset or stored blank.
pub fn get_default_target_language(conn: &Connection) -> rusqlite::Result<String> {
    let stored = get_setting(conn, DEFAULT_TARGET_LANGUAGE_KEY)?;
    Ok(match stored {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => DEFAULT_TARGET_LANGUAGE.to_string(),
    })
}

/// Resolve the effective data directory: the stored path when non-blank,
/// otherwise `default_dir`. Pure so the bootstrap path logic is unit-testable
/// without an [`AppHandle`]. Does **not** touch the filesystem.
pub fn resolve_data_dir(stored: Option<&str>, default_dir: &Path) -> PathBuf {
    match stored {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => default_dir.to_path_buf(),
    }
}

/// Read whether background prefetch is enabled (§3.5, ticket 12). Defaults to
/// [`DEFAULT_PREFETCH_ENABLED`] (ON, decision D5) when unset; a stored `"false"`
/// (case-insensitive) turns it off, anything else keeps it on.
pub fn get_prefetch_enabled(conn: &Connection) -> rusqlite::Result<bool> {
    let stored = get_setting(conn, PREFETCH_ENABLED_KEY)?;
    Ok(match stored.as_deref().map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("false") || v == "0" => false,
        Some(v) if !v.is_empty() => true,
        _ => DEFAULT_PREFETCH_ENABLED,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        db::init_schema(&c).unwrap();
        c
    }

    #[test]
    fn set_then_get_roundtrips() {
        let c = conn();
        assert_eq!(get_setting(&c, "foo").unwrap(), None);
        set_setting(&c, "foo", "bar").unwrap();
        assert_eq!(get_setting(&c, "foo").unwrap(), Some("bar".to_string()));
    }

    #[test]
    fn set_overwrites_existing_value() {
        let c = conn();
        set_setting(&c, "foo", "one").unwrap();
        set_setting(&c, "foo", "two").unwrap();
        assert_eq!(get_setting(&c, "foo").unwrap(), Some("two".to_string()));
    }

    #[test]
    fn get_model_defaults_when_unset() {
        let c = conn();
        assert_eq!(get_model(&c).unwrap(), DEFAULT_MODEL);
    }

    #[test]
    fn default_model_is_the_july_2026_sonnet() {
        // Bug #1 / ticket 14: default must be a current model that supports
        // temperature + structured_outputs (not the reasoning sonnet-5).
        assert_eq!(DEFAULT_MODEL, "anthropic/claude-sonnet-4.6");
    }

    #[test]
    fn get_model_returns_stored_value() {
        let c = conn();
        set_setting(&c, MODEL_KEY, "openai/gpt-4o").unwrap();
        assert_eq!(get_model(&c).unwrap(), "openai/gpt-4o");
    }

    #[test]
    fn get_model_falls_back_when_stored_blank() {
        let c = conn();
        set_setting(&c, MODEL_KEY, "   ").unwrap();
        assert_eq!(get_model(&c).unwrap(), DEFAULT_MODEL);
    }

    #[test]
    fn summary_token_limit_defaults_when_unset() {
        let c = conn();
        assert_eq!(
            get_summary_token_limit(&c).unwrap(),
            DEFAULT_SUMMARY_TOKEN_LIMIT
        );
    }

    #[test]
    fn summary_token_limit_returns_stored_value() {
        let c = conn();
        set_setting(&c, SUMMARY_TOKEN_LIMIT_KEY, "850").unwrap();
        assert_eq!(get_summary_token_limit(&c).unwrap(), 850);
    }

    #[test]
    fn prefetch_enabled_defaults_on_when_unset() {
        let c = conn();
        assert!(get_prefetch_enabled(&c).unwrap(), "D5: prefetch ON by default");
    }

    #[test]
    fn prefetch_enabled_reads_stored_toggle() {
        let c = conn();
        set_setting(&c, PREFETCH_ENABLED_KEY, "false").unwrap();
        assert!(!get_prefetch_enabled(&c).unwrap());
        set_setting(&c, PREFETCH_ENABLED_KEY, "true").unwrap();
        assert!(get_prefetch_enabled(&c).unwrap());
        set_setting(&c, PREFETCH_ENABLED_KEY, "0").unwrap();
        assert!(!get_prefetch_enabled(&c).unwrap());
    }

    #[test]
    fn default_target_language_defaults_to_it_when_unset() {
        let c = conn();
        assert_eq!(get_default_target_language(&c).unwrap(), DEFAULT_TARGET_LANGUAGE);
        assert_eq!(get_default_target_language(&c).unwrap(), "it");
    }

    #[test]
    fn default_target_language_returns_stored_value() {
        let c = conn();
        set_setting(&c, DEFAULT_TARGET_LANGUAGE_KEY, "es").unwrap();
        assert_eq!(get_default_target_language(&c).unwrap(), "es");
    }

    #[test]
    fn default_target_language_falls_back_when_stored_blank() {
        let c = conn();
        set_setting(&c, DEFAULT_TARGET_LANGUAGE_KEY, "   ").unwrap();
        assert_eq!(get_default_target_language(&c).unwrap(), DEFAULT_TARGET_LANGUAGE);
    }

    #[test]
    fn resolve_data_dir_uses_default_when_unset_or_blank() {
        let default_dir = Path::new("/app/data");
        assert_eq!(resolve_data_dir(None, default_dir), PathBuf::from("/app/data"));
        assert_eq!(resolve_data_dir(Some(""), default_dir), PathBuf::from("/app/data"));
        assert_eq!(resolve_data_dir(Some("   "), default_dir), PathBuf::from("/app/data"));
    }

    #[test]
    fn resolve_data_dir_uses_stored_path_when_present() {
        let default_dir = Path::new("/app/data");
        assert_eq!(
            resolve_data_dir(Some("  /custom/folder  "), default_dir),
            PathBuf::from("/custom/folder")
        );
    }

    #[test]
    fn summary_token_limit_falls_back_when_invalid_or_zero() {
        let c = conn();
        set_setting(&c, SUMMARY_TOKEN_LIMIT_KEY, "abc").unwrap();
        assert_eq!(
            get_summary_token_limit(&c).unwrap(),
            DEFAULT_SUMMARY_TOKEN_LIMIT
        );
        set_setting(&c, SUMMARY_TOKEN_LIMIT_KEY, "0").unwrap();
        assert_eq!(
            get_summary_token_limit(&c).unwrap(),
            DEFAULT_SUMMARY_TOKEN_LIMIT
        );
    }
}

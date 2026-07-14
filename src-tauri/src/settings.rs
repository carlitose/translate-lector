//! Global key/value settings (SPECIFICATION.md §3.5, §4.3).
//!
//! A thin wrapper over the `settings` table (see [`crate::db`]). Values are
//! opaque strings keyed by name; typed accessors (e.g. [`get_model`]) layer
//! defaults on top.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
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

// --- Provider presets + active provider (design §2-3, §8) --------------------

/// Settings key holding the id of the active provider.
pub const ACTIVE_PROVIDER_KEY: &str = "active_provider";
/// Default active provider when unset/blank. Decision D3: il default è locale
/// (Unsloth), non il cloud.
pub const DEFAULT_PROVIDER_ID: &str = "unsloth";

/// A selectable LLM provider: identità + endpoint + modello. `base_url` e `model`
/// hanno default built-in ma sono overridabili (persistiti in `settings`); per i
/// provider locali `model` parte vuoto ed è scelto poi a mano (free-text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Stable id: `openrouter` | `unsloth` | `lmstudio` | `ollama` | `llamaserver`.
    pub id: String,
    /// UI label (es. "OpenRouter (cloud)").
    pub label: String,
    /// Full `/v1/chat/completions` URL (default del preset o override utente).
    pub base_url: String,
    /// Model id per la richiesta (default del preset o override utente).
    pub model: String,
}

/// Built-in provider presets (design §2, research §5/§Q3). Le base-URL locali
/// sono default di partenza overridabili; Unsloth usa la porta reale dell'utente
/// (8888, non fissa: research §Q2). Il `model` dei provider locali è vuoto: lo
/// sceglie l'utente più tardi.
pub fn provider_presets() -> Vec<ProviderConfig> {
    vec![
        ProviderConfig {
            id: "openrouter".to_string(),
            label: "OpenRouter (cloud)".to_string(),
            base_url: crate::llm::OPENROUTER_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
        },
        ProviderConfig {
            id: "unsloth".to_string(),
            label: "Unsloth Studio (locale)".to_string(),
            base_url: "http://localhost:8888/v1/chat/completions".to_string(),
            model: String::new(),
        },
        ProviderConfig {
            id: "lmstudio".to_string(),
            label: "LM Studio (locale)".to_string(),
            base_url: "http://localhost:1234/v1/chat/completions".to_string(),
            model: String::new(),
        },
        ProviderConfig {
            id: "ollama".to_string(),
            label: "Ollama (locale)".to_string(),
            base_url: "http://localhost:11434/v1/chat/completions".to_string(),
            model: String::new(),
        },
        ProviderConfig {
            id: "llamaserver".to_string(),
            label: "llama.cpp server (locale)".to_string(),
            base_url: "http://127.0.0.1:8080/v1/chat/completions".to_string(),
            model: String::new(),
        },
    ]
}

/// The built-in preset for `id`, or `None` when the id is not a known provider.
pub fn provider_preset(id: &str) -> Option<ProviderConfig> {
    provider_presets().into_iter().find(|p| p.id == id)
}

/// Settings key holding a provider's base-URL override: `provider.{id}.base_url`.
pub fn provider_base_url_key(id: &str) -> String {
    format!("provider.{id}.base_url")
}

/// Settings key holding a provider's model override: `provider.{id}.model`.
pub fn provider_model_key(id: &str) -> String {
    format!("provider.{id}.model")
}

/// Read the active provider id, falling back to [`DEFAULT_PROVIDER_ID`] (D3:
/// unsloth) when unset or stored blank.
pub fn get_active_provider(conn: &Connection) -> rusqlite::Result<String> {
    let stored = get_setting(conn, ACTIVE_PROVIDER_KEY)?;
    Ok(match stored {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => DEFAULT_PROVIDER_ID.to_string(),
    })
}

/// Store the active provider id.
pub fn set_active_provider(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    set_setting(conn, ACTIVE_PROVIDER_KEY, id)
}

/// Resolve a provider's effective [`ProviderConfig`]: preset defaults with any
/// stored `base_url`/`model` overrides applied on top. For `openrouter`, when
/// `provider.openrouter.model` is unset the model falls back to the legacy
/// `model` setting via [`get_model`] (back-compat §8), so existing users keep
/// their chosen model. An unknown id yields a bare config (id as label, empty
/// endpoint) so manual overrides still apply.
pub fn get_provider_config(conn: &Connection, id: &str) -> rusqlite::Result<ProviderConfig> {
    let preset = provider_preset(id).unwrap_or_else(|| ProviderConfig {
        id: id.to_string(),
        label: id.to_string(),
        base_url: String::new(),
        model: String::new(),
    });

    let base_url = match get_setting(conn, &provider_base_url_key(id))? {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => preset.base_url,
    };

    let model = match get_setting(conn, &provider_model_key(id))? {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        // openrouter: cade sul valore legacy `model` (o DEFAULT_MODEL) via get_model.
        _ if id == "openrouter" => get_model(conn)?,
        _ => preset.model,
    };

    Ok(ProviderConfig {
        id: preset.id,
        label: preset.label,
        base_url,
        model,
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

    // --- Provider presets + active provider (ticket 07) ---------------------

    #[test]
    fn active_provider_defaults_to_unsloth_when_unset() {
        // Decision D3: il provider attivo di default è locale (Unsloth).
        let c = conn();
        assert_eq!(get_active_provider(&c).unwrap(), "unsloth");
        assert_eq!(get_active_provider(&c).unwrap(), DEFAULT_PROVIDER_ID);
    }

    #[test]
    fn active_provider_roundtrips_when_set() {
        let c = conn();
        set_active_provider(&c, "openrouter").unwrap();
        assert_eq!(get_active_provider(&c).unwrap(), "openrouter");
        set_active_provider(&c, "lmstudio").unwrap();
        assert_eq!(get_active_provider(&c).unwrap(), "lmstudio");
    }

    #[test]
    fn active_provider_falls_back_when_stored_blank() {
        let c = conn();
        set_setting(&c, ACTIVE_PROVIDER_KEY, "   ").unwrap();
        assert_eq!(get_active_provider(&c).unwrap(), DEFAULT_PROVIDER_ID);
    }

    #[test]
    fn provider_presets_contains_the_five_expected_ids_and_base_urls() {
        let presets = provider_presets();
        let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["openrouter", "unsloth", "lmstudio", "ollama", "llamaserver"]);

        let by = |id: &str| provider_preset(id).unwrap();
        assert_eq!(by("openrouter").base_url, crate::llm::OPENROUTER_URL);
        assert_eq!(by("unsloth").base_url, "http://localhost:8888/v1/chat/completions");
        assert_eq!(by("lmstudio").base_url, "http://localhost:1234/v1/chat/completions");
        assert_eq!(by("ollama").base_url, "http://localhost:11434/v1/chat/completions");
        assert_eq!(by("llamaserver").base_url, "http://127.0.0.1:8080/v1/chat/completions");
        // openrouter parte dal modello di default corrente (= DEFAULT_MODEL).
        assert_eq!(by("openrouter").model, DEFAULT_MODEL);
    }

    #[test]
    fn provider_key_helpers_format_scoped_keys() {
        assert_eq!(provider_base_url_key("unsloth"), "provider.unsloth.base_url");
        assert_eq!(provider_model_key("ollama"), "provider.ollama.model");
    }

    #[test]
    fn get_provider_config_returns_preset_defaults_when_no_overrides() {
        let c = conn();
        let cfg = get_provider_config(&c, "lmstudio").unwrap();
        assert_eq!(cfg.id, "lmstudio");
        assert_eq!(cfg.label, "LM Studio (locale)");
        assert_eq!(cfg.base_url, "http://localhost:1234/v1/chat/completions");
        assert_eq!(cfg.model, "");
    }

    #[test]
    fn get_provider_config_applies_base_url_and_model_overrides() {
        let c = conn();
        set_setting(
            &c,
            &provider_base_url_key("unsloth"),
            "http://localhost:9000/v1/chat/completions",
        )
        .unwrap();
        set_setting(&c, &provider_model_key("unsloth"), "qwen2.5:7b").unwrap();
        let cfg = get_provider_config(&c, "unsloth").unwrap();
        assert_eq!(cfg.base_url, "http://localhost:9000/v1/chat/completions");
        assert_eq!(cfg.model, "qwen2.5:7b");
    }

    #[test]
    fn get_provider_config_openrouter_falls_back_to_legacy_model() {
        // Nessun provider.openrouter.model, ma un `model` legacy impostato: deve
        // vincere (back-compat §8) così gli utenti OpenRouter mantengono la scelta.
        let c = conn();
        set_setting(&c, MODEL_KEY, "openai/gpt-4o").unwrap();
        let cfg = get_provider_config(&c, "openrouter").unwrap();
        assert_eq!(cfg.base_url, crate::llm::OPENROUTER_URL);
        assert_eq!(cfg.model, "openai/gpt-4o");
    }

    #[test]
    fn get_provider_config_openrouter_defaults_to_default_model_without_legacy() {
        let c = conn();
        let cfg = get_provider_config(&c, "openrouter").unwrap();
        assert_eq!(cfg.model, DEFAULT_MODEL);
    }

    #[test]
    fn get_provider_config_openrouter_model_override_beats_legacy() {
        let c = conn();
        set_setting(&c, MODEL_KEY, "openai/gpt-4o").unwrap();
        set_setting(&c, &provider_model_key("openrouter"), "anthropic/claude-opus-4.8").unwrap();
        let cfg = get_provider_config(&c, "openrouter").unwrap();
        assert_eq!(cfg.model, "anthropic/claude-opus-4.8");
    }
}

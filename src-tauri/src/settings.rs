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
/// Default active provider when unset/blank. Decision D5: su installazione pulita
/// il default è il provider llama.cpp diretto (`llamaserver`), non il cloud né
/// Unsloth Studio — la traduzione locale funziona out-of-the-box con lo spawn
/// on-demand di llama-server (ticket 04/05). Chi ha già una scelta persistita la
/// mantiene (D3: `unsloth` resta selezionabile di prima classe).
pub const DEFAULT_PROVIDER_ID: &str = "llamaserver";

/// Default `max_tokens` per il provider cloud (OpenRouter). Valore generoso:
/// i modelli cloud hanno finestre ampie, quindi la traduzione di pagine lunghe
/// non viene troncata (nessuna regressione rispetto al vecchio 4096 hardcoded).
pub const DEFAULT_MAX_TOKENS_CLOUD: u32 = 4096;
/// Default `max_tokens` per i provider locali (unsloth/lmstudio/ollama/llamaserver).
/// Ticket 02: i modelli locali girano spesso con `n_ctx ~4096`; chiedere l'intera
/// finestra come output non lascia spazio per `prompt + reasoning + content`
/// (empty-content, `finish_reason: length`). 2048 riserva margine dentro una
/// finestra ~4096 (invariante: mai `max_tokens >=` la finestra piccola) restando
/// abbastanza ampio per la traduzione di una pagina/chunk. Overridabile per-provider.
pub const DEFAULT_MAX_TOKENS_LOCAL: u32 = 2048;

/// Default context window (`n_ctx`) per il provider cloud (OpenRouter). Valore
/// molto grande così la formula di budget (spec §"Modello di budget token",
/// STC-01/08) non vincola mai la traduzione a pagina intera: sul cloud la
/// finestra è ampia e il chunking degrada a una singola chiamata per pagina.
pub const DEFAULT_N_CTX_CLOUD: u32 = 128_000;
/// Default context window (`n_ctx`) per i provider locali
/// (unsloth/lmstudio/ollama/llamaserver). I modelli locali girano tipicamente
/// con una finestra ~4096 token (spec §"Modello di budget token"): è il default
/// sensato da cui il budget deriva `budget_input`/`budget_unit_text`.
/// Overridabile per-provider (`provider.<id>.n_ctx`).
pub const DEFAULT_N_CTX_LOCAL: u32 = 4096;

/// Default request timeout (secondi) per il provider cloud (OpenRouter).
/// Ticket 13: coincide col default implicito di `reqwest::blocking::Client::new()`
/// (docs.rs `blocking/client.rs`: `Timeout(Some(Duration::from_secs(30)))`,
/// confermato in `docs/tickets/local-translation-latency/done/01-research-latency-baseline.md`),
/// quindi renderlo esplicito non cambia il comportamento OpenRouter.
pub const DEFAULT_TIMEOUT_SECS_CLOUD: u32 = 30;
/// Default request timeout (secondi) per i provider locali
/// (unsloth/lmstudio/ollama/llamaserver). Generoso: l'inferenza locale su
/// pagine dense può richiedere minuti (ticket 13). Overridabile per-provider
/// (`provider.<id>.timeout_secs`).
pub const DEFAULT_TIMEOUT_SECS_LOCAL: u32 = 180;

/// Default `binary_path` per il preset `llamaserver` (ticket 05, assunzione 1).
/// Casa **stabile** del binario: FUORI dalla dir temporanea e dal repo git. La
/// release ufficiale llama.cpp (llama-server.exe + DLL CUDA sorelle) va copiata
/// qui una volta sola. NON puntare al build Unsloth: dipende dalle DLL del venv
/// di Studio, la dipendenza che questa mappa sta rimuovendo.
pub const DEFAULT_LLAMASERVER_BINARY_PATH: &str =
    r"C:\Users\CGS03\.translate-lector\llama.cpp\llama-server.exe";

/// Default `model_path` per il preset `llamaserver` (ticket 05, D2). Path
/// **esplicito** al GGUF gemma-4 già in cache HuggingFace — NON auto-glob: l'hash
/// di snapshot nel path è fragile (cambia se si ri-scarica il modello) e in cache
/// può esserci più di un GGUF, quindi un default esplicito rende chiaro quale file
/// è in uso.
pub const DEFAULT_LLAMASERVER_MODEL_PATH: &str =
    r"C:\Users\CGS03\.cache\huggingface\hub\models--unsloth--gemma-4-E2B-it-qat-GGUF\snapshots\2ea637031baa8dc847d64b5dbb7011fd6a445849\gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf";

/// Messaggio d'errore azionabile (ticket 05) quando il binario llama-server
/// configurato non esiste sul disco: indirizza l'utente a ⚙️ invece di uno spawn
/// opaco (D2).
pub const LLAMA_BINARY_MISSING_MSG: &str = "Binario llama-server non trovato: imposta il path in ⚙️";

/// Messaggio d'errore azionabile (ticket 05) quando il file GGUF configurato non
/// esiste sul disco.
pub const LLAMA_MODEL_MISSING_MSG: &str = "Modello GGUF non trovato: imposta il path in ⚙️";

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
    /// Tetto `max_tokens` per la generazione (default del preset o override
    /// utente). Ticket 02: cloud generoso, locale con margine entro `n_ctx`.
    pub max_tokens: u32,
    /// Context window del modello (default del preset o override utente).
    /// Ticket 07: input della formula di budget (locale ~4096, cloud molto
    /// grande così il budget non vincola).
    pub n_ctx: u32,
    /// Timeout della richiesta HTTP in secondi (default del preset o override
    /// utente). Ticket 13: esplicito e configurabile per-provider (cloud 30s
    /// = default implicito reqwest invariato; locale 180s, generoso per
    /// l'inferenza lenta).
    pub timeout_secs: u32,
    /// Path locale del binario `llama-server` (ticket 05, provider `llamaserver`).
    /// Default precompilato alla casa stabile del binario ufficiale; vuoto per gli
    /// altri provider. Override utente in `provider.<id>.binary_path`. Consumato
    /// dallo spawner del ticket 04 (`-m` + binario), non lanciato qui.
    pub binary_path: String,
    /// Path locale del file GGUF (ticket 05, provider `llamaserver`). Default
    /// precompilato al GGUF esplicito in cache HF (D2, niente auto-glob); vuoto per
    /// gli altri provider. Override utente in `provider.<id>.model_path`.
    pub model_path: String,
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
            max_tokens: DEFAULT_MAX_TOKENS_CLOUD,
            n_ctx: DEFAULT_N_CTX_CLOUD,
            timeout_secs: DEFAULT_TIMEOUT_SECS_CLOUD,
            binary_path: String::new(),
            model_path: String::new(),
        },
        ProviderConfig {
            id: "unsloth".to_string(),
            label: "Unsloth Studio (locale)".to_string(),
            base_url: "http://localhost:8888/v1/chat/completions".to_string(),
            model: String::new(),
            max_tokens: DEFAULT_MAX_TOKENS_LOCAL,
            n_ctx: DEFAULT_N_CTX_LOCAL,
            timeout_secs: DEFAULT_TIMEOUT_SECS_LOCAL,
            binary_path: String::new(),
            model_path: String::new(),
        },
        ProviderConfig {
            id: "lmstudio".to_string(),
            label: "LM Studio (locale)".to_string(),
            base_url: "http://localhost:1234/v1/chat/completions".to_string(),
            model: String::new(),
            max_tokens: DEFAULT_MAX_TOKENS_LOCAL,
            n_ctx: DEFAULT_N_CTX_LOCAL,
            timeout_secs: DEFAULT_TIMEOUT_SECS_LOCAL,
            binary_path: String::new(),
            model_path: String::new(),
        },
        ProviderConfig {
            id: "ollama".to_string(),
            label: "Ollama (locale)".to_string(),
            base_url: "http://localhost:11434/v1/chat/completions".to_string(),
            model: String::new(),
            max_tokens: DEFAULT_MAX_TOKENS_LOCAL,
            n_ctx: DEFAULT_N_CTX_LOCAL,
            timeout_secs: DEFAULT_TIMEOUT_SECS_LOCAL,
            binary_path: String::new(),
            model_path: String::new(),
        },
        ProviderConfig {
            id: "llamaserver".to_string(),
            label: "llama.cpp server (locale)".to_string(),
            base_url: "http://127.0.0.1:8080/v1/chat/completions".to_string(),
            model: String::new(),
            max_tokens: DEFAULT_MAX_TOKENS_LOCAL,
            n_ctx: DEFAULT_N_CTX_LOCAL,
            timeout_secs: DEFAULT_TIMEOUT_SECS_LOCAL,
            // Ticket 05: default precompilati (D2 + assunzione 1). Solo questo
            // preset ha path significativi.
            binary_path: DEFAULT_LLAMASERVER_BINARY_PATH.to_string(),
            model_path: DEFAULT_LLAMASERVER_MODEL_PATH.to_string(),
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

/// Settings key holding a provider's `max_tokens` override:
/// `provider.{id}.max_tokens` (ticket 02).
pub fn provider_max_tokens_key(id: &str) -> String {
    format!("provider.{id}.max_tokens")
}

/// Settings key holding a provider's context-window override:
/// `provider.{id}.n_ctx` (ticket 07).
pub fn provider_nctx_key(id: &str) -> String {
    format!("provider.{id}.n_ctx")
}

/// Settings key holding a provider's request-timeout override:
/// `provider.{id}.timeout_secs` (ticket 13).
pub fn provider_timeout_key(id: &str) -> String {
    format!("provider.{id}.timeout_secs")
}

/// Settings key holding a provider's llama-server binary-path override:
/// `provider.{id}.binary_path` (ticket 05).
pub fn provider_binary_path_key(id: &str) -> String {
    format!("provider.{id}.binary_path")
}

/// Settings key holding a provider's GGUF model-path override:
/// `provider.{id}.model_path` (ticket 05).
pub fn provider_model_path_key(id: &str) -> String {
    format!("provider.{id}.model_path")
}

/// Read the active provider id, falling back to [`DEFAULT_PROVIDER_ID`] (D5:
/// `llamaserver`, llama.cpp diretto) when unset or stored blank. Un valore già
/// persistito (utenti esistenti) vince sempre sul default.
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
/// Resolve a per-provider `u32` override stored under `key`, falling back to
/// `default` when the setting is absent, blank, unparsable, or not a strictly
/// positive integer. Shared by `max_tokens`, `n_ctx`, and `timeout_secs` in
/// [`get_provider_config`], which previously duplicated this get → trim →
/// parse → validate chain three times (maintainability review, ticket 13).
fn resolve_u32_override(conn: &Connection, key: &str, default: u32) -> rusqlite::Result<u32> {
    Ok(
        match get_setting(conn, key)?.as_deref().map(str::trim).map(str::parse::<u32>) {
            Some(Ok(n)) if n > 0 => n,
            _ => default,
        },
    )
}

/// Resolve a per-provider `String` override stored under `key`, falling back to
/// `default` when the setting is absent or blank. Trims surrounding whitespace.
/// Shared by `binary_path`/`model_path` in [`get_provider_config`] (ticket 05),
/// mirroring the get → trim → non-blank fallback used for `base_url`/`model`.
fn resolve_string_override(conn: &Connection, key: &str, default: &str) -> rusqlite::Result<String> {
    Ok(match get_setting(conn, key)? {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    })
}

/// Validated local paths for the direct llama.cpp provider (ticket 05).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLlamaPaths {
    /// Existing path to the `llama-server` binary.
    pub binary_path: PathBuf,
    /// Existing path to the GGUF model file.
    pub model_path: PathBuf,
}

/// Whether a configured path is non-blank and exists on disk. Single source of
/// truth for the "does this configured path point at a real file" predicate
/// (ticket 05): used by the `validate_provider_paths` command for its
/// `binary_exists`/`model_exists` flags. Pure w.r.t. its `&str` input; touches
/// the filesystem via `Path::exists`.
pub fn path_configured_and_exists(path: &str) -> bool {
    let p = path.trim();
    !p.is_empty() && std::path::Path::new(p).exists()
}

/// Validate the configured binary + model paths, returning the resolved paths or
/// an **actionable Italian error** (`LLAMA_BINARY_MISSING_MSG` /
/// `LLAMA_MODEL_MISSING_MSG`). The `exists` predicate is injected so this seam is
/// unit-testable without touching the filesystem; [`validate_llama_paths`] wires
/// the real `Path::exists`. Checks the binary first, then the model. Does **not**
/// spawn any process — that is the ticket 04 spawner's job (D1).
pub fn validate_llama_paths_with<F>(
    binary_path: &str,
    model_path: &str,
    exists: F,
) -> Result<ResolvedLlamaPaths, String>
where
    F: Fn(&Path) -> bool,
{
    let binary = PathBuf::from(binary_path.trim());
    if binary_path.trim().is_empty() || !exists(&binary) {
        return Err(LLAMA_BINARY_MISSING_MSG.to_string());
    }
    let model = PathBuf::from(model_path.trim());
    if model_path.trim().is_empty() || !exists(&model) {
        return Err(LLAMA_MODEL_MISSING_MSG.to_string());
    }
    Ok(ResolvedLlamaPaths {
        binary_path: binary,
        model_path: model,
    })
}

/// Filesystem-backed [`validate_llama_paths_with`] using `Path::exists`.
pub fn validate_llama_paths(
    binary_path: &str,
    model_path: &str,
) -> Result<ResolvedLlamaPaths, String> {
    validate_llama_paths_with(binary_path, model_path, |p| p.exists())
}

pub fn get_provider_config(conn: &Connection, id: &str) -> rusqlite::Result<ProviderConfig> {
    let preset = provider_preset(id).unwrap_or_else(|| ProviderConfig {
        id: id.to_string(),
        label: id.to_string(),
        base_url: String::new(),
        model: String::new(),
        // Un id sconosciuto è trattato in modo conservativo come "locale":
        // meglio riservare margine di output che chiedere l'intera finestra.
        max_tokens: DEFAULT_MAX_TOKENS_LOCAL,
        // Idem per il context window: una finestra piccola è più prudente per
        // il budget che sovrastimare lo spazio disponibile.
        n_ctx: DEFAULT_N_CTX_LOCAL,
        // Idem per il timeout: un id sconosciuto è trattato come locale, quindi
        // generoso invece del breve default cloud.
        timeout_secs: DEFAULT_TIMEOUT_SECS_LOCAL,
        // Un id sconosciuto non ha default di path: solo `llamaserver` li usa.
        binary_path: String::new(),
        model_path: String::new(),
    });

    let base_url = resolve_string_override(conn, &provider_base_url_key(id), &preset.base_url)?;

    let model = match get_setting(conn, &provider_model_key(id))? {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        // openrouter: cade sul valore legacy `model` (o DEFAULT_MODEL) via get_model.
        _ if id == "openrouter" => get_model(conn)?,
        _ => preset.model,
    };

    // max_tokens override: intero positivo, altrimenti il default del preset
    // (cloud generoso / locale con margine). Un valore invalido o 0 è ignorato.
    let max_tokens = resolve_u32_override(conn, &provider_max_tokens_key(id), preset.max_tokens)?;

    // n_ctx override (ticket 07): intero positivo, altrimenti il default del
    // preset (locale ~4096 / cloud molto grande). Un valore invalido o 0 è
    // ignorato, così il budget non parte mai da una finestra assurda.
    let n_ctx = resolve_u32_override(conn, &provider_nctx_key(id), preset.n_ctx)?;

    // timeout_secs override (ticket 13): intero positivo, altrimenti il default
    // del preset (cloud 30s = default reqwest invariato / locale 180s generoso).
    // Un valore invalido o 0 è ignorato.
    let timeout_secs = resolve_u32_override(conn, &provider_timeout_key(id), preset.timeout_secs)?;

    // binary_path / model_path override (ticket 05): stringa non vuota, altrimenti
    // il default del preset (precompilato solo per `llamaserver`). Consumati dallo
    // spawner del ticket 04; la validazione dell'esistenza è in validate_llama_paths.
    let binary_path =
        resolve_string_override(conn, &provider_binary_path_key(id), &preset.binary_path)?;
    let model_path =
        resolve_string_override(conn, &provider_model_path_key(id), &preset.model_path)?;

    Ok(ProviderConfig {
        id: preset.id,
        label: preset.label,
        base_url,
        model,
        max_tokens,
        n_ctx,
        timeout_secs,
        binary_path,
        model_path,
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
    fn active_provider_defaults_to_llamaserver_on_first_run() {
        // Decision D5: su prima esecuzione (nessuna scelta persistita) il provider
        // attivo di default è llama.cpp diretto (`llamaserver`), non il cloud né
        // Unsloth Studio.
        let c = conn();
        assert_eq!(get_active_provider(&c).unwrap(), "llamaserver");
        assert_eq!(get_active_provider(&c).unwrap(), DEFAULT_PROVIDER_ID);
    }

    #[test]
    fn active_provider_persisted_choice_wins_over_default() {
        // D3/D5: un utente esistente con una scelta persistita (es. `unsloth`,
        // che resta selezionabile di prima classe) la mantiene sul default.
        let c = conn();
        set_active_provider(&c, "openrouter").unwrap();
        assert_eq!(get_active_provider(&c).unwrap(), "openrouter");
        set_active_provider(&c, "lmstudio").unwrap();
        assert_eq!(get_active_provider(&c).unwrap(), "lmstudio");
        set_active_provider(&c, "unsloth").unwrap();
        assert_eq!(get_active_provider(&c).unwrap(), "unsloth");
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

    // --- Ticket 02: per-provider max_tokens (output headroom) ----------------

    #[test]
    fn provider_max_tokens_key_formats_scoped_key() {
        assert_eq!(provider_max_tokens_key("unsloth"), "provider.unsloth.max_tokens");
        assert_eq!(provider_max_tokens_key("openrouter"), "provider.openrouter.max_tokens");
    }

    #[test]
    fn openrouter_preset_keeps_a_generous_max_tokens() {
        // Cloud has a large context window: keep the generous default so long
        // page translations are NOT truncated (no regression vs. the old 4096).
        assert_eq!(provider_preset("openrouter").unwrap().max_tokens, DEFAULT_MAX_TOKENS_CLOUD);
        assert_eq!(DEFAULT_MAX_TOKENS_CLOUD, 4096);
    }

    #[test]
    fn local_presets_default_to_a_smaller_max_tokens_with_headroom() {
        // Local models often run a ~4096 window: requesting the whole window as
        // output leaves no room to generate. Every local preset reserves headroom.
        for id in ["unsloth", "lmstudio", "ollama", "llamaserver"] {
            let mt = provider_preset(id).unwrap().max_tokens;
            assert_eq!(mt, DEFAULT_MAX_TOKENS_LOCAL, "{id} defaults to the local headroom value");
        }
    }

    #[test]
    fn local_default_max_tokens_is_smaller_than_cloud_and_below_a_small_window() {
        // The key invariant: never request max_tokens == a small local context
        // window; keep it strictly below so prompt+reasoning+output can coexist.
        assert!(DEFAULT_MAX_TOKENS_LOCAL < DEFAULT_MAX_TOKENS_CLOUD);
        assert!(DEFAULT_MAX_TOKENS_LOCAL < 4096, "leaves room within a ~4096 window");
    }

    #[test]
    fn get_provider_config_returns_the_preset_max_tokens_by_default() {
        let c = conn();
        assert_eq!(
            get_provider_config(&c, "openrouter").unwrap().max_tokens,
            DEFAULT_MAX_TOKENS_CLOUD
        );
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().max_tokens,
            DEFAULT_MAX_TOKENS_LOCAL
        );
    }

    #[test]
    fn get_provider_config_honors_a_max_tokens_override() {
        let c = conn();
        set_setting(&c, &provider_max_tokens_key("unsloth"), "1024").unwrap();
        assert_eq!(get_provider_config(&c, "unsloth").unwrap().max_tokens, 1024);
    }

    #[test]
    fn get_provider_config_ignores_an_invalid_or_zero_max_tokens_override() {
        let c = conn();
        set_setting(&c, &provider_max_tokens_key("unsloth"), "abc").unwrap();
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().max_tokens,
            DEFAULT_MAX_TOKENS_LOCAL
        );
        set_setting(&c, &provider_max_tokens_key("unsloth"), "0").unwrap();
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().max_tokens,
            DEFAULT_MAX_TOKENS_LOCAL
        );
    }

    // --- Ticket 07: per-provider n_ctx (context window per il budget) --------

    #[test]
    fn provider_nctx_key_formats_scoped_key() {
        assert_eq!(provider_nctx_key("unsloth"), "provider.unsloth.n_ctx");
        assert_eq!(provider_nctx_key("openrouter"), "provider.openrouter.n_ctx");
    }

    #[test]
    fn openrouter_preset_keeps_a_large_n_ctx_so_the_budget_never_constrains() {
        // Cloud ha una finestra ampia: n_ctx molto grande così la formula di
        // budget (STC-01/08) non vincola mai la traduzione a pagina intera.
        assert_eq!(provider_preset("openrouter").unwrap().n_ctx, DEFAULT_N_CTX_CLOUD);
        assert_eq!(DEFAULT_N_CTX_CLOUD, 128_000);
    }

    #[test]
    fn local_presets_default_to_a_small_n_ctx() {
        // I modelli locali girano tipicamente con una finestra ~4096 token
        // (spec §"Modello di budget token"): è il default sensato per il budget.
        for id in ["unsloth", "lmstudio", "ollama", "llamaserver"] {
            let n = provider_preset(id).unwrap().n_ctx;
            assert_eq!(n, DEFAULT_N_CTX_LOCAL, "{id} defaults to the local n_ctx");
        }
        assert_eq!(DEFAULT_N_CTX_LOCAL, 4096);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn local_default_n_ctx_is_smaller_than_cloud() {
        assert!(DEFAULT_N_CTX_LOCAL < DEFAULT_N_CTX_CLOUD);
    }

    #[test]
    fn get_provider_config_returns_the_preset_n_ctx_by_default() {
        let c = conn();
        assert_eq!(
            get_provider_config(&c, "openrouter").unwrap().n_ctx,
            DEFAULT_N_CTX_CLOUD
        );
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().n_ctx,
            DEFAULT_N_CTX_LOCAL
        );
    }

    #[test]
    fn get_provider_config_honors_an_n_ctx_override() {
        let c = conn();
        set_setting(&c, &provider_nctx_key("unsloth"), "8192").unwrap();
        assert_eq!(get_provider_config(&c, "unsloth").unwrap().n_ctx, 8192);
    }

    #[test]
    fn get_provider_config_ignores_an_invalid_or_zero_n_ctx_override() {
        let c = conn();
        set_setting(&c, &provider_nctx_key("unsloth"), "abc").unwrap();
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().n_ctx,
            DEFAULT_N_CTX_LOCAL
        );
        set_setting(&c, &provider_nctx_key("unsloth"), "0").unwrap();
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().n_ctx,
            DEFAULT_N_CTX_LOCAL
        );
    }

    #[test]
    fn get_provider_config_unknown_id_uses_conservative_local_n_ctx() {
        // Un id sconosciuto è trattato in modo conservativo come "locale":
        // meglio una finestra piccola che sovrastimare il budget disponibile.
        let c = conn();
        assert_eq!(
            get_provider_config(&c, "mystery").unwrap().n_ctx,
            DEFAULT_N_CTX_LOCAL
        );
    }

    // --- Ticket 13: per-provider timeout_secs (inferenza locale lenta) -------

    #[test]
    fn provider_timeout_key_formats_scoped_key() {
        assert_eq!(provider_timeout_key("unsloth"), "provider.unsloth.timeout_secs");
        assert_eq!(provider_timeout_key("openrouter"), "provider.openrouter.timeout_secs");
    }

    #[test]
    fn openrouter_preset_keeps_the_reqwest_default_timeout() {
        // Cloud invariato: 30s coincide col default implicito di
        // reqwest::blocking::Client::new() (confermato nel research del ticket
        // local-translation-latency/01), quindi renderlo esplicito non cambia
        // il comportamento OpenRouter.
        assert_eq!(provider_preset("openrouter").unwrap().timeout_secs, DEFAULT_TIMEOUT_SECS_CLOUD);
        assert_eq!(DEFAULT_TIMEOUT_SECS_CLOUD, 30);
    }

    #[test]
    fn local_presets_default_to_a_generous_timeout() {
        // I modelli locali possono impiegare minuti su pagine dense (ticket 13):
        // ogni preset locale usa il default generoso.
        for id in ["unsloth", "lmstudio", "ollama", "llamaserver"] {
            let t = provider_preset(id).unwrap().timeout_secs;
            assert_eq!(t, DEFAULT_TIMEOUT_SECS_LOCAL, "{id} defaults to the local timeout");
        }
        assert_eq!(DEFAULT_TIMEOUT_SECS_LOCAL, 180);
    }

    #[test]
    fn local_default_timeout_is_larger_than_cloud() {
        assert!(DEFAULT_TIMEOUT_SECS_LOCAL > DEFAULT_TIMEOUT_SECS_CLOUD);
    }

    #[test]
    fn get_provider_config_returns_the_preset_timeout_by_default() {
        let c = conn();
        assert_eq!(
            get_provider_config(&c, "openrouter").unwrap().timeout_secs,
            DEFAULT_TIMEOUT_SECS_CLOUD
        );
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().timeout_secs,
            DEFAULT_TIMEOUT_SECS_LOCAL
        );
    }

    #[test]
    fn get_provider_config_honors_a_timeout_override() {
        let c = conn();
        set_setting(&c, &provider_timeout_key("unsloth"), "60").unwrap();
        assert_eq!(get_provider_config(&c, "unsloth").unwrap().timeout_secs, 60);
    }

    #[test]
    fn get_provider_config_ignores_an_invalid_or_zero_timeout_override() {
        let c = conn();
        set_setting(&c, &provider_timeout_key("unsloth"), "abc").unwrap();
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().timeout_secs,
            DEFAULT_TIMEOUT_SECS_LOCAL
        );
        set_setting(&c, &provider_timeout_key("unsloth"), "0").unwrap();
        assert_eq!(
            get_provider_config(&c, "unsloth").unwrap().timeout_secs,
            DEFAULT_TIMEOUT_SECS_LOCAL
        );
    }

    #[test]
    fn get_provider_config_unknown_id_uses_conservative_local_timeout() {
        // Idem per il timeout: un id sconosciuto è trattato come locale,
        // quindi generoso invece del breve default cloud.
        let c = conn();
        assert_eq!(
            get_provider_config(&c, "mystery").unwrap().timeout_secs,
            DEFAULT_TIMEOUT_SECS_LOCAL
        );
    }

    // --- Ticket 05: llamaserver binary_path + model_path (path management) ----

    #[test]
    fn provider_binary_path_key_formats_scoped_key() {
        assert_eq!(
            provider_binary_path_key("llamaserver"),
            "provider.llamaserver.binary_path"
        );
        assert_eq!(provider_binary_path_key("unsloth"), "provider.unsloth.binary_path");
    }

    #[test]
    fn provider_model_path_key_formats_scoped_key() {
        assert_eq!(
            provider_model_path_key("llamaserver"),
            "provider.llamaserver.model_path"
        );
        assert_eq!(provider_model_path_key("ollama"), "provider.ollama.model_path");
    }

    #[test]
    fn llamaserver_preset_has_default_binary_and_model_paths() {
        // D2 / assunzione 1: default precompilati alla casa stabile del binario
        // e al GGUF esplicito in cache HF (niente auto-glob).
        let p = provider_preset("llamaserver").unwrap();
        assert_eq!(p.binary_path, DEFAULT_LLAMASERVER_BINARY_PATH);
        assert_eq!(p.model_path, DEFAULT_LLAMASERVER_MODEL_PATH);
    }

    #[test]
    fn llamaserver_binary_default_is_outside_temp_and_repo() {
        // Assunzione 1: la casa stabile del binario NON è la dir temporanea di
        // scratch né il build Unsloth (dipendente dal venv di Studio).
        let b = DEFAULT_LLAMASERVER_BINARY_PATH.to_lowercase();
        assert!(!b.contains("\\temp\\"), "binary default must not live in temp");
        assert!(!b.contains("unsloth"), "binary default must not be the Unsloth build");
        assert!(b.ends_with("llama-server.exe"));
    }

    #[test]
    fn other_presets_default_to_empty_binary_and_model_paths() {
        // Solo llamaserver ha default significativi; gli altri partono vuoti.
        for id in ["openrouter", "unsloth", "lmstudio", "ollama"] {
            let p = provider_preset(id).unwrap();
            assert_eq!(p.binary_path, "", "{id} binary_path default is empty");
            assert_eq!(p.model_path, "", "{id} model_path default is empty");
        }
    }

    #[test]
    fn get_provider_config_returns_preset_paths_by_default() {
        let c = conn();
        let cfg = get_provider_config(&c, "llamaserver").unwrap();
        assert_eq!(cfg.binary_path, DEFAULT_LLAMASERVER_BINARY_PATH);
        assert_eq!(cfg.model_path, DEFAULT_LLAMASERVER_MODEL_PATH);
    }

    #[test]
    fn get_provider_config_applies_binary_and_model_path_overrides() {
        let c = conn();
        set_setting(&c, &provider_binary_path_key("llamaserver"), "  D:\\bin\\llama-server.exe  ")
            .unwrap();
        set_setting(&c, &provider_model_path_key("llamaserver"), "D:\\models\\custom.gguf").unwrap();
        let cfg = get_provider_config(&c, "llamaserver").unwrap();
        assert_eq!(cfg.binary_path, "D:\\bin\\llama-server.exe");
        assert_eq!(cfg.model_path, "D:\\models\\custom.gguf");
    }

    #[test]
    fn get_provider_config_ignores_blank_path_overrides() {
        let c = conn();
        set_setting(&c, &provider_binary_path_key("llamaserver"), "   ").unwrap();
        let cfg = get_provider_config(&c, "llamaserver").unwrap();
        assert_eq!(cfg.binary_path, DEFAULT_LLAMASERVER_BINARY_PATH);
    }

    #[test]
    fn get_provider_config_unknown_id_has_empty_paths() {
        let c = conn();
        let cfg = get_provider_config(&c, "mystery").unwrap();
        assert_eq!(cfg.binary_path, "");
        assert_eq!(cfg.model_path, "");
    }

    #[test]
    fn path_configured_and_exists_is_false_when_blank() {
        assert!(!path_configured_and_exists(""));
        assert!(!path_configured_and_exists("   "));
    }

    #[test]
    fn path_configured_and_exists_is_false_when_missing() {
        assert!(!path_configured_and_exists("C:\\definitely\\not\\here.gguf"));
    }

    #[test]
    fn path_configured_and_exists_is_true_for_a_real_file() {
        // The crate manifest is guaranteed to exist on disk; a surrounding-space
        // variant must still resolve (the predicate trims first).
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        assert!(path_configured_and_exists(manifest));
        assert!(path_configured_and_exists(&format!("  {manifest}  ")));
    }

    #[test]
    fn validate_llama_paths_ok_when_both_exist() {
        let res = validate_llama_paths_with("C:\\bin\\llama-server.exe", "C:\\m\\g.gguf", |_| true)
            .unwrap();
        assert_eq!(res.binary_path, PathBuf::from("C:\\bin\\llama-server.exe"));
        assert_eq!(res.model_path, PathBuf::from("C:\\m\\g.gguf"));
    }

    #[test]
    fn validate_llama_paths_errors_when_binary_missing() {
        let bin = "C:\\bin\\llama-server.exe";
        let err = validate_llama_paths_with(bin, "C:\\m\\g.gguf", |p| p != Path::new(bin))
            .unwrap_err();
        assert_eq!(err, LLAMA_BINARY_MISSING_MSG);
    }

    #[test]
    fn validate_llama_paths_errors_when_model_missing() {
        let model = "C:\\m\\g.gguf";
        let err = validate_llama_paths_with("C:\\bin\\llama-server.exe", model, |p| {
            p != Path::new(model)
        })
        .unwrap_err();
        assert_eq!(err, LLAMA_MODEL_MISSING_MSG);
    }

    #[test]
    fn validate_llama_paths_errors_when_binary_path_blank() {
        let err = validate_llama_paths_with("   ", "C:\\m\\g.gguf", |_| true).unwrap_err();
        assert_eq!(err, LLAMA_BINARY_MISSING_MSG);
    }

    #[test]
    fn validate_llama_paths_applies_resolved_override_paths() {
        // Override-applied case: la validazione gira sui path risolti da
        // get_provider_config (override utente), non sui default del preset.
        let c = conn();
        let overridden_bin = "D:\\bin\\llama-server.exe";
        let overridden_model = "D:\\models\\custom.gguf";
        set_setting(&c, &provider_binary_path_key("llamaserver"), overridden_bin).unwrap();
        set_setting(&c, &provider_model_path_key("llamaserver"), overridden_model).unwrap();
        let cfg = get_provider_config(&c, "llamaserver").unwrap();
        // Only the overridden paths "exist"; the preset defaults do not.
        let exists = |p: &Path| {
            p == Path::new(overridden_bin) || p == Path::new(overridden_model)
        };
        let res = validate_llama_paths_with(&cfg.binary_path, &cfg.model_path, exists).unwrap();
        assert_eq!(res.binary_path, PathBuf::from(overridden_bin));
        assert_eq!(res.model_path, PathBuf::from(overridden_model));
    }
}

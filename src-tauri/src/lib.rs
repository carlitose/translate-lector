mod db;
mod documents;
mod glossary;
mod llm;
mod secrets;
mod settings;
mod translate;

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Manager;

// --- Serialize prefetch vs on-demand + cancel stale jobs (ticket 06) --------
//
// The local provider (llama-server behind Unsloth Studio) is single-model: a
// prefetch (`update_context=false`) and an on-demand translation
// (`update_context=true`) running as concurrent `spawn_blocking` tasks would
// fight over the GPU and slow each other down (decision brief L3/L4). Two
// pieces of Tauri-managed state coordinate this, kept as thin wrappers around
// plain data so the decision logic itself is pure and unit-testable without
// any Tauri/Mutex machinery (see `is_page_current` / `update_current_page`
// below and their tests).

/// `document_id -> page_number` of the last page requested **on-demand**
/// (`update_context == true`). Written only by on-demand requests, read by
/// every in-flight job (on-demand or prefetch) to decide whether it is still
/// translating the page the user is actually looking at.
struct CurrentPage(Mutex<HashMap<i64, i64>>);

impl CurrentPage {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

/// Single-occupant slot for the local provider: held for the whole duration of
/// a `translate::translate_page` call when the active provider is local
/// (`llm::is_local_url`). Cloud calls never acquire it and stay concurrent, as
/// before. Because a stale/superseded job notices at the next unit boundary
/// and cancels quickly (see `translate::TranslateParams::is_current`), holding
/// this lock does not starve an on-demand request queued behind a prefetch —
/// the prefetch yields the slot at its next boundary instead of running to
/// completion (L3: priority to on-demand without true HTTP-level preemption).
struct LocalProviderSlot(Mutex<()>);

impl LocalProviderSlot {
    fn new() -> Self {
        Self(Mutex::new(()))
    }
}

/// Locks `m`, recovering the inner value if the mutex was poisoned by a panic
/// while held. Neither `CurrentPage`'s `HashMap<i64, i64>` nor
/// `LocalProviderSlot`'s `()` carries an invariant that a panic mid-update
/// could leave broken, so refusing to lock ever again (the default poisoning
/// behaviour) would only turn one unrelated panic into a permanent, app-wide
/// outage for local translation and cursor tracking. Recovering here is safe
/// and keeps both features usable after a transient panic elsewhere.
fn lock_ignoring_poison<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether `translate::TranslateParams::is_current` should be wired up for a
/// provider at `base_url` (Ticket 06, L3/L4). Only the local provider is
/// serialized behind `LocalProviderSlot` and can be preempted by a fresher
/// on-demand request at the next unit boundary, so only it needs the
/// staleness check. Cloud providers must keep their pre-ticket behaviour of
/// always running to completion (and populating the page cache) regardless
/// of whether the page is still current when they finish — passing
/// `is_current` to them would let an in-flight cloud call be cancelled
/// without ever making its API call, silently losing that cache population.
fn should_check_is_current(base_url: &str) -> bool {
    llm::is_local_url(base_url)
}

/// Whether `page_number` is still the current page for `document_id`,
/// according to `cursor` (`document_id -> page_number`, written only by
/// on-demand requests). No cursor recorded yet for that document (the
/// earliest possible call) is treated as "current" — there is nothing yet to
/// contradict it. Pure and Mutex-free so it is directly unit-testable.
fn is_page_current(cursor: &HashMap<i64, i64>, document_id: i64, page_number: i64) -> bool {
    match cursor.get(&document_id) {
        Some(&current) => current == page_number,
        None => true,
    }
}

/// Record `page_number` as the current page for `document_id`. Must be called
/// ONLY for on-demand requests (`update_context == true`); prefetch requests
/// must never call this (they only read the cursor via `is_page_current`).
/// Pure and Mutex-free so the "write on-demand only" rule is unit-testable
/// without touching the `Mutex`/Tauri state.
fn update_current_page(cursor: &mut HashMap<i64, i64>, document_id: i64, page_number: i64) {
    cursor.insert(document_id, page_number);
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    // --- is_page_current -----------------------------------------------------

    #[test]
    fn no_cursor_recorded_yet_is_treated_as_current() {
        // Earliest-possible-call edge case (design doc, ticket 06): a document
        // with no cursor entry must not block the very first request.
        let cursor: HashMap<i64, i64> = HashMap::new();
        assert!(is_page_current(&cursor, 1, 7), "no constraint yet == current");
    }

    #[test]
    fn matching_page_number_is_current() {
        let mut cursor = HashMap::new();
        cursor.insert(1, 7);
        assert!(is_page_current(&cursor, 1, 7));
    }

    #[test]
    fn differing_page_number_for_the_same_document_is_not_current() {
        let mut cursor = HashMap::new();
        cursor.insert(1, 7); // the user navigated to page 7
        assert!(!is_page_current(&cursor, 1, 5), "page 5 is stale once 7 is current");
    }

    #[test]
    fn a_different_document_is_unaffected_by_another_documents_cursor() {
        let mut cursor = HashMap::new();
        cursor.insert(1, 7);
        assert!(
            is_page_current(&cursor, 2, 1),
            "document 2 has no cursor entry of its own yet -> current"
        );
    }

    // --- update_current_page --------------------------------------------------

    #[test]
    fn update_current_page_writes_the_cursor_for_its_document() {
        let mut cursor = HashMap::new();
        update_current_page(&mut cursor, 1, 3);
        assert_eq!(cursor.get(&1), Some(&3));
    }

    #[test]
    fn update_current_page_overwrites_a_previous_value_for_the_same_document() {
        let mut cursor = HashMap::new();
        cursor.insert(1, 3);
        update_current_page(&mut cursor, 1, 4); // real navigation moved on
        assert_eq!(cursor.get(&1), Some(&4), "the cursor tracks only the latest on-demand page");
    }

    #[test]
    fn update_current_page_is_the_only_way_the_cursor_changes_prefetch_must_never_call_it() {
        // This test documents the invariant at the call site (`translate_page`
        // command): prefetch requests (`update_context == false`) must read
        // via `is_page_current` only and never call `update_current_page`. The
        // function itself has no `update_context` parameter by design -- it
        // cannot special-case prefetch, so the caller is what must enforce the
        // rule (verified by inspection of the `translate_page` command, which
        // gates the call behind `if update_context { ... }`).
        let mut cursor = HashMap::new();
        update_current_page(&mut cursor, 9, 2);
        assert_eq!(cursor.len(), 1);
        assert_eq!(cursor.get(&9), Some(&2));
    }

    // --- should_check_is_current ----------------------------------------------

    #[test]
    fn should_check_is_current_is_true_for_the_local_provider() {
        assert!(should_check_is_current("http://localhost:8888/v1/chat/completions"));
        assert!(should_check_is_current("http://127.0.0.1:1234/v1"));
    }

    #[test]
    fn should_check_is_current_is_false_for_a_cloud_provider() {
        // Ticket 06 fix: the staleness check (`is_current`) must NOT be wired
        // up for cloud providers (e.g. OpenRouter). Cloud calls are not
        // serialized behind `LocalProviderSlot` and must keep their
        // pre-ticket behaviour of always running to completion and
        // populating the page cache, even if the page is no longer current
        // by the time they finish.
        assert!(!should_check_is_current("https://openrouter.ai/api/v1/chat/completions"));
        assert!(!should_check_is_current("https://api.example.com/v1/chat/completions"));
    }
}

/// Bridge check: proves the webview -> core invoke path works.
#[tauri::command]
fn ping() -> String {
    "pong from Rust core".to_string()
}

/// Store (or overwrite) a provider's API key in the OS credential store.
#[tauri::command]
fn store_api_key(provider_id: String, key: String) -> Result<(), String> {
    secrets::set_api_key(&provider_id, &key).map_err(|e| e.to_string())
}

/// Load a provider's API key. Returns `null` when none is stored.
#[tauri::command]
fn load_api_key(provider_id: String) -> Result<Option<String>, String> {
    secrets::get_api_key(&provider_id).map_err(|e| e.to_string())
}

/// Remove a provider's stored API key (idempotent).
#[tauri::command]
fn clear_api_key(provider_id: String) -> Result<(), String> {
    secrets::delete_api_key(&provider_id).map_err(|e| e.to_string())
}

/// Whether a provider's API key is stored, without exposing the secret.
#[tauri::command]
fn has_api_key(provider_id: String) -> Result<bool, String> {
    secrets::has_api_key(&provider_id).map_err(|e| e.to_string())
}

/// Read a global setting by key. Returns `null` when unset.
#[tauri::command]
fn get_setting(app: tauri::AppHandle, key: String) -> Result<Option<String>, String> {
    let conn = open_db(&app)?;
    settings::get_setting(&conn, &key).map_err(|e| e.to_string())
}

/// Store (insert or overwrite) a global setting.
#[tauri::command]
fn set_setting(app: tauri::AppHandle, key: String, value: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    settings::set_setting(&conn, &key, &value).map_err(|e| e.to_string())
}

/// Read the configured OpenRouter model, falling back to the default (D5).
#[tauri::command]
fn get_model(app: tauri::AppHandle) -> Result<String, String> {
    let conn = open_db(&app)?;
    settings::get_model(&conn).map_err(|e| e.to_string())
}

/// Read the active provider id (D3: default "unsloth" when unset).
#[tauri::command]
fn get_active_provider(app: tauri::AppHandle) -> Result<String, String> {
    let conn = open_db(&app)?;
    settings::get_active_provider(&conn).map_err(|e| e.to_string())
}

/// Set the active provider id (one of the built-in preset ids).
#[tauri::command]
fn set_active_provider(app: tauri::AppHandle, provider_id: String) -> Result<(), String> {
    let conn = open_db(&app)?;
    settings::set_active_provider(&conn, &provider_id).map_err(|e| e.to_string())
}

/// Resolve one provider's effective config: preset defaults + stored overrides
/// (openrouter falls back to the legacy `model` key).
#[tauri::command]
fn get_provider_config(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<settings::ProviderConfig, String> {
    let conn = open_db(&app)?;
    settings::get_provider_config(&conn, &provider_id).map_err(|e| e.to_string())
}

/// List the built-in provider presets (id + label + default base_url/model) for
/// the settings UI.
#[tauri::command]
fn list_providers() -> Vec<settings::ProviderConfig> {
    settings::provider_presets()
}

/// Read the default target language for new documents (§3.5, D4), falling back
/// to the default ("it") when unset.
#[tauri::command]
fn get_default_target_language(app: tauri::AppHandle) -> Result<String, String> {
    let conn = open_db(&app)?;
    settings::get_default_target_language(&conn).map_err(|e| e.to_string())
}

/// Empty the per-page translation cache (§3.5 "Svuota cache"). Returns the number
/// of cached translations removed; leaves documents/sessions/glossary/settings.
#[tauri::command]
fn clear_translations_cache(app: tauri::AppHandle) -> Result<usize, String> {
    let conn = open_db(&app)?;
    db::clear_translations_cache(&conn).map_err(|e| e.to_string())
}

/// Outcome of choosing a data folder (§3.5). `restart_required` is always `true`
/// for the MVP: the new folder is used for the DB only from the next launch.
#[derive(serde::Serialize)]
struct DataDirResult {
    /// The absolute folder now recorded for the local data (DB/cache/glossary).
    path: String,
    /// Whether the app must be restarted for the change to take effect (MVP: yes).
    restart_required: bool,
}

/// Read the effective data folder: the user-chosen path when set, else the OS
/// app-data default. Reflects what the DB actually uses on this launch.
#[tauri::command]
fn get_data_dir(app: tauri::AppHandle) -> Result<String, String> {
    let default_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    let stored = read_data_dir_pointer(&app);
    let dir = settings::resolve_data_dir(stored.as_deref(), &default_dir);
    Ok(dir.to_string_lossy().to_string())
}

/// Record a new data folder (§3.5). Safe by design for the MVP: it creates the
/// target directory if needed, persists the choice (in `settings` and in a
/// bootstrap pointer read at startup) and reports that a restart is required. It
/// never moves or deletes the existing `.db` — the user copies data across
/// manually. Takes effect on the next launch.
#[tauri::command]
fn set_data_dir(app: tauri::AppHandle, path: String) -> Result<DataDirResult, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("La cartella dati non può essere vuota.".to_string());
    }
    std::fs::create_dir_all(trimmed)
        .map_err(|e| format!("cannot create data dir {trimmed}: {e}"))?;

    // Persist in the settings table (source of truth for §3.5).
    let conn = open_db(&app)?;
    settings::set_setting(&conn, settings::DATA_DIR_KEY, trimmed).map_err(|e| e.to_string())?;

    // Bootstrap pointer, read before the DB is opened on the next launch so the
    // choice survives regardless of which DB is currently open.
    let pointer = data_dir_pointer_path(&app)?;
    std::fs::write(&pointer, trimmed)
        .map_err(|e| format!("cannot write data-dir pointer: {e}"))?;

    Ok(DataDirResult {
        path: trimmed.to_string(),
        restart_required: true,
    })
}

/// Ensure the SQLite database exists and its schema is initialised.
/// Returns the absolute path of the `.db` file.
#[tauri::command]
fn init_database(app: tauri::AppHandle) -> Result<String, String> {
    let db_path = database_path(&app).map_err(|e| e.to_string())?;
    db::open_and_init(&db_path).map_err(|e| e.to_string())?;
    Ok(db_path.to_string_lossy().to_string())
}

/// Open the app database (creating/upgrading its schema first).
fn open_db(app: &tauri::AppHandle) -> Result<rusqlite::Connection, String> {
    let db_path = database_path(app)?;
    db::open_and_init(&db_path).map_err(|e| e.to_string())
}

/// Read a PDF file's raw bytes for pdf.js to render in the webview.
///
/// Returns the bytes as a binary IPC response (an `ArrayBuffer` on the JS side)
/// rather than a JSON number array, keeping large files cheap to transfer.
#[tauri::command]
fn read_pdf_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Register/refresh a document by partial hash (D2) and return its stored row.
#[tauri::command]
fn register_document(
    app: tauri::AppHandle,
    path: String,
    total_pages: i64,
    title: String,
) -> Result<documents::Document, String> {
    let conn = open_db(&app)?;
    documents::register_document(&conn, &path, total_pages, &title)
}

/// Load or create the page-discrete reading session (D1) for a document.
#[tauri::command]
fn open_or_create_session(
    app: tauri::AppHandle,
    document_id: i64,
) -> Result<documents::Session, String> {
    let conn = open_db(&app)?;
    documents::open_or_create_session(&conn, document_id)
}

/// Persist reading progress (current page + target language) for a session.
#[tauri::command]
fn update_session(
    app: tauri::AppHandle,
    session_id: i64,
    current_page: i64,
    target_language: String,
) -> Result<(), String> {
    let conn = open_db(&app)?;
    documents::update_session(&conn, session_id, current_page, &target_language)
}

/// Load the most-recent session joined to its document, for startup restore
/// (FR10). Returns `null` when nothing has been read yet.
#[tauri::command]
fn get_last_session(app: tauri::AppHandle) -> Result<Option<documents::LastSession>, String> {
    let conn = open_db(&app)?;
    documents::get_last_session(&conn)
}

/// List recently opened documents for the "Recenti" list (FR09), newest first.
#[tauri::command]
fn list_recent_documents(
    app: tauri::AppHandle,
    limit: i64,
) -> Result<Vec<documents::RecentDocument>, String> {
    let conn = open_db(&app)?;
    documents::list_recent_documents(&conn, limit)
}

/// Whether a stored PDF path still points at a readable file (EC06).
#[tauri::command]
fn file_exists(path: String) -> bool {
    documents::file_exists(&path)
}

/// Re-match a user-picked file to a document by partial hash (EC06). Returns the
/// refreshed document on a match, or `null` when the file is a different one.
#[tauri::command]
fn relocate_document(
    app: tauri::AppHandle,
    document_id: i64,
    candidate_path: String,
) -> Result<Option<documents::Document>, String> {
    let conn = open_db(&app)?;
    documents::relocate_document(&conn, document_id, &candidate_path)
}

/// Drop a document from "Recenti" without deleting its data (EC06).
#[tauri::command]
fn remove_recent(app: tauri::AppHandle, document_id: i64) -> Result<(), String> {
    let conn = open_db(&app)?;
    documents::remove_recent(&conn, document_id)
}

/// Translate a page via OpenRouter, honouring the per-page cache
/// (SPECIFICATION §3.2/§4.4, UC02, tickets 08/09/12). Runs the blocking service
/// on a worker thread so the webview stays responsive during the network call.
///
/// `update_context` is `true` on real navigation (advances the percettore:
/// rolling summary + glossary) and `false` on **prefetch** of the next page,
/// which only warms the cache (ticket 12). Transient network/API errors are
/// retried with exponential backoff (NFR06); 429 (EC07) is retried too.
#[tauri::command]
async fn translate_page(
    app: tauri::AppHandle,
    document_id: i64,
    page_number: i64,
    target_language: String,
    page_text: String,
    update_context: bool,
) -> Result<translate::TranslationResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_db(&app)?;

        // Ticket 06 (L3/L4): on-demand requests are the ONLY writer of the
        // current-page cursor, and they write it BEFORE translating, so an
        // in-flight prefetch for another page notices as early as possible
        // that it is now stale. Prefetch never writes the cursor, only reads
        // it (via the `is_current` predicate below).
        let current_page = app.state::<CurrentPage>();
        if update_context {
            let mut cursor = lock_ignoring_poison(&current_page.0);
            update_current_page(&mut cursor, document_id, page_number);
        }
        let is_current = move || {
            let cursor = lock_ignoring_poison(&current_page.0);
            is_page_current(&cursor, document_id, page_number)
        };

        // Risolvi il provider attivo (D3: default unsloth) e la sua config
        // (base-URL + modello, con override e fallback legacy `model` per
        // openrouter). La chiave è provider-scoped (Ticket 06).
        let active_id = settings::get_active_provider(&conn).map_err(|e| e.to_string())?;
        let cfg = settings::get_provider_config(&conn, &active_id).map_err(|e| e.to_string())?;
        let api_key = secrets::get_api_key(&active_id)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        // Costruisci il client sull'endpoint del provider attivo; le attribution
        // header OpenRouter partono solo per openrouter (Ticket 05).
        let base = llm::ChatCompletionsClient::new(
            cfg.base_url.clone(),
            api_key,
            /* send_openrouter_headers = */ active_id == "openrouter",
            cfg.timeout_secs,
        );
        // Retry transient failures (5xx/429/offline) with backoff (NFR06). A
        // Timeout is retried too on cloud (unchanged), but NOT on a local
        // provider (ticket 13 / decision L4): a systematically slow local
        // server signals a real problem, so retrying just triples the wait.
        // The local/remote distinction lives next to `RetryPolicy` itself
        // (`llm::RetryPolicy::for_base_url`) so other call sites reuse it.
        let retry_policy = llm::RetryPolicy::for_base_url(&cfg.base_url);
        let client = llm::RetryingChatClient::new(&base, retry_policy);
        let params = translate::TranslateParams {
            document_id,
            page_number,
            target_language: &target_language,
            page_text: &page_text,
            model: &cfg.model,
            // Tetto max_tokens del provider attivo (Ticket 02): cloud generoso,
            // locale con margine entro n_ctx per lasciare spazio all'output.
            max_tokens: cfg.max_tokens,
            // Context window del provider attivo (Ticket 07): input della formula
            // di budget (STC-08). Locale ~4096 → unità piccole; cloud grande →
            // una sola unità = pagina intera (degradazione D2).
            n_ctx: cfg.n_ctx,
            update_context,
            // Ticket 06 (L3/L4): only the local provider needs the staleness
            // check (see `should_check_is_current`). Cloud providers keep
            // their pre-ticket behaviour unchanged — they always run to
            // completion and populate the page cache, even if the page is no
            // longer current by the time they finish.
            is_current: if should_check_is_current(&cfg.base_url) { Some(&is_current) } else { None },
        };

        // Serialize the local provider (L3/L4): one in-flight translation at a
        // time, held for the whole call. Cloud is left concurrent/unchanged —
        // no slot is acquired. Priority to on-demand is realized by the stale
        // job noticing `is_current() == false` at its next unit boundary and
        // returning quickly (see `translate::TranslateParams::is_current`), not
        // by true HTTP-level preemption (the client is blocking, out of scope).
        // The slot's `State` is bound to a local so its `Arc`-backed `Mutex`
        // outlives the guard (a `State` temporary would be freed too early).
        let local_slot = app.state::<LocalProviderSlot>();
        let _local_guard = if llm::is_local_url(&cfg.base_url) {
            Some(lock_ignoring_poison(&local_slot.0))
        } else {
            None
        };
        translate::translate_page(&conn, &client, &params).map_err(|e| e.user_message())
    })
    .await
    .map_err(|e| format!("translation task failed: {e}"))?
}

/// Cheap reachability probe for the active/selected provider (ticket 09, D3/D7),
/// used by the non-blocking onboarding hint. Resolves the provider's effective
/// `base_url` and does a short-timeout GET to its `/v1/models`. Returns `true`
/// when the endpoint answers at all, `false` when it is down (connection refused
/// / timeout). Never throws for "down" — a missing/misconfigured server is just
/// `false`. Runs on a worker thread so the short network wait never blocks the UI.
/// It performs no translation and never falls back to the cloud (D4).
#[tauri::command]
async fn check_provider_reachable(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open_db(&app)?;
        let cfg = settings::get_provider_config(&conn, &provider_id).map_err(|e| e.to_string())?;
        Ok::<bool, String>(llm::probe_reachable(&cfg.base_url))
    })
    .await
    .map_err(|e| format!("reachability check failed: {e}"))?
}

/// Whether background prefetch of the next page is enabled (§3.5, ticket 12).
/// Defaults to ON (decision D5).
#[tauri::command]
fn get_prefetch_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    let conn = open_db(&app)?;
    settings::get_prefetch_enabled(&conn).map_err(|e| e.to_string())
}

/// List the current document's glossary terms for the panel (ticket 10).
#[tauri::command]
fn list_glossary(
    app: tauri::AppHandle,
    document_id: i64,
) -> Result<Vec<glossary::GlossaryEntry>, String> {
    let conn = open_db(&app)?;
    glossary::list_glossary(&conn, document_id).map_err(|e| e.to_string())
}

/// Persist the user's edits to one glossary term (UC03): translation, note and
/// the `locked` absolute-constraint flag. Never creates duplicates.
#[tauri::command]
fn update_glossary_term(
    app: tauri::AppHandle,
    id: i64,
    translation: String,
    note: String,
    locked: bool,
) -> Result<(), String> {
    let conn = open_db(&app)?;
    glossary::update_glossary_term(&conn, id, &translation, &note, locked)
        .map_err(|e| e.to_string())
}

/// Resolve the `.db` path. Uses the user-chosen data folder recorded in the
/// bootstrap pointer (§3.5, ticket 13) when present, otherwise the OS app-data
/// default. Creates the chosen directory. The pointer is read here — before the
/// DB is opened — so relocating the folder takes effect on the next launch
/// without ever moving or deleting the existing file.
fn database_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let default_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    let stored = read_data_dir_pointer(app);
    let dir = settings::resolve_data_dir(stored.as_deref(), &default_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create data dir: {e}"))?;
    Ok(dir.join("translate-lector.db"))
}

/// Absolute path of the bootstrap pointer file that records the user-chosen data
/// folder. Kept in the OS app-config dir (not the data dir itself) so it always
/// survives a relocation. Creates the config dir on demand.
fn data_dir_pointer_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("cannot resolve app config dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create app config dir: {e}"))?;
    Ok(dir.join("data_dir.txt"))
}

/// Read the recorded data-folder pointer, or `None` when unset/blank/unreadable.
fn read_data_dir_pointer(app: &tauri::AppHandle) -> Option<String> {
    let pointer = data_dir_pointer_path(app).ok()?;
    let raw = std::fs::read_to_string(pointer).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Initialise SQLite on first run (idempotent thereafter).
            let db_path = database_path(&app.handle())?;
            db::open_and_init(&db_path)?;
            println!("SQLite initialised at {}", db_path.display());
            // Ticket 06 (L3/L4): shared state for serializing the local
            // provider and cancelling stale prefetch jobs. See the module-level
            // doc comment near `CurrentPage`/`LocalProviderSlot` above.
            app.manage(CurrentPage::new());
            app.manage(LocalProviderSlot::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            init_database,
            store_api_key,
            load_api_key,
            clear_api_key,
            has_api_key,
            get_setting,
            set_setting,
            get_model,
            get_active_provider,
            set_active_provider,
            get_provider_config,
            list_providers,
            get_default_target_language,
            clear_translations_cache,
            get_data_dir,
            set_data_dir,
            read_pdf_bytes,
            register_document,
            open_or_create_session,
            update_session,
            get_last_session,
            list_recent_documents,
            file_exists,
            relocate_document,
            remove_recent,
            translate_page,
            check_provider_reachable,
            get_prefetch_enabled,
            list_glossary,
            update_glossary_term
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

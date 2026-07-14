mod db;
mod documents;
mod glossary;
mod llm;
mod secrets;
mod settings;
mod translate;

use tauri::Manager;

/// Bridge check: proves the webview -> core invoke path works.
#[tauri::command]
fn ping() -> String {
    "pong from Rust core".to_string()
}

/// Store (or overwrite) the OpenRouter API key in the OS credential store.
#[tauri::command]
fn store_api_key(key: String) -> Result<(), String> {
    secrets::set_api_key(&key).map_err(|e| e.to_string())
}

/// Load the OpenRouter API key. Returns `null` when none is stored.
#[tauri::command]
fn load_api_key() -> Result<Option<String>, String> {
    secrets::get_api_key().map_err(|e| e.to_string())
}

/// Remove the stored OpenRouter API key (idempotent).
#[tauri::command]
fn clear_api_key() -> Result<(), String> {
    secrets::delete_api_key().map_err(|e| e.to_string())
}

/// Whether an OpenRouter API key is stored, without exposing the secret.
#[tauri::command]
fn has_api_key() -> Result<bool, String> {
    secrets::has_api_key().map_err(|e| e.to_string())
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
        let api_key = secrets::get_api_key()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let model = settings::get_model(&conn).map_err(|e| e.to_string())?;
        // Behaviour unchanged: still OpenRouter with the same endpoint, key and
        // attribution headers. Provider selection arrives in later tickets.
        let base = llm::ChatCompletionsClient::new(
            llm::OPENROUTER_URL,
            api_key,
            /* send_openrouter_headers = */ true,
        );
        // Retry transient failures (timeout/5xx/429/offline) with backoff (NFR06).
        let client = llm::RetryingChatClient::new(&base, llm::RetryPolicy::default());
        let params = translate::TranslateParams {
            document_id,
            page_number,
            target_language: &target_language,
            page_text: &page_text,
            model: &model,
            update_context,
        };
        translate::translate_page(&conn, &client, &params).map_err(|e| e.user_message())
    })
    .await
    .map_err(|e| format!("translation task failed: {e}"))?
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
            get_prefetch_enabled,
            list_glossary,
            update_glossary_term
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

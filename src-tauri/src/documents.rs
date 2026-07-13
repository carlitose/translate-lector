//! Document registration and reading-session persistence (SPECIFICATION §4.3,
//! ticket 06).
//!
//! A PDF is recognised by a **partial hash** (decision D2): SHA-256 over the
//! first and last [`PARTIAL_CHUNK`] bytes of the file plus its total size. This
//! lets the app recognise the same file even after it is renamed or moved,
//! without reading megabytes of PDF on every open.
//!
//! Sessions are **page-discrete** (decision D1): only `current_page` is tracked;
//! `scroll_position` is intentionally left unused.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Bytes hashed from the head and, separately, the tail of the file (D2).
const PARTIAL_CHUNK: u64 = 64 * 1024;

/// A row of the `documents` table as exposed to the frontend.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Document {
    pub document_id: i64,
    pub file_path: String,
    pub file_hash: String,
    pub title: String,
    pub total_pages: i64,
}

/// A row of the `sessions` table as exposed to the frontend (page-discrete, D1).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Session {
    pub session_id: i64,
    pub document_id: i64,
    pub target_language: String,
    pub current_page: i64,
}

/// Compute the partial hash (D2) of the file at `path`.
///
/// SHA-256 of `size ‖ head ‖ tail`, where `head`/`tail` are the first and last
/// [`PARTIAL_CHUNK`] bytes. For files no larger than `2 * PARTIAL_CHUNK` the
/// whole file is hashed once (head and tail would otherwise overlap). Folding
/// the size in keeps two different files that share head+tail bytes distinct.
pub fn partial_hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();

    let mut hasher = Sha256::new();
    hasher.update(size.to_le_bytes());

    if size <= 2 * PARTIAL_CHUNK {
        // Small file: head and tail would overlap, so hash the whole thing once.
        let mut buf = Vec::with_capacity(size as usize);
        file.read_to_end(&mut buf)?;
        hasher.update(&buf);
    } else {
        let mut head = vec![0u8; PARTIAL_CHUNK as usize];
        file.read_exact(&mut head)?;
        hasher.update(&head);

        let mut tail = vec![0u8; PARTIAL_CHUNK as usize];
        file.seek(SeekFrom::End(-(PARTIAL_CHUNK as i64)))?;
        file.read_exact(&mut tail)?;
        hasher.update(&tail);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Upsert a document by `file_hash`. Returns the stored [`Document`].
///
/// If a row with the same partial hash already exists it is updated in place
/// (path/title/pages refreshed, `last_opened_at` bumped) so a renamed or moved
/// file reuses the same `documents` row. Otherwise a new row is inserted.
pub fn register_document(
    conn: &Connection,
    path: &str,
    total_pages: i64,
    title: &str,
) -> Result<Document, String> {
    let file_hash = partial_hash(Path::new(path)).map_err(|e| format!("hashing failed: {e}"))?;

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM documents WHERE file_hash = ?1",
            params![file_hash],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let document_id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE documents
                    SET file_path = ?1, title = ?2, total_pages = ?3,
                        last_opened_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                  WHERE id = ?4",
                params![path, title, total_pages, id],
            )
            .map_err(|e| e.to_string())?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO documents (file_path, file_hash, title, total_pages, last_opened_at)
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                params![path, file_hash, title, total_pages],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
    };

    Ok(Document {
        document_id,
        file_path: path.to_string(),
        file_hash,
        title: title.to_string(),
        total_pages,
    })
}

/// Load the reading session for `document_id`, creating it with defaults
/// (`target_language = "it"`, `current_page = 1`) when none exists yet.
pub fn open_or_create_session(conn: &Connection, document_id: i64) -> Result<Session, String> {
    let existing = conn
        .query_row(
            "SELECT id, target_language, current_page
               FROM sessions
              WHERE document_id = ?1
              ORDER BY id
              LIMIT 1",
            params![document_id],
            |r| {
                Ok(Session {
                    session_id: r.get(0)?,
                    document_id,
                    target_language: r.get(1)?,
                    current_page: r.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if let Some(session) = existing {
        return Ok(session);
    }

    // New session adopts the configured default target language (§3.5, D4);
    // falls back to "it" when unset. Page-discrete default (D1): scroll unused.
    let default_language = crate::settings::get_default_target_language(conn)
        .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO sessions (document_id, target_language, current_page, updated_at)
         VALUES (?1, ?2, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        params![document_id, default_language],
    )
    .map_err(|e| e.to_string())?;

    Ok(Session {
        session_id: conn.last_insert_rowid(),
        document_id,
        target_language: default_language,
        current_page: 1,
    })
}

/// Persist reading progress: the discrete current page and the target language.
pub fn update_session(
    conn: &Connection,
    session_id: i64,
    current_page: i64,
    target_language: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE sessions
            SET current_page = ?1, target_language = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id = ?3",
        params![current_page, target_language, session_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The most-recent reading session joined to its document (FR10 restore).
///
/// Carries everything the frontend needs to reopen the last document at the
/// saved page/language without a second round-trip: the session progress plus
/// the document's path, partial hash (D2) and metadata.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastSession {
    pub session_id: i64,
    pub document_id: i64,
    pub target_language: String,
    pub current_page: i64,
    pub file_path: String,
    pub file_hash: String,
    pub title: String,
    pub total_pages: i64,
}

/// A `documents` history row for the "Recenti" list (FR09), newest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecentDocument {
    pub document_id: i64,
    pub file_path: String,
    pub file_hash: String,
    pub title: String,
    pub total_pages: i64,
    pub last_opened_at: String,
}

/// Load the session to restore on startup (FR10): the one updated most recently,
/// joined to its document. Returns `None` when nothing has been read yet.
pub fn get_last_session(conn: &Connection) -> Result<Option<LastSession>, String> {
    conn.query_row(
        "SELECT s.id, s.document_id, s.target_language, s.current_page,
                d.file_path, d.file_hash, d.title, d.total_pages
           FROM sessions s
           JOIN documents d ON d.id = s.document_id
          ORDER BY s.updated_at DESC, s.id DESC
          LIMIT 1",
        [],
        |r| {
            Ok(LastSession {
                session_id: r.get(0)?,
                document_id: r.get(1)?,
                target_language: r.get(2)?,
                current_page: r.get(3)?,
                file_path: r.get(4)?,
                file_hash: r.get(5)?,
                title: r.get(6)?,
                total_pages: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// List recently opened documents (FR09), most recent first, capped at `limit`.
///
/// Rows whose `last_opened_at` is NULL are excluded — this is how
/// [`remove_recent`] drops an entry from the list without deleting its data.
pub fn list_recent_documents(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<RecentDocument>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, file_path, file_hash, title, total_pages, last_opened_at
               FROM documents
              WHERE last_opened_at IS NOT NULL
              ORDER BY last_opened_at DESC, id DESC
              LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(RecentDocument {
                document_id: r.get(0)?,
                file_path: r.get(1)?,
                file_hash: r.get(2)?,
                title: r.get(3)?,
                total_pages: r.get(4)?,
                last_opened_at: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

/// Whether a stored `file_path` still points at a readable file (EC06).
///
/// A pure filesystem check: no DB access, never panics on a missing path — it
/// simply returns `false`. Used by the restore/recents path before opening.
pub fn file_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

/// Re-match a moved/renamed file to its `documents` row by partial hash (EC06).
///
/// Given a user-picked `candidate_path`, hash it and compare against the hash
/// stored for `document_id`. On a match the row's `file_path` is updated (and
/// `last_opened_at` bumped) and the refreshed [`Document`] returned. On a
/// mismatch — a different file — returns `Ok(None)` and leaves the row intact.
/// The original `documents`/cache rows are never deleted here.
pub fn relocate_document(
    conn: &Connection,
    document_id: i64,
    candidate_path: &str,
) -> Result<Option<Document>, String> {
    let stored: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT file_hash, title, total_pages FROM documents WHERE id = ?1",
            params![document_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let (stored_hash, title, total_pages) = match stored {
        Some(v) => v,
        None => return Ok(None),
    };

    let candidate_hash =
        partial_hash(Path::new(candidate_path)).map_err(|e| format!("hashing failed: {e}"))?;
    if candidate_hash != stored_hash {
        return Ok(None); // a different file — do not touch the row
    }

    conn.execute(
        "UPDATE documents
            SET file_path = ?1,
                last_opened_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE id = ?2",
        params![candidate_path, document_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(Some(Document {
        document_id,
        file_path: candidate_path.to_string(),
        file_hash: stored_hash,
        title,
        total_pages,
    }))
}

/// Drop a document from the "Recenti" list without deleting its data (EC06).
///
/// Clears `last_opened_at` so [`list_recent_documents`] no longer returns it;
/// the row, its cache, glossary and session are preserved so the user can
/// re-open it later (e.g. after restoring a moved file).
pub fn remove_recent(conn: &Connection, document_id: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE documents SET last_opened_at = NULL WHERE id = ?1",
        params![document_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read the rolling summary stored on the document's session (§3.3, §4.3).
/// Returns an empty string when there is no session yet or the column is NULL —
/// i.e. "first page, no context".
pub fn get_rolling_summary(conn: &Connection, document_id: i64) -> rusqlite::Result<String> {
    let value: Option<Option<String>> = conn
        .query_row(
            "SELECT rolling_summary FROM sessions
              WHERE document_id = ?1 ORDER BY id LIMIT 1",
            params![document_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    Ok(value.flatten().unwrap_or_default())
}

/// Persist the rolling summary on the document's session (§3.3). A no-op when no
/// session exists (0 rows updated is not an error); the real flow always creates
/// the session via [`open_or_create_session`] before translating.
pub fn set_rolling_summary(
    conn: &Connection,
    document_id: i64,
    summary: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions
            SET rolling_summary = ?1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
          WHERE document_id = ?2",
        params![summary, document_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tl-doc-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::init_schema(&c).unwrap();
        c
    }

    #[test]
    fn partial_hash_is_deterministic_and_stable_across_rename() {
        let dir = unique_dir("hash");
        // > 2 * PARTIAL_CHUNK so the head+tail branch runs.
        let mut bytes = vec![0u8; (PARTIAL_CHUNK as usize) * 2 + 5000];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let original = dir.join("original.pdf");
        write_file(&original, &bytes);

        let h1 = partial_hash(&original).unwrap();
        let h2 = partial_hash(&original).unwrap();
        assert_eq!(h1, h2, "hash must be deterministic");

        // Same bytes under a new name/location -> same hash.
        let renamed = dir.join("moved-then-renamed.pdf");
        write_file(&renamed, &bytes);
        assert_eq!(h1, partial_hash(&renamed).unwrap(), "hash stable across rename");

        // Different content -> different hash.
        let mut other = bytes.clone();
        other[0] ^= 0xFF;
        let diff = dir.join("different.pdf");
        write_file(&diff, &other);
        assert_ne!(h1, partial_hash(&diff).unwrap());
    }

    #[test]
    fn partial_hash_handles_files_smaller_than_two_chunks() {
        let dir = unique_dir("small");
        let small = dir.join("tiny.pdf");
        write_file(&small, b"%PDF-1.7 tiny file");
        // Must not panic and must be deterministic.
        let h = partial_hash(&small).unwrap();
        assert_eq!(h, partial_hash(&small).unwrap());
        assert_eq!(h.len(), 64, "sha-256 hex is 64 chars");
    }

    #[test]
    fn register_document_is_idempotent_by_hash() {
        let c = conn();
        let dir = unique_dir("upsert");
        let path = dir.join("doc.pdf");
        write_file(&path, b"%PDF body content for upsert test");
        let p = path.to_string_lossy().to_string();

        let d1 = register_document(&c, &p, 10, "doc").unwrap();
        let d2 = register_document(&c, &p, 10, "doc").unwrap();
        assert_eq!(d1.document_id, d2.document_id, "same hash -> same row");

        // Re-open the same bytes under a different path -> still one row, reused.
        let renamed = dir.join("doc-renamed.pdf");
        write_file(&renamed, b"%PDF body content for upsert test");
        let d3 = register_document(&c, &renamed.to_string_lossy(), 42, "renamed").unwrap();
        assert_eq!(d1.document_id, d3.document_id, "rename reuses the row");
        assert_eq!(d3.total_pages, 42, "metadata refreshed on re-open");
        assert_eq!(d3.file_path, renamed.to_string_lossy(), "path refreshed");

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "exactly one documents row for one hash");
    }

    #[test]
    fn open_or_create_session_creates_then_reloads_same_row() {
        let c = conn();
        let dir = unique_dir("session");
        let path = dir.join("s.pdf");
        write_file(&path, b"%PDF session test");
        let doc = register_document(&c, &path.to_string_lossy(), 5, "s").unwrap();

        let s1 = open_or_create_session(&c, doc.document_id).unwrap();
        assert_eq!(s1.target_language, "it", "default target language");
        assert_eq!(s1.current_page, 1, "default page");

        let s2 = open_or_create_session(&c, doc.document_id).unwrap();
        assert_eq!(s1, s2, "reload returns the same session row");

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "no duplicate session created");
    }

    #[test]
    fn open_or_create_session_uses_configured_default_language() {
        let c = conn();
        crate::settings::set_setting(&c, crate::settings::DEFAULT_TARGET_LANGUAGE_KEY, "es")
            .unwrap();
        let dir = unique_dir("deflang");
        let path = dir.join("dl.pdf");
        write_file(&path, b"%PDF default lang");
        let doc = register_document(&c, &path.to_string_lossy(), 5, "dl").unwrap();

        let s = open_or_create_session(&c, doc.document_id).unwrap();
        assert_eq!(s.target_language, "es", "new session uses configured default");
    }

    #[test]
    fn rolling_summary_defaults_empty_then_persists_and_reloads() {
        let c = conn();
        let dir = unique_dir("summary");
        let path = dir.join("sum.pdf");
        write_file(&path, b"%PDF summary test");
        let doc = register_document(&c, &path.to_string_lossy(), 5, "sum").unwrap();

        // No session yet -> empty.
        assert_eq!(get_rolling_summary(&c, doc.document_id).unwrap(), "");

        open_or_create_session(&c, doc.document_id).unwrap();
        // Fresh session -> still empty (NULL column).
        assert_eq!(get_rolling_summary(&c, doc.document_id).unwrap(), "");

        set_rolling_summary(&c, doc.document_id, "Riassunto pagina 1.").unwrap();
        assert_eq!(
            get_rolling_summary(&c, doc.document_id).unwrap(),
            "Riassunto pagina 1."
        );

        // Overwrite on the next page.
        set_rolling_summary(&c, doc.document_id, "Riassunto pagine 1-2.").unwrap();
        assert_eq!(
            get_rolling_summary(&c, doc.document_id).unwrap(),
            "Riassunto pagine 1-2."
        );
    }

    #[test]
    fn set_rolling_summary_without_session_is_noop() {
        let c = conn();
        let dir = unique_dir("nosession");
        let path = dir.join("ns.pdf");
        write_file(&path, b"%PDF no session");
        let doc = register_document(&c, &path.to_string_lossy(), 5, "ns").unwrap();
        // Must not error even with no session row.
        set_rolling_summary(&c, doc.document_id, "ignored").unwrap();
        assert_eq!(get_rolling_summary(&c, doc.document_id).unwrap(), "");
    }

    #[test]
    fn update_session_persists_page_and_language() {
        let c = conn();
        let dir = unique_dir("update");
        let path = dir.join("u.pdf");
        write_file(&path, b"%PDF update test");
        let doc = register_document(&c, &path.to_string_lossy(), 20, "u").unwrap();
        let s = open_or_create_session(&c, doc.document_id).unwrap();

        update_session(&c, s.session_id, 7, "en").unwrap();

        let reloaded = open_or_create_session(&c, doc.document_id).unwrap();
        assert_eq!(reloaded.current_page, 7);
        assert_eq!(reloaded.target_language, "en");
        assert_eq!(reloaded.session_id, s.session_id);
    }

    /// Register a document from freshly-written bytes; returns its id.
    fn register(c: &Connection, dir: &Path, name: &str, pages: i64) -> i64 {
        let path = dir.join(name);
        // Distinct bytes per file so partial hashes differ.
        write_file(&path, format!("%PDF content for {name}").as_bytes());
        register_document(c, &path.to_string_lossy(), pages, name)
            .unwrap()
            .document_id
    }

    #[test]
    fn update_session_bumps_updated_at() {
        let c = conn();
        let dir = unique_dir("bump");
        let doc = register(&c, &dir, "b.pdf", 10);
        let s = open_or_create_session(&c, doc).unwrap();

        // Force a known-old timestamp, then update and confirm it moved.
        c.execute(
            "UPDATE sessions SET updated_at = '2000-01-01T00:00:00Z' WHERE id = ?1",
            params![s.session_id],
        )
        .unwrap();

        update_session(&c, s.session_id, 4, "fr").unwrap();

        let (page, updated): (i64, String) = c
            .query_row(
                "SELECT current_page, updated_at FROM sessions WHERE id = ?1",
                params![s.session_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(page, 4, "current_page persisted");
        assert_ne!(updated, "2000-01-01T00:00:00Z", "updated_at bumped");
    }

    #[test]
    fn get_last_session_returns_none_when_empty_then_most_recent_with_document() {
        let c = conn();
        assert!(get_last_session(&c).unwrap().is_none(), "nothing read yet");

        let dir = unique_dir("last");
        let doc_a = register(&c, &dir, "a.pdf", 12);
        let doc_b = register(&c, &dir, "second.pdf", 30);
        let sa = open_or_create_session(&c, doc_a).unwrap();
        let sb = open_or_create_session(&c, doc_b).unwrap();

        // Make A the most recently updated even though B's session is newer.
        c.execute(
            "UPDATE sessions SET updated_at = '2020-01-01T00:00:00Z' WHERE id = ?1",
            params![sb.session_id],
        )
        .unwrap();
        update_session(&c, sa.session_id, 5, "de").unwrap();

        let last = get_last_session(&c).unwrap().expect("a last session exists");
        assert_eq!(last.session_id, sa.session_id, "most-recently-updated wins");
        assert_eq!(last.document_id, doc_a);
        assert_eq!(last.current_page, 5);
        assert_eq!(last.target_language, "de");
        assert_eq!(last.title, "a.pdf", "joined document info present");
        assert_eq!(last.total_pages, 12);
        assert!(!last.file_path.is_empty());
        assert_eq!(last.file_hash.len(), 64, "partial hash carried through");
    }

    #[test]
    fn list_recent_documents_is_ordered_desc_and_limited() {
        let c = conn();
        let dir = unique_dir("recents");
        let d1 = register(&c, &dir, "one.pdf", 1);
        let d2 = register(&c, &dir, "two.pdf", 2);
        let d3 = register(&c, &dir, "three.pdf", 3);

        // Controlled, distinct timestamps: d2 newest, then d3, then d1.
        for (id, ts) in [
            (d1, "2021-01-01T00:00:00Z"),
            (d3, "2022-06-06T00:00:00Z"),
            (d2, "2023-12-31T00:00:00Z"),
        ] {
            c.execute(
                "UPDATE documents SET last_opened_at = ?1 WHERE id = ?2",
                params![ts, id],
            )
            .unwrap();
        }

        let all = list_recent_documents(&c, 10).unwrap();
        let ids: Vec<i64> = all.iter().map(|r| r.document_id).collect();
        assert_eq!(ids, vec![d2, d3, d1], "ordered by last_opened_at desc");

        let limited = list_recent_documents(&c, 2).unwrap();
        assert_eq!(limited.len(), 2, "limit respected");
        assert_eq!(limited[0].document_id, d2);
        assert_eq!(limited[1].document_id, d3);
    }

    #[test]
    fn file_exists_reports_present_and_missing_without_panic() {
        let dir = unique_dir("exists");
        let present = dir.join("here.pdf");
        write_file(&present, b"%PDF here");
        assert!(file_exists(&present.to_string_lossy()));

        let missing = dir.join("gone.pdf");
        // EC06 missing branch: typed `false`, no panic.
        assert!(!file_exists(&missing.to_string_lossy()));
    }

    #[test]
    fn remove_recent_hides_document_but_keeps_the_row() {
        let c = conn();
        let dir = unique_dir("remove");
        let doc = register(&c, &dir, "keep.pdf", 8);

        assert_eq!(list_recent_documents(&c, 10).unwrap().len(), 1);

        remove_recent(&c, doc).unwrap();
        assert!(
            list_recent_documents(&c, 10).unwrap().is_empty(),
            "dropped from recents"
        );

        // Data is preserved: the documents row still exists.
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM documents WHERE id = ?1", params![doc], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "row not deleted (user may re-open later)");
    }

    #[test]
    fn relocate_document_rematches_by_hash_and_updates_path() {
        let c = conn();
        let dir = unique_dir("relocate");
        let original = dir.join("orig.pdf");
        let bytes = b"%PDF the very same bytes for relocation".to_vec();
        write_file(&original, &bytes);
        let doc = register_document(&c, &original.to_string_lossy(), 9, "orig").unwrap();

        // Same bytes at a new location (the user "located" the moved file).
        let moved = dir.join("subdir-moved.pdf");
        write_file(&moved, &bytes);
        let relocated = relocate_document(&c, doc.document_id, &moved.to_string_lossy())
            .unwrap()
            .expect("hash matches -> row updated");
        assert_eq!(relocated.document_id, doc.document_id);
        assert_eq!(relocated.file_path, moved.to_string_lossy());

        // A genuinely different file must NOT be accepted.
        let wrong = dir.join("wrong.pdf");
        write_file(&wrong, b"%PDF totally different content");
        assert!(
            relocate_document(&c, doc.document_id, &wrong.to_string_lossy())
                .unwrap()
                .is_none(),
            "mismatched hash rejected"
        );

        // Unknown document id -> None, no panic.
        assert!(relocate_document(&c, 99999, &moved.to_string_lossy())
            .unwrap()
            .is_none());
    }
}

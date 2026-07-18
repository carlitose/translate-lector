//! SQLite storage for translate-lector.
//!
//! Schema mirrors SPECIFICATION.md §4.3. A single `.db` file holds the PDF
//! history, per-document reading sessions, the per-page translation cache,
//! the dynamic glossary and a global key/value settings table.

use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

/// Create every table required by SPECIFICATION.md §4.3 if it does not exist.
///
/// Idempotent: safe to call on every startup.
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        -- Known PDFs (history)
        CREATE TABLE IF NOT EXISTS documents (
            id             INTEGER PRIMARY KEY,
            file_path      TEXT,
            file_hash      TEXT,
            title          TEXT,
            total_pages    INTEGER,
            last_opened_at TEXT
        );

        -- Reading state per document
        CREATE TABLE IF NOT EXISTS sessions (
            id              INTEGER PRIMARY KEY,
            document_id     INTEGER REFERENCES documents(id),
            target_language TEXT,
            current_page    INTEGER,
            scroll_position REAL,
            rolling_summary TEXT,
            updated_at      TEXT
        );

        -- Per-page translations (cache)
        CREATE TABLE IF NOT EXISTS translations_cache (
            id               INTEGER PRIMARY KEY,
            document_id      INTEGER REFERENCES documents(id),
            page_number      INTEGER,
            target_language  TEXT,
            source_text      TEXT,
            translated_text  TEXT,
            created_at       TEXT,
            -- Two-phase arrival (ticket 01, prefetched-page-arrival-latency): the
            -- perceptor-update no longer runs on the response path of
            -- `translate_page`; it advances the context in a separate
            -- `advance_context` phase. This marker records whether the context
            -- (rolling summary + glossary) has been advanced for this page yet, so
            -- the perceptor runs EXACTLY ONCE per page on the first real
            -- navigation and never again on a re-visit (page-hit), while a FAILED
            -- perceptor leaves it 0 so a later re-visit retries. Set to 0 by the
            -- view-path page write and flipped to 1 only on a FULL perceptor
            -- success. DEFAULT 1 so pre-existing rows (all written by complete
            -- pre-two-phase navigations) are treated as already-advanced.
            context_advanced INTEGER NOT NULL DEFAULT 1,
            UNIQUE(document_id, page_number, target_language)
        );

        -- Per-unit translations (cache di ripresa, ticket 09).
        -- Granularità di unità (paragrafo/frase) entro una pagina: consente di
        -- riprendere una pagina interrotta a metà senza ritradurre le unità già
        -- fatte, e di invalidare per singola unità quando il testo cambia
        -- (`source_hash`). Tabella separata da `translations_cache` per non
        -- sovraccaricare la riga di pagina: quella resta il segnale "pagina fatta"
        -- (percettore avanzato), questa è il livello intermedio/di ripresa.
        CREATE TABLE IF NOT EXISTS unit_translations (
            id              INTEGER PRIMARY KEY,
            document_id     INTEGER REFERENCES documents(id),
            page_number     INTEGER,
            unit_index      INTEGER,
            target_language TEXT,
            source_hash     TEXT,
            translated_text TEXT,
            created_at      TEXT,
            UNIQUE(document_id, page_number, unit_index, target_language)
        );

        -- Glossary terms per document
        CREATE TABLE IF NOT EXISTS glossary (
            id              INTEGER PRIMARY KEY,
            document_id     INTEGER REFERENCES documents(id),
            source_term     TEXT,
            translation     TEXT,
            type            TEXT,
            locked          INTEGER,
            note            TEXT,
            first_seen_page INTEGER
        );

        -- Global key/value configuration
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT
        );
        "#,
    )?;

    // Two-phase arrival (ticket 01): add the `context_advanced` marker column to
    // pre-existing `translations_cache` tables. Idempotent — gated on the column's
    // presence via `PRAGMA table_info` so steady-state opens do no write. Existing
    // rows default to 1 ("already advanced"). This is a deliberate tradeoff, not a
    // claim that every legacy row actually advanced its context: legacy rows are
    // already-read pages, and marking them advanced avoids re-running the perceptor
    // across the whole back-catalogue on upgrade. The rare exception — a page whose
    // only cache row came from a pre-Option-B prefetch that never got a real
    // navigation — will not grow the glossary on the next visit; acceptable given it
    // is a one-off legacy edge and the perceptor resumes normally on new pages.
    let has_context_advanced: bool = {
        let mut stmt = conn.prepare("PRAGMA table_info(translations_cache)")?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
        let mut found = false;
        for col in cols {
            if col? == "context_advanced" {
                found = true;
                break;
            }
        }
        found
    };
    if !has_context_advanced {
        conn.execute_batch(
            "ALTER TABLE translations_cache
                 ADD COLUMN context_advanced INTEGER NOT NULL DEFAULT 1;",
        )?;
    }

    // Glossary dedup (ticket 04). The dedup key is `(document_id,
    // lower(trim(source_term)))` — note `lower()` is SQLite's ASCII-only
    // casefold, so this narrows the previous Rust full-Unicode `to_lowercase()`;
    // the same normalisation is used by every writer/probe (see `glossary.rs`) so
    // they can never disagree. Before this migration, dedup was a non-atomic
    // SELECT-then-INSERT with no schema constraint, so two concurrent writers
    // (manual add + background perceptor) could both pass the existence check
    // and create duplicate rows.
    //
    // Gate the one-time reconciliation on the index's presence. Once
    // `ux_glossary_dedup` exists new duplicates are impossible, so reconciling
    // again is a provable no-op; skipping it keeps steady-state opens (from ~24
    // Tauri command sites) write-free. If the index is ABSENT we must reconcile
    // any pre-existing duplicates FIRST (the UNIQUE index cannot be built on
    // dirty data), then create it. If PRESENT, both steps are skipped.
    let index_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master
              WHERE type = 'index' AND name = 'ux_glossary_dedup'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !index_exists {
        reconcile_glossary_duplicates(conn)?;
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_glossary_dedup
                 ON glossary(document_id, lower(trim(source_term)));",
        )?;
    }
    Ok(())
}

/// Collapse pre-existing duplicate glossary rows to one row per dedup key
/// `(document_id, lower(trim(source_term)))`, deterministically and idempotently.
///
/// Survivor selection per group: **prefer a locked row** (a locked term is never
/// lost), then the **lowest `id`**. Every other row in the group is deleted. On a
/// database with no duplicates this deletes nothing, so it is safe to run on
/// every startup before (re)creating [`ux_glossary_dedup`]. Returns how many rows
/// were removed.
pub(crate) fn reconcile_glossary_duplicates(conn: &Connection) -> rusqlite::Result<usize> {
    // Delete row `g` when a strictly "better" survivor `keeper` exists in the same
    // group. `keeper` is a better survivor than `g` when:
    //   - keeper is locked and g is not, OR
    //   - they share the same locked-ness and keeper has the lower id.
    // The single best row in each group has no better peer, so it survives.
    //
    // Worked example — group {id 10 unlocked, id 11 locked, id 12 unlocked}:
    // keeper=11 beats both 10 and 12 (locked>unlocked), so 10 and 12 are deleted
    // and the locked id 11 survives even though it is not the lowest id.
    let removed = conn.execute(
        "DELETE FROM glossary AS g
          WHERE EXISTS (
              SELECT 1 FROM glossary AS keeper
               WHERE keeper.document_id = g.document_id
                 AND lower(trim(keeper.source_term)) = lower(trim(g.source_term))
                 AND keeper.id <> g.id
                 AND (
                     -- keeper locked, g unlocked
                     (COALESCE(keeper.locked, 0) <> 0 AND COALESCE(g.locked, 0) = 0)
                     -- same locked-ness, keeper wins by lower id
                     OR (
                         (COALESCE(keeper.locked, 0) <> 0) = (COALESCE(g.locked, 0) <> 0)
                         AND keeper.id < g.id
                     )
                 )
          )",
        [],
    )?;
    Ok(removed)
}

/// Empty the translation cache (§3.5 "Svuota cache", ticket 13).
///
/// Deletes every row from `translations_cache` **and** from the per-unit resume
/// cache `unit_translations` (ticket 09): svuotare solo la cache di pagina
/// lascerebbe il livello di ripresa a servire vecchie unità, vanificando lo
/// svuotamento. Documents, sessions, glossary e settings restano intatti. Ritorna
/// il numero di righe di **pagina** rimosse (semantica invariata per i chiamanti).
pub fn clear_translations_cache(conn: &Connection) -> rusqlite::Result<usize> {
    let removed = conn.execute("DELETE FROM translations_cache", [])?;
    conn.execute("DELETE FROM unit_translations", [])?;
    Ok(removed)
}

/// Open (creating if needed) the database at `path` and initialise the schema.
pub fn open_and_init<P: AsRef<Path>>(path: P) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    init_schema(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn init_schema_creates_the_five_tables() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let tables = table_names(&conn);
        for expected in [
            "documents",
            "sessions",
            "translations_cache",
            "unit_translations",
            "glossary",
            "settings",
        ] {
            assert!(
                tables.iter().any(|t| t == expected),
                "missing table `{expected}`; found: {tables:?}"
            );
        }
    }

    /// Ticket 09: la migrazione crea la tabella di cache per-unità con la sua
    /// UNIQUE key `(document_id, page_number, unit_index, target_language)`.
    #[test]
    fn init_schema_creates_the_unit_translations_table_with_unique_key() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        assert!(table_names(&conn).iter().any(|t| t == "unit_translations"));

        conn.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/x.pdf', 'h', 'Doc', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO unit_translations
                (document_id, page_number, unit_index, target_language, source_hash, translated_text)
             VALUES (1, 2, 0, 'it', 'abc', 'ciao')",
            [],
        )
        .unwrap();
        // Same key again must violate the UNIQUE constraint.
        let dup = conn.execute(
            "INSERT INTO unit_translations
                (document_id, page_number, unit_index, target_language, source_hash, translated_text)
             VALUES (1, 2, 0, 'it', 'xyz', 'salve')",
            [],
        );
        assert!(dup.is_err(), "duplicate (doc,page,unit_index,lang) must be rejected");
        // A different unit_index for the same page is a distinct row.
        conn.execute(
            "INSERT INTO unit_translations
                (document_id, page_number, unit_index, target_language, source_hash, translated_text)
             VALUES (1, 2, 1, 'it', 'def', 'mondo')",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM unit_translations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn init_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        // Second call must not fail.
        init_schema(&conn).unwrap();
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn clear_translations_cache_empties_only_the_cache() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        // Seed one row in every table (documents first for the FKs).
        conn.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/x.pdf', 'hash', 'Doc', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (document_id, target_language, current_page)
             VALUES (1, 'it', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO translations_cache
                (document_id, page_number, target_language, source_text, translated_text)
             VALUES (1, 1, 'it', 'hello', 'ciao')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO unit_translations
                (document_id, page_number, unit_index, target_language, source_hash, translated_text)
             VALUES (1, 1, 0, 'it', 'abc', 'ciao')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO glossary (document_id, source_term, translation, type, locked)
             VALUES (1, 'board', 'consiglio', 'tecnico', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('model', 'openai/gpt-4o')",
            [],
        )
        .unwrap();

        let removed = clear_translations_cache(&conn).unwrap();

        assert_eq!(removed, 1, "one page cache row removed (return = page rows)");
        assert_eq!(count(&conn, "translations_cache"), 0, "page cache emptied");
        assert_eq!(count(&conn, "unit_translations"), 0, "per-unit resume cache emptied too");
        assert_eq!(count(&conn, "documents"), 1, "documents kept");
        assert_eq!(count(&conn, "sessions"), 1, "sessions kept");
        assert_eq!(count(&conn, "glossary"), 1, "glossary kept");
        assert_eq!(count(&conn, "settings"), 1, "settings kept");
    }

    #[test]
    fn clear_translations_cache_on_empty_cache_is_a_noop() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        assert_eq!(clear_translations_cache(&conn).unwrap(), 0);
    }

    // --- Ticket 04: atomic glossary dedup (UNIQUE index + reconciliation) ----

    /// The UNIQUE index normalises the dedup key per document, so a second row
    /// with the same `source_term` (case-insensitive + trim) is rejected by the
    /// storage layer — no non-atomic SELECT-then-INSERT can create a duplicate.
    #[test]
    fn glossary_dedup_index_rejects_duplicate_source_term_case_insensitive() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/x.pdf', 'h', 'Doc', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO glossary (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'Board', 'consiglio', 't', 0, '', 1)",
            [],
        )
        .unwrap();
        // Same key, different case and surrounding whitespace -> rejected.
        let dup = conn.execute(
            "INSERT INTO glossary (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, '  board ', 'x', 't', 0, '', 2)",
            [],
        );
        assert!(dup.is_err(), "UNIQUE index must reject a case/space-insensitive duplicate");
        // The same term in a different document is a distinct key.
        conn.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (2, '/y.pdf', 'h', 'Doc2', 3)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO glossary (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (2, 'board', 'x', 't', 0, '', 1)",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM glossary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2, "the index is scoped per document");
    }

    /// Pre-existing duplicates (created before the index existed) are reconciled
    /// by the migration: exactly one row survives per key and a locked duplicate
    /// is always the survivor — a locked term is never lost.
    #[test]
    fn migration_reconciles_preexisting_duplicates_keeping_locked() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        // Drop the index to reproduce the dirty pre-migration state, then seed
        // duplicate rows in mixed case (the locked one is not the lowest id).
        conn.execute("DROP INDEX IF EXISTS ux_glossary_dedup", []).unwrap();
        conn.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/x.pdf', 'h', 'Doc', 3)",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO glossary (id, document_id, source_term, translation, type, locked, note, first_seen_page) VALUES (10, 1, 'Board', 'a', 't', 0, '', 1);
             INSERT INTO glossary (id, document_id, source_term, translation, type, locked, note, first_seen_page) VALUES (11, 1, 'board', 'LOCKED-KEEP', 't', 1, 'nota', 5);
             INSERT INTO glossary (id, document_id, source_term, translation, type, locked, note, first_seen_page) VALUES (12, 1, ' BOARD ', 'c', 't', 0, '', 9);
             INSERT INTO glossary (id, document_id, source_term, translation, type, locked, note, first_seen_page) VALUES (13, 1, 'ceo', 'ad', 't', 0, '', 2);",
        )
        .unwrap();

        // Run the migration: it must reconcile the dirty data and (re)create the
        // index without failing.
        init_schema(&conn).unwrap();

        let rows: Vec<(i64, i64, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, locked, translation FROM glossary WHERE lower(trim(source_term)) = 'board'")
                .unwrap();
            let r = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap();
            r.map(|x| x.unwrap()).collect()
        };
        assert_eq!(rows.len(), 1, "one survivor per dedup key");
        assert_eq!(rows[0].0, 11, "the locked row is kept even if it is not the lowest id");
        assert_eq!(rows[0].1, 1, "survivor is locked (locked term never lost)");
        assert_eq!(rows[0].2, "LOCKED-KEEP", "the locked row's data is preserved");

        let ceo: i64 = conn
            .query_row("SELECT COUNT(*) FROM glossary WHERE source_term = 'ceo'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ceo, 1, "an unrelated non-duplicated term is untouched");

        // The index is now in place and enforcing uniqueness.
        let dup = conn.execute(
            "INSERT INTO glossary (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'BOARD', 'z', 't', 0, '', 1)",
            [],
        );
        assert!(dup.is_err(), "index recreated and enforcing after reconcile");
    }

    /// Reconciliation is idempotent / safe when there are no duplicates: repeated
    /// migration runs never delete a legitimate row.
    #[test]
    fn migration_reconcile_is_noop_without_duplicates() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/x.pdf', 'h', 'Doc', 3)",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO glossary (document_id, source_term, translation, type, locked, note, first_seen_page) VALUES (1, 'board', 'a', 't', 1, '', 1);
             INSERT INTO glossary (document_id, source_term, translation, type, locked, note, first_seen_page) VALUES (1, 'ceo', 'b', 't', 0, '', 2);",
        )
        .unwrap();
        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM glossary", [], |r| r.get(0))
            .unwrap();

        // Re-running the migration must not delete anything on clean data.
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();

        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM glossary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after, "reconcile is a no-op without duplicates");
        assert_eq!(after, 2);
    }

    #[test]
    fn open_and_init_creates_a_db_file() {
        let dir = std::env::temp_dir().join(format!("tl-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("translate-lector.db");
        let _ = std::fs::remove_file(&db_path);

        let conn = open_and_init(&db_path).unwrap();
        assert!(db_path.exists());
        assert_eq!(table_names(&conn).len(), 6);
    }
}

//! SQLite storage for translate-lector.
//!
//! Schema mirrors SPECIFICATION.md §4.3. A single `.db` file holds the PDF
//! history, per-document reading sessions, the per-page translation cache,
//! the dynamic glossary and a global key/value settings table.

use rusqlite::Connection;
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
            id              INTEGER PRIMARY KEY,
            document_id     INTEGER REFERENCES documents(id),
            page_number     INTEGER,
            target_language TEXT,
            source_text     TEXT,
            translated_text TEXT,
            created_at      TEXT,
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
    )
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

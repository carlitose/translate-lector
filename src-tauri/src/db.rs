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

/// Empty the per-page translation cache (§3.5 "Svuota cache", ticket 13).
///
/// Deletes every row from `translations_cache` and nothing else — documents,
/// sessions, glossary and settings are left intact. Returns the row count removed.
pub fn clear_translations_cache(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM translations_cache", [])
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
            "glossary",
            "settings",
        ] {
            assert!(
                tables.iter().any(|t| t == expected),
                "missing table `{expected}`; found: {tables:?}"
            );
        }
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

        assert_eq!(removed, 1, "one cache row removed");
        assert_eq!(count(&conn, "translations_cache"), 0, "cache emptied");
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
        assert_eq!(table_names(&conn).len(), 5);
    }
}

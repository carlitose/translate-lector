//! Dynamic glossary population and rendering (SPECIFICATION §3.3, §4.3, §4.4,
//! ticket 09).
//!
//! The percettore proposes `new_glossary_terms[]` per page; this module inserts
//! them into the `glossary` table with `locked = 0` and `first_seen_page` set to
//! the current page, **deduped** against terms already stored for the document
//! (by `source_term`, case-insensitive) and **never** altering existing rows —
//! so user-locked terms (ticket 10) are preserved untouched.
//!
//! It also renders the stored glossary into the locked / unlocked line blocks
//! consumed by the prompt builder (`llm::build_user_prompt`).

use crate::llm::GlossaryTerm;
use rusqlite::{params, Connection};
use serde::Serialize;

/// A stored glossary row (§4.3) as needed to build the prompt context and to
/// render the glossary panel (ticket 10). `term_type` serialises as `term_type`
/// for the frontend.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlossaryEntry {
    pub id: i64,
    pub source_term: String,
    pub translation: String,
    pub term_type: String,
    pub locked: bool,
    pub note: String,
    pub first_seen_page: i64,
}

/// Load every glossary entry for a document, ordered by first appearance.
pub fn list_glossary(conn: &Connection, document_id: i64) -> rusqlite::Result<Vec<GlossaryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_term, translation, type, locked, note, first_seen_page
           FROM glossary
          WHERE document_id = ?1
          ORDER BY first_seen_page, id",
    )?;
    let rows = stmt.query_map(params![document_id], |r| {
        Ok(GlossaryEntry {
            id: r.get(0)?,
            source_term: r.get(1)?,
            translation: r.get(2)?,
            term_type: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            locked: r.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,
            note: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            first_seen_page: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
        })
    })?;
    rows.collect()
}

/// Apply the user's edits to a single glossary row (ticket 10, UC03): set its
/// `translation`, `note` and `locked` flag in place. Only ever updates the row
/// with the given `id`, so it can never create a duplicate. `source_term`,
/// `type` and `first_seen_page` are left untouched.
pub fn update_glossary_term(
    conn: &Connection,
    id: i64,
    translation: &str,
    note: &str,
    locked: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE glossary
            SET translation = ?2, note = ?3, locked = ?4
          WHERE id = ?1",
        params![id, translation, note, locked as i64],
    )?;
    Ok(())
}

/// Render a slice of entries as prompt lines: `source_term => translation
/// [type] (note)` (the `(note)` suffix is omitted when the note is empty).
/// Returns an empty string when there are no entries (the prompt builder then
/// substitutes its `(nessuno)` placeholder).
pub fn render_terms(entries: &[&GlossaryEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let note = if e.note.trim().is_empty() {
                String::new()
            } else {
                format!(" ({})", e.note.trim())
            };
            format!("{} => {}  [{}]{}", e.source_term, e.translation, e.term_type, note)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The locked (absolute-constraint) and unlocked (suggested) line blocks for the
/// document's glossary, ready to pass to `llm::build_user_prompt`.
pub fn render_locked_unlocked(entries: &[GlossaryEntry]) -> (String, String) {
    let locked: Vec<&GlossaryEntry> = entries.iter().filter(|e| e.locked).collect();
    let unlocked: Vec<&GlossaryEntry> = entries.iter().filter(|e| !e.locked).collect();
    (render_terms(&locked), render_terms(&unlocked))
}

/// Insert `terms` for a document with `locked = 0` and `first_seen_page = page`,
/// skipping any whose `source_term` already exists for the document
/// (case-insensitive) and any duplicated within the incoming batch. Existing
/// rows — including user-locked ones — are never modified. Returns how many new
/// rows were inserted.
pub fn insert_terms_deduped(
    conn: &Connection,
    document_id: i64,
    terms: &[GlossaryTerm],
    page: i64,
) -> rusqlite::Result<usize> {
    // Existing source_terms for this document, lowercased.
    let mut existing: std::collections::HashSet<String> = {
        let mut stmt =
            conn.prepare("SELECT source_term FROM glossary WHERE document_id = ?1")?;
        let rows = stmt.query_map(params![document_id], |r| r.get::<_, String>(0))?;
        rows.filter_map(Result::ok)
            .map(|s| s.trim().to_lowercase())
            .collect()
    };

    let mut inserted = 0usize;
    for term in terms {
        let key = term.source_term.trim().to_lowercase();
        if key.is_empty() || existing.contains(&key) {
            continue; // dedup against DB and within the batch
        }
        conn.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![
                document_id,
                term.source_term,
                term.translation,
                term.term_type,
                term.note,
                page
            ],
        )?;
        existing.insert(key);
        inserted += 1;
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::init_schema(&c).unwrap();
        c.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/tmp/x.pdf', 'hash', 'x', 10)",
            [],
        )
        .unwrap();
        c
    }

    fn term(source: &str, translation: &str) -> GlossaryTerm {
        GlossaryTerm {
            source_term: source.into(),
            translation: translation.into(),
            term_type: "comune".into(),
            note: String::new(),
        }
    }

    #[test]
    fn inserts_new_terms_unlocked_with_first_seen_page() {
        let c = conn();
        let n = insert_terms_deduped(&c, 1, &[term("board", "consiglio")], 12).unwrap();
        assert_eq!(n, 1);

        let e = &list_glossary(&c, 1).unwrap()[0];
        assert_eq!(e.source_term, "board");
        assert_eq!(e.translation, "consiglio");
        assert!(!e.locked, "new terms are inserted unlocked");
        assert_eq!(e.first_seen_page, 12);
    }

    #[test]
    fn dedups_against_existing_case_insensitive_and_within_batch() {
        let c = conn();
        insert_terms_deduped(&c, 1, &[term("Board", "consiglio")], 1).unwrap();

        // "board" duplicates the existing "Board"; the two "CEO" duplicate each
        // other within the batch -> only one "CEO" is added.
        let n = insert_terms_deduped(
            &c,
            1,
            &[term("board", "altra"), term("CEO", "ad"), term("ceo", "amministratore")],
            5,
        )
        .unwrap();
        assert_eq!(n, 1, "only CEO is new");

        let entries = list_glossary(&c, 1).unwrap();
        assert_eq!(entries.len(), 2);
        // The original translation is untouched by the duplicate attempt.
        let board = entries.iter().find(|e| e.source_term == "Board").unwrap();
        assert_eq!(board.translation, "consiglio");
    }

    #[test]
    fn never_modifies_existing_locked_terms() {
        let c = conn();
        // A user-locked term (ticket 10 would create these).
        c.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'board', 'CONSIGLIO-BLOCCATO', 'tecnico', 1, 'nota', 3)",
            [],
        )
        .unwrap();

        // Model proposes the same term with a different translation -> ignored.
        let n = insert_terms_deduped(&c, 1, &[term("board", "consiglio-nuovo")], 9).unwrap();
        assert_eq!(n, 0, "locked term is not duplicated");

        let e = &list_glossary(&c, 1).unwrap()[0];
        assert!(e.locked, "still locked");
        assert_eq!(e.translation, "CONSIGLIO-BLOCCATO", "translation preserved");
        assert_eq!(e.first_seen_page, 3, "first_seen_page preserved");
    }

    /// Read the raw `locked`/`translation`/`note` columns for a source term.
    fn row(c: &Connection, source: &str) -> (i64, String, String, i64) {
        c.query_row(
            "SELECT id, translation, note, locked FROM glossary WHERE source_term = ?1",
            params![source],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    #[test]
    fn list_returns_id_and_all_fields() {
        let c = conn();
        c.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'board', 'consiglio', 'tecnico', 1, 'nota', 7)",
            [],
        )
        .unwrap();
        let e = &list_glossary(&c, 1).unwrap()[0];
        assert!(e.id > 0, "id is surfaced");
        assert_eq!(e.source_term, "board");
        assert_eq!(e.translation, "consiglio");
        assert_eq!(e.term_type, "tecnico");
        assert!(e.locked);
        assert_eq!(e.note, "nota");
        assert_eq!(e.first_seen_page, 7);
    }

    #[test]
    fn update_persists_translation_note_and_locked() {
        let c = conn();
        insert_terms_deduped(&c, 1, &[term("board", "consiglio")], 3).unwrap();
        let id = list_glossary(&c, 1).unwrap()[0].id;

        update_glossary_term(&c, id, "consiglio di amministrazione", "vincolante", true).unwrap();

        let e = &list_glossary(&c, 1).unwrap()[0];
        assert_eq!(e.translation, "consiglio di amministrazione");
        assert_eq!(e.note, "vincolante");
        assert!(e.locked);
        // Fields not part of the update are preserved.
        assert_eq!(e.source_term, "board");
        assert_eq!(e.first_seen_page, 3);
    }

    #[test]
    fn toggling_locked_persists_both_ways() {
        let c = conn();
        insert_terms_deduped(&c, 1, &[term("board", "consiglio")], 1).unwrap();
        let id = list_glossary(&c, 1).unwrap()[0].id;
        assert!(!list_glossary(&c, 1).unwrap()[0].locked);

        update_glossary_term(&c, id, "consiglio", "", true).unwrap();
        assert!(list_glossary(&c, 1).unwrap()[0].locked, "locked on");

        update_glossary_term(&c, id, "consiglio", "", false).unwrap();
        assert!(!list_glossary(&c, 1).unwrap()[0].locked, "locked off");
    }

    #[test]
    fn update_does_not_create_duplicate_rows() {
        let c = conn();
        insert_terms_deduped(&c, 1, &[term("board", "consiglio")], 1).unwrap();
        let id = list_glossary(&c, 1).unwrap()[0].id;

        update_glossary_term(&c, id, "nuovo", "n", true).unwrap();
        update_glossary_term(&c, id, "ancora", "m", false).unwrap();

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM glossary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "an update must never insert a row");
    }

    #[test]
    fn freshly_locked_term_is_rendered_as_absolute_constraint() {
        // Integration with the percettore (ticket 09): after the user locks a
        // term, `render_locked_unlocked` must place it in the absolute block.
        let c = conn();
        insert_terms_deduped(&c, 1, &[term("board", "consiglio")], 1).unwrap();
        let id = list_glossary(&c, 1).unwrap()[0].id;

        // Before locking it is a mere suggestion.
        let (locked_before, unlocked_before) =
            render_locked_unlocked(&list_glossary(&c, 1).unwrap());
        assert!(!locked_before.contains("board"));
        assert!(unlocked_before.contains("board => consiglio"));

        update_glossary_term(&c, id, "consiglio di amministrazione", "", true).unwrap();

        let (locked_after, unlocked_after) =
            render_locked_unlocked(&list_glossary(&c, 1).unwrap());
        assert!(
            locked_after.contains("board => consiglio di amministrazione"),
            "the freshly-locked, edited term is now an absolute constraint"
        );
        assert!(!unlocked_after.contains("board"));
    }

    // Silence the unused-helper warning when only some tests use `row`.
    #[test]
    fn row_helper_reads_columns() {
        let c = conn();
        insert_terms_deduped(&c, 1, &[term("board", "consiglio")], 1).unwrap();
        let (id, translation, note, locked) = row(&c, "board");
        assert!(id > 0);
        assert_eq!(translation, "consiglio");
        assert_eq!(note, "");
        assert_eq!(locked, 0);
    }

    #[test]
    fn render_splits_locked_as_absolute_and_unlocked_as_suggested() {
        let c = conn();
        c.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'board', 'consiglio', 'tecnico', 1, 'contesto', 3)",
            [],
        )
        .unwrap();
        insert_terms_deduped(&c, 1, &[term("CEO", "amministratrice delegata")], 3).unwrap();

        let entries = list_glossary(&c, 1).unwrap();
        let (locked, unlocked) = render_locked_unlocked(&entries);
        assert!(locked.contains("board => consiglio  [tecnico] (contesto)"));
        assert!(!locked.contains("CEO"));
        assert!(unlocked.contains("CEO => amministratrice delegata  [comune]"));
        assert!(!unlocked.contains("board"));
    }
}

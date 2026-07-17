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

// --- Selezione deterministica del glossario (ticket 03) ----------------------

/// Selezione **deterministica** (nessun LLM) dei termini di glossario rilevanti
/// per una singola unità di traduzione (SPECIFICATION / wayfinder
/// small-context, idea-chiave utente). Restituisce solo le `entries` il cui
/// `source_term` compare in `unit_text`, tagliando drasticamente il prompt-
/// glossario rispetto a "invia tutto il glossario".
///
/// Regole di match (case-insensitive, su **confini di parola**):
/// - il testo e i termini sono tokenizzati in parole (sequenze di caratteri
///   alfanumerici Unicode); un termine **multiword** matcha come sotto-sequenza
///   contigua di token, così "art" NON matcha dentro "start" né "consiglio di
///   amministrazione" matcha "consiglio comunale";
/// - **tolleranza morfologica semplice** applicata SOLO all'ultima parola del
///   termine (vedi [`is_plural_variant`]): plurale inglese in `-s`/`-es` e
///   alternanze di vocale finale italiane `-o/-i`, `-a/-e`, `-e/-i`.
///
/// Vincoli: i termini **locked** che matchano sono SEMPRE inclusi (vincolo
/// assoluto preservato). `unlocked_cap` limita opzionalmente il numero di
/// termini **unlocked** restituiti (i primi, in ordine di `entries`); i locked
/// non sono mai scartati dal cap. L'ordine di `entries` è preservato.
///
/// Cablata nel flusso live da STC-08 (`translate::translate_page`): ogni unità di
/// traduzione riceve nel prompt solo il glossario selezionato per quell'unità.
pub fn select_glossary(
    unit_text: &str,
    entries: &[GlossaryEntry],
    unlocked_cap: Option<usize>,
) -> Vec<GlossaryEntry> {
    let text_tokens = tokenize(unit_text);
    let mut unlocked_kept = 0usize;
    let mut selected = Vec::new();
    for e in entries {
        if !term_matches(&text_tokens, &e.source_term) {
            continue;
        }
        if e.locked {
            selected.push(e.clone()); // vincolo assoluto: mai scartato dal cap
        } else {
            if let Some(cap) = unlocked_cap {
                if unlocked_kept >= cap {
                    continue;
                }
            }
            unlocked_kept += 1;
            selected.push(e.clone());
        }
    }
    selected
}

/// Spezza `text` in parole minuscole (token alfanumerici Unicode); la
/// punteggiatura e gli spazi fanno da separatore, garantendo il match su
/// confine di parola.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// `true` se i token di `source_term` compaiono come sotto-sequenza contigua in
/// `text_tokens`: tutte le parole devono coincidere esattamente, tranne
/// l'ultima che tollera una variante di plurale ([`is_plural_variant`]).
fn term_matches(text_tokens: &[String], source_term: &str) -> bool {
    let term_tokens = tokenize(source_term);
    let n = term_tokens.len();
    if n == 0 || text_tokens.len() < n {
        return false;
    }
    for start in 0..=(text_tokens.len() - n) {
        let hit = term_tokens.iter().enumerate().all(|(i, ct)| {
            let tt = &text_tokens[start + i];
            if i == n - 1 {
                is_plural_variant(tt, ct)
            } else {
                tt == ct
            }
        });
        if hit {
            return true;
        }
    }
    false
}

/// Tolleranza morfologica **semplice e bidirezionale** fra due parole già
/// minuscole. Copre:
/// - uguaglianza esatta;
/// - plurale inglese: una parola è l'altra + `s` o + `es`
///   (`board`/`boards`, `box`/`boxes`);
/// - alternanza di vocale finale italiana a parità di lunghezza (stem >= 3):
///   `-o/-i` (titolo/titoli), `-a/-e` (casa/case), `-e/-i` (cane/cani).
///
/// Limiti noti (documentati per il ticket): non copre plurali irregolari
/// (uomo/uomini, città invariabile), plurali con mutazione di consonante
/// (amico/amici, banco/banchi), inflessioni verbali/aggettivali, sinonimi o
/// abbreviazioni. Può produrre falsi positivi fra classi omografe (es. `casi`
/// vs il termine `case` per l'alternanza `-e/-i`): accettabile in un prototipo
/// di selezione, dato che i termini scartati per errore restano rari e i locked
/// non dipendono da questa euristica.
fn is_plural_variant(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    // Plurale inglese: long = short + "s" | "es".
    if long == format!("{short}s") || long == format!("{short}es") {
        return true;
    }
    // Alternanza di vocale finale italiana (stessa lunghezza, radice uguale).
    if short.len() == long.len() && short.chars().count() >= 3 {
        let s: Vec<char> = short.chars().collect();
        let l: Vec<char> = long.chars().collect();
        if s[..s.len() - 1] == l[..l.len() - 1] {
            let (x, y) = (s[s.len() - 1], l[l.len() - 1]);
            let pair = (x.min(y), x.max(y));
            return matches!(pair, ('i', 'o') | ('a', 'e') | ('e', 'i'));
        }
    }
    false
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
    let mut inserted = 0usize;
    for term in terms {
        // Skip blank source terms (they carry no dedup key). Dedup against the DB
        // and within the batch is enforced atomically by the `ux_glossary_dedup`
        // UNIQUE index (ticket 04): `ON CONFLICT DO NOTHING` inserts only when no
        // row with the same `(document_id, lower(trim(source_term)))` exists,
        // never clobbering an existing (locked included) row.
        if term.source_term.trim().is_empty() {
            continue;
        }
        let changed = conn.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
             ON CONFLICT DO NOTHING",
            params![
                document_id,
                term.source_term,
                term.translation,
                term.term_type,
                term.note,
                page
            ],
        )?;
        inserted += changed;
    }
    Ok(inserted)
}

// --- Aggiunta manuale di un termine (ticket 02) ------------------------------

/// Outcome of a *successful* manual add attempt (ticket 02, decision 01 #2).
/// Validation failures are not outcomes — they travel through the `Result`
/// error channel as [`AddTermError::Validation`], mirroring how the rest of the
/// codebase surfaces validation errors (see [`crate::llm::LlmError`]).
///
/// - `Inserted(id)` — a brand-new row was created; `id` is the new row's id.
/// - `Duplicate(id)` — a row with the same `source_term` (case-insensitive)
///   already exists in the document; `id` is the **existing** row's id, so the
///   UI can open it in edit instead of inserting again. Nothing is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    Inserted(i64),
    Duplicate(i64),
}

/// Error from [`add_manual_term`]: either a validation failure (empty/whitespace
/// `source_term`/`translation`) or an underlying database error. The Tauri
/// command maps both to `Err(String)` via `Display`, so the frontend keeps
/// seeing the same Italian validation messages.
#[derive(Debug)]
pub enum AddTermError {
    /// `source_term` or `translation` was empty/whitespace after trimming.
    Validation(&'static str),
    /// Underlying SQLite failure.
    Db(rusqlite::Error),
}

impl std::fmt::Display for AddTermError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddTermError::Validation(reason) => f.write_str(reason),
            AddTermError::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AddTermError {}

impl From<rusqlite::Error> for AddTermError {
    fn from(e: rusqlite::Error) -> Self {
        AddTermError::Db(e)
    }
}

/// Manually add a glossary term for a document (ticket 02). Unlike
/// [`insert_terms_deduped`] (percettore-only, always `locked = 0`), the caller
/// chooses `locked`. `first_seen_page` is stored as `0`, the "manual" marker
/// (decision 01 #3).
///
/// Dedup is enforced at the storage layer by the `ux_glossary_dedup` UNIQUE
/// index on `(document_id, lower(trim(source_term)))` via
/// `INSERT … ON CONFLICT DO NOTHING`: `Inserted` vs `Duplicate` is derived from
/// the conflict result, not a prior SELECT. On a conflict the existing row is
/// fetched (using the same `lower(trim(...))` key) and returned **untouched**
/// (never clobbered, locked rows included) via [`AddOutcome::Duplicate`] — no
/// insert, no update. Note the key uses SQLite's `lower()` (ASCII casefold), a
/// deliberate narrowing of the old Rust full-Unicode `to_lowercase()` so that
/// index, conflict, probe and reconcile share one identical normalisation.
///
/// Core-side validation: `source_term` and `translation` must be non-empty
/// after trim, otherwise [`AddTermError::Validation`] is returned through the
/// error channel with no DB write. Stored text values are all trimmed.
pub fn add_manual_term(
    conn: &Connection,
    document_id: i64,
    source_term: &str,
    translation: &str,
    term_type: &str,
    note: &str,
    locked: bool,
) -> Result<AddOutcome, AddTermError> {
    let source = source_term.trim();
    let trans = translation.trim();
    if source.is_empty() {
        return Err(AddTermError::Validation("source_term non può essere vuoto"));
    }
    if trans.is_empty() {
        return Err(AddTermError::Validation("translation non può essere vuota"));
    }

    // Atomic dedup (ticket 04): attempt the insert with `ON CONFLICT DO NOTHING`
    // against the `ux_glossary_dedup` UNIQUE index on
    // `(document_id, lower(trim(source_term)))`. `Inserted` vs `Duplicate` is
    // derived from the conflict result, not a prior SELECT, so two concurrent
    // writers can never both pass an existence check and create a duplicate row.
    let changed = conn.execute(
        "INSERT INTO glossary
             (document_id, source_term, translation, type, locked, note, first_seen_page)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)
         ON CONFLICT DO NOTHING",
        params![
            document_id,
            source,
            trans,
            term_type.trim(),
            locked as i64,
            note.trim()
        ],
    )?;
    if changed == 1 {
        return Ok(AddOutcome::Inserted(conn.last_insert_rowid()));
    }

    // Conflict: a row with the same normalised key already exists. Never clobber
    // it (locked included) — fetch and return its id so the UI can open it. Both
    // sides of the comparison MUST use the IDENTICAL `lower(trim(...))` — SQLite's
    // ASCII casefold — that the `ux_glossary_dedup` index and `ON CONFLICT` use;
    // binding a Rust `to_lowercase()` here instead disagreed on non-ASCII
    // uppercase (e.g. 'SOCIETÀ') and turned a real duplicate into a spurious
    // QueryReturnedNoRows error. Bind the raw source_term and let SQLite lower it.
    let existing_id: i64 = conn.query_row(
        "SELECT id FROM glossary
         WHERE document_id = ?1 AND lower(trim(source_term)) = lower(trim(?2))
         LIMIT 1",
        params![document_id, source],
        |r| r.get(0),
    )?;
    Ok(AddOutcome::Duplicate(existing_id))
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

    // --- add_manual_term (ticket 02, aggiunta manuale) -----------------------

    #[test]
    fn add_manual_inserts_new_unlocked_term_with_page_zero() {
        let c = conn();
        let out = add_manual_term(&c, 1, "board", "consiglio", "tecnico", "nota", false).unwrap();
        let id = match out {
            AddOutcome::Inserted(id) => id,
            other => panic!("expected Inserted, got {other:?}"),
        };
        assert!(id > 0, "Inserted carries the new row id");

        let e = &list_glossary(&c, 1).unwrap()[0];
        assert_eq!(e.id, id);
        assert_eq!(e.source_term, "board");
        assert_eq!(e.translation, "consiglio");
        assert_eq!(e.term_type, "tecnico");
        assert_eq!(e.note, "nota");
        assert!(!e.locked, "unlocked when caller passes locked = false");
        assert_eq!(e.first_seen_page, 0, "manual terms are marked with page 0");
    }

    #[test]
    fn add_manual_inserts_new_locked_term() {
        let c = conn();
        let out = add_manual_term(&c, 1, "board", "consiglio", "tecnico", "", true).unwrap();
        assert!(matches!(out, AddOutcome::Inserted(_)));

        let e = &list_glossary(&c, 1).unwrap()[0];
        assert!(e.locked, "locked when caller passes locked = true");
    }

    #[test]
    fn add_manual_duplicate_case_insensitive_returns_existing_id_untouched() {
        let c = conn();
        // Pre-existing locked row with its own translation/note/page.
        c.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'Board', 'CONSIGLIO-ESISTENTE', 'tecnico', 1, 'nota-orig', 7)",
            [],
        )
        .unwrap();
        let existing_id = list_glossary(&c, 1).unwrap()[0].id;

        // Different case, different translation/locked -> must NOT clobber.
        let out = add_manual_term(&c, 1, "board", "altra-traduzione", "comune", "altra-nota", false)
            .unwrap();
        assert_eq!(out, AddOutcome::Duplicate(existing_id));

        let entries = list_glossary(&c, 1).unwrap();
        assert_eq!(entries.len(), 1, "no new row inserted on duplicate");
        let e = &entries[0];
        assert_eq!(e.translation, "CONSIGLIO-ESISTENTE", "translation preserved");
        assert!(e.locked, "locked flag preserved");
        assert_eq!(e.note, "nota-orig", "note preserved");
        assert_eq!(e.first_seen_page, 7, "first_seen_page preserved");
    }

    #[test]
    fn add_manual_rejects_empty_source_term_without_inserting() {
        let c = conn();
        let err = add_manual_term(&c, 1, "   ", "consiglio", "tecnico", "", true).unwrap_err();
        assert!(
            matches!(err, AddTermError::Validation(_)),
            "empty source rejected through the error channel"
        );
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM glossary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "nothing inserted on invalid source");
    }

    #[test]
    fn add_manual_rejects_empty_translation_without_inserting() {
        let c = conn();
        let err = add_manual_term(&c, 1, "board", "   ", "tecnico", "", true).unwrap_err();
        assert!(
            matches!(err, AddTermError::Validation(_)),
            "empty translation rejected through the error channel"
        );
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM glossary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "nothing inserted on invalid translation");
    }

    #[test]
    fn add_manual_term_is_listed_and_selected_when_present_in_text() {
        let c = conn();
        add_manual_term(&c, 1, "board", "consiglio", "tecnico", "", false).unwrap();

        let entries = list_glossary(&c, 1).unwrap();
        assert!(
            entries.iter().any(|e| e.source_term == "board"),
            "manual term appears in list_glossary"
        );
        let selected = select_glossary("the board met today", &entries, None);
        assert_eq!(
            sources(&selected),
            vec!["board".to_string()],
            "manual term is picked by select_glossary when it occurs in the unit"
        );
    }

    /// Ticket 04: the manual and perceptor writers share one storage-level dedup
    /// guarantee. Whatever the writer or the case, a single key yields a single
    /// row — the UNIQUE index prevents the concurrent-duplicate race — and the
    /// existing (locked) row is never clobbered.
    #[test]
    fn manual_and_perceptor_paths_dedup_across_case_via_unique_index() {
        let c = conn();
        // Manual add, mixed case, locked.
        let out = add_manual_term(&c, 1, "Board", "consiglio", "tecnico", "", true).unwrap();
        assert!(matches!(out, AddOutcome::Inserted(_)));

        // Perceptor proposes the same term in a different case -> deduped, no insert.
        let n = insert_terms_deduped(&c, 1, &[term("board", "altro")], 4).unwrap();
        assert_eq!(n, 0, "perceptor batch dedups against the manual row across case");

        // A second manual add in yet another case -> Duplicate, no new row.
        let out2 = add_manual_term(&c, 1, "BOARD", "x", "comune", "", false).unwrap();
        assert!(matches!(out2, AddOutcome::Duplicate(_)), "second add is a duplicate");

        let entries = list_glossary(&c, 1).unwrap();
        assert_eq!(entries.len(), 1, "exactly one row for the key regardless of writer/case");
        assert!(entries[0].locked, "the original locked manual row is preserved");
        assert_eq!(entries[0].translation, "consiglio", "existing translation not clobbered");
    }

    /// Ticket 04: `add_manual_term` derives `Duplicate(existing_id)` from the
    /// conflict result and returns the existing row's id (so the UI can open it).
    #[test]
    fn add_manual_duplicate_returns_existing_id_from_conflict() {
        let c = conn();
        let inserted_id = match add_manual_term(&c, 1, "board", "consiglio", "tecnico", "", false).unwrap() {
            AddOutcome::Inserted(id) => id,
            other => panic!("expected Inserted, got {other:?}"),
        };
        let dup = add_manual_term(&c, 1, "BOARD", "altro", "comune", "", true).unwrap();
        assert_eq!(dup, AddOutcome::Duplicate(inserted_id), "conflict yields the existing id");
    }

    /// Ticket 04 follow-up (bug): an all-caps accented term (e.g. Italian
    /// 'SOCIETÀ') must dedup to `Duplicate(existing_id)`, never error. The
    /// conflict fires on the ASCII-casefold `ux_glossary_dedup` index, but the
    /// old fetch probe bound Rust's full-Unicode `to_lowercase()` ('società')
    /// and compared it to SQLite's ASCII `lower()` ('societÀ'); they disagreed
    /// on 'À', the probe found no row and the function returned `Err(Db)`. Both
    /// sides now use `lower(trim(...))`, so they agree.
    ///
    /// Note: dedup is ASCII casefold, so the accent's own case is significant —
    /// 'SOCIETÀ' and 'società' are *distinct* keys. These variants keep the
    /// accent uppercase so they genuinely collide on the index.
    #[test]
    fn add_manual_duplicate_all_caps_accented_returns_duplicate_not_error() {
        let c = conn();
        let inserted_id =
            match add_manual_term(&c, 1, "SOCIETÀ", "company", "comune", "", false).unwrap() {
                AddOutcome::Inserted(id) => id,
                other => panic!("expected Inserted, got {other:?}"),
            };
        // Same all-caps accented term again -> conflict on the ASCII index. Old
        // code would return Err(Db) here (unwrap would panic).
        let out = add_manual_term(&c, 1, "SOCIETÀ", "azienda", "comune", "", true).unwrap();
        assert_eq!(
            out,
            AddOutcome::Duplicate(inserted_id),
            "accented duplicate returns the existing id, not an error"
        );
        // ASCII-case variant that keeps the accent uppercase also collides.
        let out2 = add_manual_term(&c, 1, "SocietÀ", "ditta", "comune", "", false).unwrap();
        assert_eq!(out2, AddOutcome::Duplicate(inserted_id));

        let entries = list_glossary(&c, 1).unwrap();
        assert_eq!(entries.len(), 1, "no second row for the accented key");
        assert_eq!(entries[0].translation, "company", "existing row left untouched");
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

    // --- select_glossary (ticket 03, selezione deterministica) ---------------

    /// Build an in-memory entry without touching the DB (pure-function tests).
    fn entry(source: &str, translation: &str, locked: bool) -> GlossaryEntry {
        GlossaryEntry {
            id: 0,
            source_term: source.into(),
            translation: translation.into(),
            term_type: "comune".into(),
            locked,
            note: String::new(),
            first_seen_page: 1,
        }
    }

    fn sources(v: &[GlossaryEntry]) -> Vec<String> {
        v.iter().map(|e| e.source_term.clone()).collect()
    }

    #[test]
    fn selects_present_term_and_skips_absent() {
        let entries = vec![
            entry("board", "consiglio", false),
            entry("shareholder", "azionista", false),
        ];
        let got = select_glossary("The board approved the plan.", &entries, None);
        assert_eq!(sources(&got), vec!["board".to_string()]);
    }

    #[test]
    fn match_is_case_insensitive_on_word_boundaries() {
        let entries = vec![entry("Board", "consiglio", false)];
        // Case-insensitive hit.
        assert_eq!(select_glossary("the BOARD met", &entries, None).len(), 1);
        // "board" as a substring of "boardroom" must NOT match (word boundary).
        assert_eq!(select_glossary("a boardroom was booked", &entries, None).len(), 0);
    }

    #[test]
    fn no_substring_false_positive() {
        // The classic case: "art" must not match inside "start".
        let entries = vec![entry("art", "arte", false)];
        assert_eq!(select_glossary("we start now", &entries, None).len(), 0);
        assert_eq!(select_glossary("modern art matters", &entries, None).len(), 1);
    }

    #[test]
    fn matches_multiword_term() {
        let entries = vec![entry("consiglio di amministrazione", "board", false)];
        let got = select_glossary(
            "Il consiglio di amministrazione ha deciso oggi.",
            &entries,
            None,
        );
        assert_eq!(got.len(), 1);
        // A partial sub-sequence must not match the full multiword term.
        assert_eq!(
            select_glossary("solo il consiglio comunale", &entries, None).len(),
            0
        );
    }

    #[test]
    fn matches_simple_plural_morphology() {
        // English trailing -s.
        let e_en = vec![entry("board", "consiglio", false)];
        assert_eq!(select_glossary("the boards met", &e_en, None).len(), 1);
        // Italian -o/-i plural on the last word.
        let e_it = vec![entry("titolo", "title", false)];
        assert_eq!(select_glossary("i titoli emessi", &e_it, None).len(), 1);
    }

    #[test]
    fn locked_always_included_beyond_unlocked_cap() {
        let entries = vec![
            entry("alpha", "a", false),
            entry("beta", "b", false),
            entry("gamma", "g", false),
            entry("delta", "d", true), // locked
        ];
        // All four appear in the text; cap unlocked to 1, locked must survive.
        let got = select_glossary("alpha beta gamma delta", &entries, Some(1));
        let got_sources = sources(&got);
        // Locked delta present regardless of the cap.
        assert!(got_sources.contains(&"delta".to_string()), "locked kept");
        // Exactly one unlocked kept (the first in order).
        let unlocked_kept = got.iter().filter(|e| !e.locked).count();
        assert_eq!(unlocked_kept, 1, "unlocked capped to 1");
        assert_eq!(got.iter().filter(|e| e.locked).count(), 1, "locked uncapped");
    }

    #[test]
    fn empty_glossary_and_empty_unit_return_nothing() {
        let entries = vec![entry("board", "consiglio", false)];
        assert!(select_glossary("", &entries, None).is_empty());
        assert!(select_glossary("board board board", &[], None).is_empty());
        assert!(select_glossary("", &[], None).is_empty());
    }

    #[test]
    fn measures_prompt_reduction_vs_full_glossary() {
        use crate::llm::{est_tokens, DEFAULT_CHARS_PER_TOKEN};

        // Realistic synthetic glossary of 120 terms.
        let mut entries: Vec<GlossaryEntry> = (0..120)
            .map(|i| {
                entry(
                    &format!("term{i:03}"),
                    &format!("traduzione del termine numero {i}"),
                    i % 20 == 0, // a few locked
                )
            })
            .collect();
        // Inject a handful of terms that actually appear in the unit.
        entries.push(entry("board", "consiglio di amministrazione", false));
        entries.push(entry("shareholder", "azionista", false));
        entries.push(entry("dividend", "dividendo", true)); // locked + present

        let unit = "The board approved a dividend for every shareholder this year.";

        let full = render_terms(&entries.iter().collect::<Vec<_>>());
        let selected_entries = select_glossary(unit, &entries, None);
        let selected = render_terms(&selected_entries.iter().collect::<Vec<_>>());

        let full_tok = est_tokens(&full, DEFAULT_CHARS_PER_TOKEN);
        let sel_tok = est_tokens(&selected, DEFAULT_CHARS_PER_TOKEN);

        eprintln!(
            "MEASURE glossary tokens: full={full_tok} selected={sel_tok} \
             reduction={:.1}% (chars full={} selected={}) selected_terms={}",
            100.0 * (1.0 - sel_tok as f64 / full_tok as f64),
            full.chars().count(),
            selected.chars().count(),
            selected_entries.len(),
        );

        // The three present terms are selected (incl. the locked one).
        assert_eq!(selected_entries.len(), 3);
        assert!(selected_entries.iter().any(|e| e.source_term == "dividend" && e.locked));
        // Drastic reduction: the subset is a tiny fraction of the whole.
        assert!(sel_tok * 5 < full_tok, "expected >80% token reduction");
    }
}

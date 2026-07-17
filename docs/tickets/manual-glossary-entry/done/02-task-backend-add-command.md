# 02 — Backend: core + comando Tauri per aggiungere un termine

## Parent Spec

[manual-glossary-entry-wayfinder.md](../../specs/manual-glossary-entry-wayfinder.md)

## Type

task

## Outcome

Esiste un comando Tauri `add_glossary_term` che inserisce un nuovo termine nel glossario del
documento corrente, rispettando dedup/no-clobber, con lucchetto configurabile e
`first_seen_page = 0` (marcatore "manuale"). Coperto da test.

## Acceptance Criteria

- [ ] Nuova funzione core in `src-tauri/src/glossary.rs`, es.
      `add_manual_term(conn, document_id, source_term, translation, type, note, locked) ->
      rusqlite::Result<AddOutcome>` dove `AddOutcome` distingue "inserito" (con l'`id` nuovo)
      da "duplicato" (**decisione 01 #2**: restituisce l'`id` della riga esistente perché la
      UI la apra in modifica — NON rifiuta, NON fa upsert). Es.
      `enum AddOutcome { Inserted(i64), Duplicate(i64) }`.
- [ ] Dedup case-insensitive contro i `source_term` già presenti nel documento; **mai**
      modifica righe esistenti (locked inclusi) — sul duplicato il core si limita a
      restituire l'`id` esistente (riuso della logica di esistenza di `insert_terms_deduped`).
- [ ] `locked` è parametrico (default deciso in 01, raccomandato `true`); `first_seen_page`
      salvato a `0`.
- [ ] Validazione lato core: `source_term` e `translation` non vuoti (trim) ⇒ altrimenti
      esito d'errore/segnalazione, nessun insert.
- [ ] Nuovo comando `add_glossary_term` in `src-tauri/src/lib.rs`, registrato
      nell'`invoke_handler` (accanto a `list_glossary`/`update_glossary_term`), che mappa
      l'esito in `Result<_, String>` coerente con gli altri comandi.
- [ ] Test unitari (RED→GREEN): inserimento nuovo termine (unlocked e locked); duplicato
      case-insensitive ⇒ `AddOutcome::Duplicate(id_esistente)` **senza toccare l'esistente**
      (translation/locked della riga esistente invariati); validazione (source/translation
      vuoti) rifiutata; `first_seen_page == 0`; il termine appena aggiunto compare in
      `list_glossary` e, se il suo `source_term` è nel testo, è selezionato da
      `select_glossary`.
- [ ] Suite Rust verde (`cd src-tauri && cargo test`) + `cargo build` pulito.

## Blocked By

- 01-grilling-manual-add-decisions.md

## Frontier

Primo passo eseguibile dopo le decisioni. È il contratto su cui si appoggia la UI (ticket
03).

## Work Plan

1. RED: test in `glossary.rs` per `add_manual_term` (nuovo, duplicato, validazione,
   first_seen_page=0, interazione con list/select).
2. GREEN: implementare `add_manual_term` riusando la logica di esistenza/dedup di
   `insert_terms_deduped`; introdurre `AddOutcome`.
3. Aggiungere e registrare il comando `add_glossary_term` in `lib.rs`.
4. `cargo test` + `cargo build`; suite verde.

## Evidence to Capture

- Nome/firma della funzione core e del comando; risultati dei test.

## Out of Scope

- UI (ticket 03). Eliminazione/modifica di source_term. Import/export. Migrazioni di schema.

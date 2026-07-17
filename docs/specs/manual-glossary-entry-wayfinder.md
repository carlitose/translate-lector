# Aggiunta manuale di termini al glossario

## Type

Wayfinding spec

## Status

Active

## Destination

L'utente può **aggiungere manualmente** un nuovo termine al glossario del documento
corrente (source_term + traduzione + tipo + nota + eventuale lucchetto), non solo
modificare/bloccare i termini già proposti dal percettore. Il termine aggiunto entra nella
stessa tabella `glossary`, partecipa alla selezione deterministica (`select_glossary`) e al
prompt (locked = vincolo assoluto, unlocked = suggerimento) come qualsiasi altro termine.

## Decisions So Far

- **Il glossario oggi è solo auto-popolato + editabile.** I comandi Tauri esistenti sono
  `list_glossary` e `update_glossary_term` (`src-tauri/src/lib.rs:695,706`); il core
  (`src-tauri/src/glossary.rs`) espone `list_glossary`, `update_glossary_term` (per `id`),
  `insert_terms_deduped` (solo percettore, `locked = 0`) e `select_glossary`. **Non esiste
  alcun percorso di inserimento manuale.** (Evidenza: grep glossary in lib.rs; lettura di
  glossary.rs e GlossaryPanel.svelte.)
- **La UI del glossario** (`src/lib/GlossaryPanel.svelte`) è un modale con una tabella che
  modifica in-place traduzione/nota/lucchetto per riga; nessun form di aggiunta.
- **Schema** (`src-tauri/src/db.rs`, tabella `glossary`): `id, document_id, source_term,
  translation, type, locked, note, first_seen_page`. Il `type` è testo libero
  (`typeLabel` mostra `—` se vuoto). **Nessuna migrazione di schema è necessaria** per
  l'aggiunta manuale: bastano gli stessi campi.
- **Dedup**: `insert_terms_deduped` scarta i `source_term` già presenti (case-insensitive)
  e non tocca mai le righe esistenti (locked inclusi). L'aggiunta manuale deve rispettare
  la stessa invariante di non-clobbering.
- **Ortogonale al lavoro in corso** su prefetch/percettore (PR ticket 01 / ticket 02): non
  tocca `translate.rs`/`llm.rs`. Le uniche superfici condivise sono `glossary.rs` e la UI
  del glossario, non modificate da quei due ticket. Da costruire su un branch fresco da
  `main` dopo il merge di quelle PR.

## Decisioni di prodotto (fissate — ticket 01, 2026-07-17)

Le 5 decisioni di prodotto sono state prese dall'utente durante il grilling (ticket 01):

1. **Lucchetto di default**: **locked = true**. Un termine inserito a mano è autorevole
   (vincolo assoluto nel prompt); la checkbox nel form resta modificabile.
2. **Duplicato** (`source_term` già presente, case-insensitive): **aprire in modifica la
   riga esistente**, NON rifiutare e NON fare upsert silenzioso. Il core segnala l'esistenza
   (restituisce la riga/`id` esistente); la UI evidenzia/apre in modifica quella riga invece
   di inserirne una nuova. ⚠️ Diverge dall'assunzione raccomandata ("rifiutare con
   messaggio"): impatta il contratto di 02 (`AddOutcome` deve trasportare l'`id` esistente)
   e di 03 (la UI, su duplicato, apre l'editing inline della riga esistente).
3. **`first_seen_page`** per un termine senza pagina d'origine: **sentinella `0`**, mostrata
   come **"manuale"** nella colonna "Pag."; nessuna colonna/migrazione nuova.
4. **`type`**: **testo libero** con **datalist** di suggerimenti (comune / tecnico / nome
   proprio); nessun set fisso.
5. **Validazione**: `source_term` non vuoto **e** `translation` non vuota (trim) — riuso di
   `isValidTranslation`; `type`/`note` opzionali.

## Out of Scope

- Import/export del glossario (CSV/JSON), o glossari condivisi tra documenti.
- Rimozione/eliminazione di termini (feature separata; qui solo aggiunta).
- Modifica del `source_term` di un termine esistente (resta immutabile, come oggi).
- Migrazioni di schema o una colonna `origin` dedicata (si usa `first_seen_page=0` come
  marcatore).
- Qualsiasi modifica al flusso percettore/prefetch (ticket 01/02).

## Frontier / Blocking Edges

- ~~**Edge — decisioni di prodotto (ticket 01, grilling).**~~ **RISOLTO (2026-07-17)** —
  vedi "Decisioni di prodotto (fissate)". Attenzione: la scelta #2 (duplicato ⇒ aprire in
  modifica, non rifiutare) modifica il contratto assunto in 02/03; aggiornare quei ticket
  prima dell'esecuzione.
- Una volta fissate le decisioni, i due task (backend, poi frontend) sono lineari e
  indipendentemente testabili.

## Ticket Plan

- **01 — grilling** — `01-grilling-manual-add-decisions.md`: fissare le 5 decisioni di
  prodotto sopra (o confermare le assunzioni). Output: decisioni registrate nel map.
- **02 — task** — `02-task-backend-add-command.md`: core `add_manual_term` in `glossary.rs`
  (dedup/no-clobber, locked configurabile, `first_seen_page=0`) + comando Tauri
  `add_glossary_term` in `lib.rs` + test. Output: comando invocabile e testato.
- **03 — task** — `03-task-frontend-add-form.md`: form "Aggiungi termine" in
  `GlossaryPanel.svelte` + helper/tipi in `glossary.ts` + test; wiring del comando; ricarica
  lista; messaggio su duplicato; "Pag." = "manuale" per i termini a mano. Output: l'utente
  aggiunge un termine dalla UI end-to-end.

## Next Review

Dopo il ticket 01: aggiornare "Not Yet Specified" con le decisioni prese e sbloccare 02/03.
Dopo 02/03: verificare end-to-end che un termine aggiunto a mano compaia nella lista, sia
selezionato da `select_glossary` quando appare nel testo, e (se locked) finisca nel blocco
vincoli assoluti del prompt.

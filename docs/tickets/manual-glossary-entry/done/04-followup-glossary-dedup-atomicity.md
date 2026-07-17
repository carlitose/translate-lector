# 04 — Follow-up: dedup del glossario atomico (indice UNIQUE + INSERT … ON CONFLICT)

## Parent Spec

[manual-glossary-entry-wayfinder.md](../../specs/manual-glossary-entry-wayfinder.md)

## Type

task (hardening — emerso in review del ticket 02, fuori dallo scope originale)

## Contesto

La review del ticket 02 ha rilevato che la dedup del glossario è una coppia
**SELECT-then-INSERT non atomica**, senza transazione e senza vincolo UNIQUE a livello di
schema. Ogni comando apre la propria `Connection` SQLite (`open_db`), quindi un
`add_glossary_term` invocato dall'utente può interlacciarsi con il percorso del percettore
in background (`insert_terms_deduped`, chiamato da `translate::translate_page`): entrambi i
controlli di esistenza possono passare prima che uno dei due `INSERT` venga committato,
producendo **righe duplicate** e violando l'invariante documentata "mai crea duplicati".

La proprietà **preesiste** al ticket 02 (`insert_terms_deduped` ha lo stesso pattern); il
ticket 02 l'ha solo ereditata e resa più probabile aggiungendo un secondo scrittore (l'add
manuale). Il fix richiede una migrazione di schema, esplicitamente **fuori scope** per lo
spec manual-glossary-entry, da cui questo follow-up separato.

## Outcome

La dedup del glossario è garantita a livello di storage: due scrittori concorrenti sullo
stesso `source_term` (case-insensitive) nello stesso documento non possono mai creare righe
duplicate. `AddOutcome::Inserted` vs `Duplicate` deriva dal risultato del conflitto, non da
una lettura precedente. Nessuna riga esistente viene mai modificata (no-clobber, locked
inclusi). Coperto da test.

## Acceptance Criteria

- [ ] Migrazione di schema in `src-tauri/src/db.rs`: indice UNIQUE che normalizza la chiave
      di dedup per documento, es. `CREATE UNIQUE INDEX IF NOT EXISTS ux_glossary_dedup ON
      glossary(document_id, lower(trim(source_term)))`. La migrazione è **idempotente** e
      gestisce dati esistenti (vedi criterio sui duplicati pregressi).
- [ ] Prima di creare l'indice, i **duplicati già presenti** in DB vengono riconciliati in
      modo deterministico (es. si mantiene la riga con `id` minimo / o quella locked se
      presente; le altre vengono rimosse) — altrimenti la creazione dell'indice fallirebbe.
      La strategia di riconciliazione è documentata e testata; **mai** perde un termine
      locked.
- [ ] `add_manual_term` (glossary.rs) usa `INSERT … ON CONFLICT DO NOTHING` (o una
      transazione `IMMEDIATE` con check+insert), derivando `Inserted(new_id)` /
      `Duplicate(existing_id)` dall'esito del conflitto invece che da una SELECT precedente.
- [ ] `insert_terms_deduped` (glossary.rs) usa lo stesso meccanismo `ON CONFLICT DO NOTHING`
      per il batch del percettore, mantenendo il conteggio degli inseriti effettivi.
- [ ] Semantica invariata rispetto a oggi: dedup case-insensitive + trim; no-clobber su
      righe esistenti (locked incluse); `first_seen_page` preservato sulle righe esistenti;
      i manuali restano `first_seen_page = 0`.
- [ ] Test (RED→GREEN): duplicato concorrente non crea righe doppie (simulazione di due
      insert ravvicinati / stessa chiave con case diverso); riconciliazione dei duplicati
      pregressi con preservazione del locked; regressione — tutti i test glossary esistenti
      restano verdi.
- [ ] Suite Rust verde (`cd src-tauri && cargo test`) + `cargo build` pulito.

## Blocked By

- #8 (ticket 02 — `add_glossary_term` / `add_manual_term`) mergiato in `main`.

## Frontier

Hardening successivo al merge della feature. Non blocca la UI (ticket 03): il bug è una race
rara che oggi si manifesta al più con una riga doppia occasionale, gestibile a mano.
Affrontarlo quando la feature è in `main`, così la migrazione parte dallo schema consolidato.

## Work Plan

1. RED: test che dimostra la creazione di righe duplicate con la stessa chiave (case diverso)
   senza il vincolo UNIQUE; test di riconciliazione dei duplicati pregressi.
2. GREEN: migrazione con riconciliazione dei duplicati + indice UNIQUE in `db.rs`.
3. Convertire `add_manual_term` e `insert_terms_deduped` a `INSERT … ON CONFLICT DO NOTHING`,
   derivando l'esito dal conflitto.
4. `cargo test` + `cargo build`; verificare che i test di dedup/no-clobber esistenti restino
   verdi.

## Evidence to Capture

- Definizione dell'indice e strategia di riconciliazione; nomi/esito dei test; conferma che
  nessun termine locked viene perso dalla migrazione.

## Out of Scope

- UI. Modifica del contratto dei comandi Tauri (l'esito `{status, id}` resta identico).
  Cambiamenti alla logica di selezione/rendering del glossario.

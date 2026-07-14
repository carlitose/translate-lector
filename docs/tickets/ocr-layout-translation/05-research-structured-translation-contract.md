# 05 — Contratto traduzione strutturata + percettore + schema dati

## Parent Spec

[ocr-layout-translation-wayfinder.md](../../specs/ocr-layout-translation-wayfinder.md)

## Type

research

## Outcome

Un design chiuso per: (a) **tradurre preservando la struttura** (per-blocco) mantenendo la coerenza del
percettore (rolling summary + glossario) dell'MVP; e (b) l'**estensione del modello dati SQLite** per
memorizzare layout OCR, cache OCR e ricostruzione. Prepara le build verticali.

## Acceptance Criteria

- [ ] **Contratto traduzione strutturata**: come mappare blocchi OCR ↔ testo tradotto. Opzioni valutate:
      tradurre l'intera pagina come testo e ri-mappare ai box, vs tradurre blocco-per-blocco con ID,
      vs un unico JSON `{ blocks: [{id, translated_text}], updated_summary, new_glossary_terms }`.
      Decisione motivata.
- [ ] **Compatibilità percettore**: il nuovo contratto deve ancora produrre `updated_summary` e
      `new_glossary_terms` come l'attuale (vedi SPECIFICATION.md §4.4 e `src-tauri/src/llm.rs` /
      `translate.rs`). Confermare che summary/glossario restano coerenti sulla pipeline OCR.
- [ ] **Schema dati**: estensione di `translations_cache` (SPECIFICATION.md §4.3) o nuove tabelle per:
      modello di layout per pagina, box+testo per blocco, immagini/regioni di pagina (path o blob),
      flag "pagina OCR vs testo". Bozza SQL di migrazione.
- [ ] **Rilevamento pagina scansionata**: formalizzare la logica di routing (sostituisce EC01) —
      per-pagina, riusando il campionamento già in `src/routes/+page.svelte`.
- [ ] **Impatto costi/token**: nota su come la traduzione strutturata cambia il consumo token rispetto
      al testo semplice attuale.
- [ ] Design registrato nel parent spec, pronto per `to-tickets`.

## Blocked By

- Ticket 02 (struttura di layout realmente disponibile).
- Ticket 04 (decisioni su vista/export/fedeltà, che vincolano cosa va persistito).

## Frontier

Ultimo edge prima delle build verticali. Chiude il "come" tecnico una volta noti "quanta struttura
abbiamo" (02), "cosa deve fare il prodotto" (04) e "come si rende" (03).

## Work Plan

1. Rivedere il contratto e il percettore attuali in `src-tauri/src/llm.rs`, `translate.rs`,
   `src/lib/translation.ts` e SPECIFICATION.md §4.4.
2. Progettare il contratto di traduzione strutturata riusando il più possibile il percettore esistente.
3. Progettare l'estensione dello schema SQLite (`src-tauri/src/db.rs`) con migrazione.
4. Formalizzare il rilevamento/routing pagina scansionata a partire dal codice EC01 esistente.
5. Scrivere il design nel parent spec; verificare che sia sufficiente per `to-tickets`.

## Evidence to Capture

- Contratto JSON proposto (input/output) e confronto con quello attuale.
- Bozza SQL delle tabelle/colonne nuove.
- Nota su token/costi e su compatibilità percettore.

## Out of Scope

- Implementazione (spetta alle build verticali derivate dopo questa mappa).

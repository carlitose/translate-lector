# Ticket 02 — Mitigazione: cap del correction-retry del percettore sul provider locale

## Parent Spec

[prefetched-page-arrival-latency-diagnosis.md](../../specs/prefetched-page-arrival-latency-diagnosis.md)

## Type

AFK

## What to Build

Con il two-phase arrival (ticket 01) il percettore è fuori dal percorso di risposta, ma il
suo costo resta reale: sul provider locale il parse JSON strict fallisce di routine e
`complete_and_parse_perceptor_update` paga una **seconda** chiamata LLM full-page di
correzione — che con l'esito `Recovered` (summary non avanzato) si ripete ad ogni pagina.
Questo tiene occupato il `LocalProviderSlot` e ritarda il prefetch di N+1 in coda.

Ridurre il costo del caso peggiore: sul provider locale, limitare o saltare il
correction-retry del percettore quando il primo tentativo non produce JSON conforme,
mantenendo l'osservabilità dell'esito (`perceptor_update_failed` / nota contesto) e la
semantica di retry-alla-rivisita introdotta dal ticket 01.

Vedi la spec, sezione **Decision / Solution**, punto 3.

## Acceptance Criteria

- [ ] Sul provider locale, un perceptor-update col primo tentativo non-JSON costa al
      massimo 1 chiamata LLM (nessun correction-retry, o retry dietro opt-in/config).
- [ ] Il comportamento sui provider cloud resta invariato (retry come oggi).
- [ ] L'esito parziale/fallito resta osservabile (`PerceptorUpdateResult` →
      `perceptor_update_failed`), e il marker del ticket 01 non si setta, così la
      rivisita ritenta.
- [ ] Suite `cargo test` verde.

## Blocked By

- [01-two-phase-arrival-decouple-perceptor.md](./01-two-phase-arrival-decouple-perceptor.md)

## Frontier

Blocked by ticket 01 (la semantica marker/retry-alla-rivisita deve esistere prima di
togliere il retry immediato, altrimenti si perdono termini senza seconda chance).

## Step-by-Step Implementation Plan

1. **Parametrizzare il retry** in `complete_and_parse_perceptor_update`
   (`src-tauri/src/translate.rs`): aggiungere un flag/parametro (es. `allow_correction_retry`)
   ai `TranslateParams` o derivarlo dal provider (locale ⇒ false), riusando il predicato
   provider-locale già esistente (`should_attach_is_current`/`is_local` in `lib.rs`).
   Verificare che il default per i provider cloud sia invariato.
2. **Mappare l'esito senza retry**: primo tentativo non conforme ⇒ tornare
   `Recovered`/`Failed` secondo la semantica di 6f37c30 (nessuna nuova variante se non
   necessaria), lasciando il marker del ticket 01 unset.
3. **Test**: mock client che risponde non-JSON al percettore ⇒ sul locale 1 sola chiamata,
   esito osservabile; sul cloud 2 chiamate (retry) come oggi. Adattare eventuali test
   esistenti che assumono il retry incondizionato.

## Testing Plan

- Unit test di conteggio chiamate come sopra (pattern mock esistente in `translate.rs`).
- Restano verdi i test di resilienza del percettore introdotti da 6f37c30
  (`glossary-not-updating/02`), adattati al nuovo flag dove serve.
- Verifica manuale: log dell'app sul locale — un solo tentativo percettore per pagina.

## Out of Scope

- Migliorare il prompt/parse del percettore per ridurre i fallimenti (epica separata).
- Cambiare la semantica di `Recovered`/avanzamento summary.

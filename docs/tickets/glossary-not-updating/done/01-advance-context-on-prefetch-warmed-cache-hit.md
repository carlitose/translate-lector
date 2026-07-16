# 01 — Avanzare summary + glossario sulla pagina scaldata dal prefetch

## Parent Spec

[glossary-not-updating-diagnosis.md](../../specs/glossary-not-updating-diagnosis.md)

## What to Build

L'auto-popolamento del glossario deve tornare a funzionare durante la lettura normale (con
prefetch attivo, provider locale di default). Oggi il prefetch della pagina N+1
(`updateContext:false`) scrive la cache di **pagina** incondizionatamente (`translate.rs:991`) ma
salta il percettore; alla navigazione reale su N+1 (`updateContext:true`) il cache-hit di pagina
ritorna prima del blocco percettore (`translate.rs:704-715`), quindi
`glossary::insert_terms_deduped` non viene mai raggiunto e nessun termine viene aggiunto. Vedi la
diagnosi per la catena completa e il commit di regressione (`d0fe497`).

La correzione (opzione B della diagnosi): **il prefetch non deve scrivere la cache di pagina** —
solo la cache per-unità (STC-09). Guardare `cache_insert` dietro `p.update_context`. Alla
navigazione reale la pagina risulta un miss di pagina, la pipeline prosegue, le unità sono servite
dalla cache per-unità (nessuna ri-traduzione), il percettore gira una volta, il glossario cresce e
la riga di pagina viene finalmente scritta come "completa".

Se durante l'implementazione emerge che servire la traduzione senza ri-assemblare le unità è
preferibile, è accettabile l'opzione A (flag `context_advanced` su `translations_cache`); in tal
caso documentare la scelta e gestire le righe già esistenti.

## Acceptance Criteria

- [ ] Con provider `llamaserver` e prefetch ON, leggendo in sequenza le pagine, il glossario
      **cresce**: i `new_glossary_terms` proposti dal percettore vengono inseriti (deduped) per
      ogni pagina visitata per la prima volta in navigazione reale.
- [ ] Il percettore gira **esattamente una volta** per pagina in navigazione reale (nessuna doppia
      esecuzione, nessun avanzamento di contesto fuori ordine da parte del prefetch).
- [ ] La traduzione della pagina resta immediata all'arrivo (nessuna ri-traduzione delle unità
      grazie alla cache per-unità STC-09).
- [ ] Il `rolling_summary` avanza una volta per pagina reale, come prima (EC05 invariato).
- [ ] Termini locked mai modificati; check riga avvelenata (ticket 16) invariato; serializzazione
      L3 invariata; comportamento cloud invariato.
- [ ] Nuovo test di regressione (RED prima del fix, GREEN dopo): prefetch di P
      (`update_context=false`) → navigazione su P (`update_context=true`, stesso testo) ⇒ il
      percettore viene chiamato una volta e il glossario passa da 0 a N termini proposti.
- [ ] Suite completa verde (`cargo test` in `src-tauri`, `npm test`).

## Blocked By

- None - can start immediately.

## Frontier

Ready now. Bug attivo che l'utente sta colpendo dal vivo; fix piccolo e localizzato in
`translate_page` + test. È la causa primaria dello stop dell'auto-popolamento del glossario.

## Step-by-Step Implementation Plan

1. **RED**: aggiungere in `translate.rs` (modulo test, con `MockClient` + DB in-memory) il test
   composto prefetch-poi-navigazione descritto negli Acceptance Criteria. Deve fallire oggi
   (glossario 0, 0 chiamate percettore sul secondo accesso).
2. **GREEN**: in `src-tauri/src/translate.rs::translate_page` guardare `cache_insert`
   (`~riga 991`) dietro `if p.update_context`, così il prefetch non scrive la cache di pagina.
   Verificare che il ramo `update_context` (summary + `insert_terms_deduped`) resti come oggi e
   che alla navigazione reale la pagina sia un miss di pagina che prosegue fino al percettore.
3. **Verifica riuso per-unità**: confermare (test o ispezione) che le unità tradotte durante il
   prefetch sono servite dalla cache per-unità (STC-09) alla navigazione reale — niente
   ri-traduzione, latenza invariata.
4. **Regressione**: eseguire i test esistenti su cache/percettore/prefetch — in particolare
   `prefetch_caches_translation_without_touching_summary_or_glossary` va **aggiornato** al nuovo
   contratto (il prefetch non scrive più la cache di pagina; verifica invece la cache per-unità).
5. Verifica finale: `cargo build` + `cargo test` in `src-tauri`; `npm test`; suite verde.
6. Verifica manuale (non AFK): app dev, provider `llamaserver`, leggere alcune pagine in sequenza
   e confermare che il pannello glossario cresce ad ogni nuova pagina.

## Testing Plan

- Nuovo unit test composto (prefetch → navigazione reale → glossario cresce, percettore chiamato
  una volta, traduzione servita).
- Aggiornamento del test di prefetch esistente al nuovo contratto di caching.
- Regressione: test cache/percettore/serializzazione verdi.
- Manuale: crescita del glossario durante lettura sequenziale col provider locale.

## Out of Scope

- Resilienza/osservabilità del fallimento del perceptor-update sul modello locale — ticket 02.
- Modifiche al frontend (il prefetch parte già correttamente).
- Modifiche al provider cloud o alla serializzazione L3.

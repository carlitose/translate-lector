# Ticket 01 — Two-phase arrival: consegna la traduzione subito, avanza il contesto fuori dal percorso di risposta

## Parent Spec

[prefetched-page-arrival-latency-diagnosis.md](../../specs/prefetched-page-arrival-latency-diagnosis.md)

## Type

AFK

## What to Build

Alla navigazione reale su una pagina, `translate_page` deve restituire il testo tradotto
appena le unità sono assemblate (dalla cache per-unità scaldata dal prefetch, o appena
tradotte), **senza** attendere il perceptor-update. L'avanzamento del contesto
(percettore → `set_rolling_summary` + `insert_terms_deduped`) diventa una fase separata
eseguita fuori dal percorso di risposta, esattamente una volta per pagina alla prima
navigazione reale.

Comportamento end-to-end atteso: leggendo in sequenza con prefetch attivo sul provider
locale, l'arrivo su N+1 mostra il testo istantaneamente (0 chiamate LLM bloccanti) e il
glossario continua a crescere pagina dopo pagina.

Vedi la spec, sezioni **Decision / Solution** (forma del fix e i 2 dettagli vincolanti:
marker "contesto avanzato" e ordinamento) e **Testing Decisions**.

## Acceptance Criteria

- [ ] Arrivo su pagina interamente prefetchata: il testo ritorna con 0 chiamate LLM
      sincrone sul percorso di risposta (regression test dai repro in `./repro/`).
- [ ] Il percettore gira comunque una volta per pagina alla prima navigazione reale e il
      glossario cresce (il regression test di 2e81d42
      `prefetch_warmed_page_advances_context_on_real_navigation_without_retranslating`
      resta verde nella sua sostanza: contesto avanzato, unità non ritradotte).
- [ ] Rivisita di una pagina già avanzata: il percettore NON rigira.
- [ ] Fallimento del percettore (esiti `Recovered`/`Failed` di 6f37c30): il marker NON si
      setta; una rivisita successiva ritenta l'avanzamento; `perceptor_update_failed`
      resta osservabile dal frontend.
- [ ] Il prefetch di N+1 parte solo dopo che l'avanzamento contesto di N è completato
      (ordinamento del summary preservato).
- [ ] Il prefetch continua a NON avanzare il contesto e a NON scrivere la cache di pagina.
- [ ] Suite `cargo test` verde, inclusi i test di cancellazione is_current (d0fe497) e
      righe avvelenate (ticket 16).

## Blocked By

- None - can start immediately.

## Frontier

Ready now. Unica decisione implementativa aperta (dalla spec, Open Questions): marker
`context_advanced` su `translations_cache` (migrazione schema, rivisite servite dal
page-hit) **vs** scrittura della cache di pagina posticipata alla fase di avanzamento
(nessuna migrazione, rivisite intermedie riassemblate per-unità). Scegliere la prima se
la migrazione è banale (`ALTER TABLE ... ADD COLUMN ... DEFAULT 1` per le righe esistenti,
che sono tutte pre-Opzione-B o scritte da navigazioni reali complete); documentare la
scelta nel commit.

## Step-by-Step Implementation Plan

1. **Estrarre la fase di avanzamento contesto da `translate_page`**
   (`src-tauri/src/translate.rs`): isolare il blocco percettore (chiamata
   `complete_and_parse_perceptor_update`, summary, `insert_terms_deduped`, gestione
   `PerceptorUpdateResult`) in una funzione dedicata (es. `advance_context`) che prende
   documento/pagina/testo e il client. `translate_page` su navigazione reale si ferma
   dopo l'assemblaggio delle unità (+ scrittura cache secondo la decisione sul marker) e
   ritorna. Verificare: i test esistenti compilano; il percettore non è più raggiungibile
   dal percorso di risposta.
2. **Semantica "esattamente una volta" per pagina**: implementare il marker scelto (colonna
   su `translations_cache` in `db.rs` + `cache_insert`/`cache_lookup`, oppure posticipo
   della scrittura di pagina). Regola: il marker si setta solo su esito `Full` (summary
   avanzato E termini inseriti secondo la semantica di 6f37c30); su `Recovered`/`Failed`
   resta unset così la rivisita ritenta. Pitfall: non ricreare il bug del glossario — il
   fast-return del page-hit non deve saltare un avanzamento mai avvenuto.
3. **Nuovo comando Tauri `advance_context`** (`src-tauri/src/lib.rs`): espone la fase 2.
   Deve acquisire `LocalProviderSlot` come le traduzioni (serializzazione col modello
   locale) e restare guidato dal cursore (se l'utente è già andato oltre, l'avanzamento
   della pagina superata può comunque completare: è in ordine perché serializzato).
   Riutilizzare il wiring provider/config di `translate_page`. Verificare con un unit test
   del comando o della funzione condivisa.
4. **Wiring frontend** (`src/routes/+page.svelte`): dopo che `translate_page` risolve,
   renderizzare subito il testo; poi `await invoke('advance_context', ...)` (aggiornando
   `contextNoteText`/`perceptor_update_failed` dal suo risultato) e **solo dopo** lanciare
   `void prefetchNextPage()`. Pitfall: mantenere il guard `isCurrentRequest` per scartare
   risposte stantie; non bloccare la UI durante `advance_context` (l'utente sta leggendo).
5. **Regression test dal repro**: portare dentro `translate.rs` (mod tests) il test di
   conteggio dai patch in `./repro/` (es. `arrival_on_fully_prefetched_page_*`): prefetch
   completo di P → navigazione reale su P ⇒ 0 chiamate LLM sul percorso di risposta, testo
   identico dalla cache; poi `advance_context` ⇒ percettore chiamato, glossario cresciuto.
   Aggiungere i test del marker (rivisita non rigira; fallimento ritenta) e verificare la
   suite completa.

## Testing Plan

- Nuovi test livello `translate_page`/`advance_context` come sopra (mock client, pattern
  esistente in fondo a `translate.rs`).
- Devono restare verdi: `prefetch_caches_translation_without_touching_summary_or_glossary`,
  il regression test di 2e81d42 (adattato alla nuova forma two-phase se necessario, senza
  perderne l'intento), i test di cancellazione is_current e i test glossario (41).
- Verifica manuale (app reale, provider llamaserver): lettura sequenziale di 3-4 pagine
  con prefetch ON ⇒ arrivo istantaneo su ogni N+1, glossario che cresce, nessun doppio
  percettore per pagina nei log.

## Out of Scope

- Mitigazione del correction-retry del percettore sul locale (ticket 02).
- Affidabilità del parse JSON del percettore (epica glossary-not-updating/02).
- Il vettore "termine locked aggiunto tra prefetch e arrivo" (open question nella spec).

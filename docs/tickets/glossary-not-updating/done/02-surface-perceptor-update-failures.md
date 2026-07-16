# 02 — Rendere osservabile/resiliente il fallimento del perceptor-update (provider locale)

## Parent Spec

[glossary-not-updating-diagnosis.md](../../specs/glossary-not-updating-diagnosis.md)

## What to Build

Bug secondario emerso dalla triangolazione (lente data-flow). STC-10 avvolge la chiamata
perceptor-update in un `match` che, su errore, logga solo con `eprintln!` e lascia
`summary_advanced = false` (`src-tauri/src/translate.rs:971-983`). Sulle pagine **non** prefetchate
(cache miss), la chiamata percettore usa il parsing a JSON stretto (`content()` +
`parse_perceptor_update`, `llm.rs`), molto più fragile del fallback testo-libero usato dalla
traduzione per-unità (`content_complete()`/`parse_translation`). Sul modello locale piccolo
(`llamaserver`, `n_ctx` ridotto, `max_tokens` limitato) questa chiamata può fallire di routine
(budget EC08 / contenuto vuoto o reasoning-only / JSON non conforme anche dopo il retry di
correzione), perdendo **silenziosamente** i termini del glossario per quelle pagine.

Obiettivo: rendere il fallimento **osservabile** all'utente e ridurne la frequenza, senza mai
perdere la traduzione già prodotta (invariante STC-10).

## Acceptance Criteria

- [ ] Quando il perceptor-update fallisce, l'utente riceve un segnale non intrusivo (es. stato/nota
      "contesto non aggiornato per questa pagina" nella UI), non solo un `eprintln!` invisibile.
- [ ] La traduzione della pagina resta valida, cachata e mostrata anche quando il percettore
      fallisce (STC-10 invariato).
- [ ] Riduzione dei fallimenti sul provider locale tramite almeno una fra: (a) stessa cura di
      retry/crescita-budget su troncamento già usata dal loop delle unità, e/o (b) estrazione
      tollerante di `new_glossary_terms` da JSON parziale/malformato, così i termini si inseriscono
      anche quando l'`updated_summary` non è recuperabile.
- [ ] Se si adotta (b): il glossario può crescere anche quando il summary non avanza — disaccoppiare
      l'inserimento dei termini dalla rigida condizione `summary_advanced` dove è sicuro farlo.
- [ ] Termini locked mai modificati; nessuna regressione del caching della traduzione.
- [ ] Test: (RED→GREEN) un mock client che restituisce una risposta percettore che oggi fa fallire
      il parse ⇒ dopo il fix i termini estraibili vengono inseriti e/o il fallimento è segnalato.
- [ ] Suite completa verde.

## Blocked By

- None tecnicamente. **Consigliato dopo il ticket 01**: una volta ripristinato il flusso primario
  (glossario cresce sulle pagine prefetchate), è più facile isolare e osservare i fallimenti
  residui del percettore sulle pagine a cache miss.

## Frontier

Secondario. Non è ciò che ha spento l'auto-popolamento (sulle pagine prefetchate il percettore non
viene proprio chiamato), ma è un bug reale di robustezza/osservabilità che degrada la qualità del
glossario sul provider locale e che oggi è completamente silenzioso.

## Step-by-Step Implementation Plan

1. Riprodurre il fallimento del percettore con un mock che emula il modello locale: contenuto vuoto
   + `finish_reason:length` (EC08), reasoning-only, e JSON non conforme. Verificare che oggi i
   termini vadano persi silenziosamente.
2. Decidere l'approccio (a) retry/budget come le unità, (b) estrazione tollerante dei termini, o
   entrambi. Documentare la scelta nel ticket.
3. Implementare in `translate.rs` (boundary del perceptor-update, `~958-983`) e/o `llm.rs`
   (`parse_perceptor_update` e formato risposta). Mantenere l'invariante: la traduzione non si
   perde mai.
4. Aggiungere il segnale UI (propagare uno stato "contesto non aggiornato" nel risultato di
   `translate_page` → frontend), evitando modali intrusivi.
5. Test unitari dei nuovi rami di parsing/resilienza + test che il segnale sia propagato.
6. `cargo test` + `npm test`; verifica manuale col provider locale su pagine reali (non prefetchate,
   es. saltando direttamente a una pagina).

## Testing Plan

- Unit test dei sotto-casi di fallimento (EC08 / vuoto / JSON malformato) con estrazione tollerante
  e/o segnalazione.
- Test di propagazione del segnale al frontend.
- Regressione: caching della traduzione e termini locked invariati.
- Manuale: su una pagina a cache miss col provider locale, o i termini si inseriscono o il
  fallimento è visibile.

## Out of Scope

- Il fix primario del prefetch/cache-hit — ticket 01.
- Sostituire il modello locale o cambiare i default `n_ctx`/`max_tokens` (eventuale follow-up
  separato se la resilienza non basta).

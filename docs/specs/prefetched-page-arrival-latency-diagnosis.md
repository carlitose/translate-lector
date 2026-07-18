# Diagnosi — L'arrivo sulla pagina N+1 prefetchata è bloccato dal perceptor-update sincrono

## Type

Diagnostic spec

## Status

Confirmed (2026-07-18, triangolazione a 3 diagnosi indipendenti, convergenza 3/3 sul
meccanismo, 2 feedback loop deterministici indipendenti + 1 loop di falsificazione;
confidenza alta)

## Problem / Context

Sintomo riportato: "con il fix del glossario si è di nuovo rotto il sistema che mi traduce
la pagina N+1". Dopo il fix del glossario (Opzione B, commit `2e81d42` — il prefetch non
scrive più la cache di pagina), arrivando sulla pagina N+1 già prefetchata la traduzione
**non appare subito**: l'utente aspetta come se la pagina venisse ritradotta da zero.

Requisito duro: devono funzionare **entrambi** — l'auto-popolamento del glossario
([glossary-not-updating-diagnosis.md](glossary-not-updating-diagnosis.md)) e la traduzione
anticipata della pagina N+1
([local-prefetch-cancellation-diagnosis.md](local-prefetch-cancellation-diagnosis.md)).
Quest'area ha già oscillato due volte: il fix del prefetch (`d0fe497`) ha rotto il
glossario; il fix del glossario (`2e81d42`) ha ora degradato l'arrivo su N+1.

## Goals

- Arrivo su N+1 prefetchata: testo tradotto visibile senza attendere chiamate LLM.
- Il glossario continua a crescere ad ogni navigazione reale (percettore eseguito
  esattamente una volta per pagina).
- Nessuna regressione dei fix precedenti (d0fe497, 2e81d42, ticket 16, STC-09).

## Non-Goals

- Migliorare l'affidabilità del parse JSON del percettore sul modello locale (tracciato a
  parte, 6f37c30 / ticket glossary-not-updating/02).
- Ridefinire la staleness del prefetch (opzione B della diagnosi prefetch-cancellation).

## Evidence

Le tre diagnosi (lenti: repro-first, data-flow, recent-change) sono partite senza ipotesi
condivise e sono convergenti 3/3 sullo stesso meccanismo.

**Cosa NON è rotto** (falsificato da tutte e tre):

- Le unità **non** vengono ritradotte all'arrivo: la cache per-unità (STC-09) è intatta.
  Il regression test di 2e81d42
  (`prefetch_warmed_page_advances_context_on_real_navigation_without_retranslating`) passa;
  test di conteggio con mock client confermano 0 chiamate translate-unit all'arrivo.
- Il prefetch parte e completa: `should_attach_is_current` (`src-tauri/src/lib.rs:85-86`)
  esclude ancora il prefetch dal check `is_current` (d0fe497 intatto); il repro mostra
  `unit_rows==1, page_rows==0` dopo il prefetch.
- I commit glossario successivi (indice UNIQUE `fce364a`, `add_glossary_term` `560c9c7`/
  form `fb2c482`) non toccano split/pack/chiavi cache/prefetch: 41 test glossario verdi.
- Le chiavi della cache per-unità (`doc, page, unit_index, lang, source_hash(body)`,
  `translate.rs:518-535`) non includono glossario/summary; il prefetch parte solo dopo che
  la navigazione su N è risolta, quindi il contesto è identico tra prefetch e arrivo.

**Cosa è rotto** (meccanismo consenso):

- Con l'Opzione B, la navigazione reale su N+1 prefetchata è per design un **MISS della
  cache di pagina**: il fast-return del cache-hit (`translate.rs:766-780`) non scatta al
  primo arrivo. La pipeline riassembla il testo dalla cache per-unità (0 chiamate modello)
  ma `translate_page` esegue **sincrono** il blocco perceptor-update
  (`translate.rs:1006-1076`) e ritorna solo dopo (`~:1119-1144`). Il frontend renderizza
  solo alla risoluzione dell'`invoke` (`src/routes/+page.svelte:421-441`).
- Sul provider locale (default `llamaserver`) il percettore fallisce di routine il parse
  JSON strict → retry di correzione in `complete_and_parse_perceptor_update`
  (`translate.rs:627-679`) → **2 chiamate LLM full-page bloccanti** prima che il testo già
  pronto attraversi l'IPC. Con l'esito `Recovered` di 6f37c30 il summary non avanza, quindi
  il doppio costo si ripete ad ogni pagina.
- Prima di 2e81d42 l'arrivo era un cache-hit di pagina istantaneo (che però saltava il
  percettore: era il bug del glossario). Il claim "latenza invariata" di 2e81d42 valeva
  solo per le chiamate translate-unit, non per il time-to-display.

**Feedback loop costruiti** (patch conservate in
`docs/tickets/prefetched-page-arrival-latency/repro/`):

- *repro-first*: 2 test falliti deterministici a livello `translate_page` — 1 chiamata
  modello bloccante prima del ritorno del testo interamente cachato; con client da
  150ms/chiamata e risposte perceptor non-JSON, arrivo = 301ms ≈ 2 chiamate piene
  (`repro-first.patch`).
- *recent-change*: test di conteggio fallito `left: 2, right: 0` — 2 chiamate LLM sincrone
  all'arrivo su pagina interamente scaldata (`recent-change.patch`).
- *data-flow*: 3 test diagnostici verdi che falsificano le ipotesi di miss della cache
  per-unità e inchiodano il costo dell'arrivo a 2 chiamate perceptor sincrone
  (`data-flow-diag.patch`).

## Decision / Solution

**Root cause**: il perceptor-update è finito sul percorso critico dell'arrivo. Il fix va
in `src-tauri/src/translate.rs` + `src-tauri/src/lib.rs` (+ wiring in
`src/routes/+page.svelte`): **decoppiare la restituzione della traduzione
dall'avanzamento del contesto** (two-phase arrival).

- **Fase 1 (view path)**: alla navigazione reale, `translate_page` ritorna il testo appena
  le unità sono assemblate (dalla cache per-unità o appena tradotte). Il frontend
  renderizza subito.
- **Fase 2 (context path)**: il perceptor-update + `set_rolling_summary` +
  `insert_terms_deduped` girano fuori dal percorso di risposta — comando dedicato
  `advance_context(document_id, page_number, page_text)` invocato dal frontend dopo il
  render (o task backend), sempre serializzato dietro `LocalProviderSlot` e guidato dal
  cursore.

Dettagli vincolanti emersi dalla triangolazione:

1. **Marker "contesto avanzato"**: se la fase 1 scrive la cache di pagina prima che il
   percettore giri, serve un marcatore per-pagina (flag su `translations_cache` o tabella
   dedicata) così che il percettore giri **esattamente una volta** per pagina alla prima
   navigazione reale e non venga saltato dal fast-return del page-hit alla rivisita —
   altrimenti si ricrea il bug del glossario. (In alternativa: scrivere la cache di pagina
   solo al termine della fase 2, accettando il riassemblaggio per-unità alle rivisite
   intermedie.)
2. **Ordinamento del contesto**: il frontend deve attendere il completamento di
   `advance_context` di N **prima** di lanciare `prefetchNextPage()` di N+1, così il
   prefetch usa il summary aggiornato e il contesto avanza in ordine (l'utente sta già
   leggendo: il prefetch non è sul percorso percepito).
3. **Mitigazione complementare**: valutare il taglio/cap del correction-retry del
   percettore sul provider locale una volta che il testo è già consegnato (dimezza il
   costo del caso peggiore, non elimina il problema).

Vincoli preservati: glossario cresce ad ogni navigazione reale; prefetch efficace (0
chiamate bloccanti all'arrivo); il prefetch non avanza mai il contesto né scrive la cache
di pagina; termini locked immutabili; check righe avvelenate (ticket 16); riuso STC-09.

## Options Considered

- **A — Two-phase arrival (scelta)**: ritorno immediato del testo + avanzamento contesto
  fuori dal percorso di risposta. Endorsed dalle tre diagnosi; unica forma che soddisfa
  entrambi i requisiti senza sacrificarne uno.
- **B — Evento Tauri prima del blocco percettore**: `translate_page` resta monolitico ma
  emette il testo via evento; l'`invoke` risolve dopo col contesto. Variante minimale
  della stessa idea; meno pulita (doppio canale per lo stesso dato) ma senza marker.
- **C — Solo mitigazione retry**: cap del correction-retry sul locale. Dimezza il caso
  peggiore ma lascia 1 chiamata LLM piena bloccante sull'arrivo: non risolve il sintomo.
- **D — Tornare al page-cache-hit sull'arrivo**: ripristina la latenza zero ma ricrea il
  bug del glossario (già diagnosticato e fixato): esclusa.

## Testing Decisions

- Gap che ha fatto passare il bug: nessun test misura il **numero di chiamate LLM
  bloccanti** (o il time-to-display) sull'arrivo di una pagina prefetchata; il regression
  test di 2e81d42 verifica solo l'assenza di ri-traduzione delle unità.
- Aggiungere come regression test i repro della triangolazione (patch in
  `docs/tickets/prefetched-page-arrival-latency/repro/`): arrivo su pagina interamente
  prefetchata ⇒ il testo ritorna con **0 chiamate LLM sincrone**, e il percettore gira
  comunque (una volta) fuori dal percorso di risposta, facendo crescere il glossario.
- Test del marker: prima navigazione reale su pagina prefetchata → percettore gira;
  rivisita della stessa pagina → percettore NON rigira; fallimento del percettore →
  rivisita successiva lo ritenta (il marker non deve marcarsi su fallimento).
- Test di ordinamento: `advance_context` di N completa prima che il prefetch di N+1 parta.

## Follow-Up Tickets

- Ticket 01 — Fix primario: two-phase arrival (`translate_page` ritorna al riassemblaggio;
  comando `advance_context`; marker contesto-avanzato; wiring frontend render-then-advance;
  regression test dai repro).
- Ticket 02 — Mitigazione: cap/skip del correction-retry del percettore sul provider
  locale quando il testo è già consegnato (opzionale, riduce il caso peggiore).

## Open Questions

- **Divergenza minore (unica della triangolazione)**: un termine **locked** aggiunto
  manualmente tra prefetch e arrivo può cambiare lo split delle unità (via `glossary_est`
  → `budget_unit_text`, `translate.rs:804-806`) e causare un miss reale della cache
  per-unità? Una diagnosi lo ritiene possibile per paragrafi vicini alla soglia di split
  con `n_ctx` piccolo; un'altra l'ha testato (packing stabile col clamp fisso
  `PACK_TARGET_TOKENS`, tutti hit) senza però coprire il caso a ridosso della soglia. Non
  è la root cause; da tenere d'occhio se il sintomo ricompare dopo un add manuale.
- Scelta implementativa nel ticket 01: marker su `translations_cache` (migrazione schema)
  vs scrittura della cache di pagina posticipata alla fase 2 (nessuna migrazione, rivisite
  intermedie servite per-unità).

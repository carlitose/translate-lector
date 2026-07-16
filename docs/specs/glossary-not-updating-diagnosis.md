# Diagnosi — Il glossario non si aggiorna più (auto-popolamento del percettore)

## Type

Diagnostic spec

## Status

Confirmed (2026-07-16, triangolazione a 3 diagnosi indipendenti; convergenza 2/3 sul
meccanismo primario con **due repro deterministici**, +1 insight secondario reale;
confidenza alta)

## Symptom

L'auto-popolamento del glossario **non funziona più**: navigando/traducendo nuove pagine non
vengono più aggiunti nuovi termini (la lista del glossario smette di crescere). La traduzione
delle pagine continua a funzionare regolarmente. Ambito confermato con l'utente: solo i termini
**automatici** proposti dal percettore; le modifiche manuali (traduzione/nota/lucchetto) dal
pannello non sono in questo ambito e risultano ancora funzionanti.

## Root Cause

**Il prefetch avvelena la crescita del glossario tramite la cache di pagina.** In
`translate::translate_page` il percettore è l'**unico** produttore di `new_glossary_terms[]`, ed
è cablato per girare *solo* quando la pagina è un cache miss e la navigazione è reale
(`update_context == true`). Il prefetch della pagina N+1 scalda però la cache di pagina, e il
successivo cache-hit sulla navigazione reale ritorna **prima** del blocco percettore.

Sequenza deterministica:

1. Dopo ogni navigazione reale, il frontend fa prefetch di N+1 con `updateContext: false`
   (`src/routes/+page.svelte:450-463`, `prefetchNextPage`). Prefetch attivo di default
   (`settings.rs:40` `DEFAULT_PREFETCH_ENABLED = true`).
2. In `translate_page` con `update_context == false`, il blocco percettore
   (`src-tauri/src/translate.rs:937-984`, guardato da `if p.update_context`) viene saltato, **ma
   `cache_insert` gira comunque incondizionatamente** (`translate.rs:991`). La pagina N+1 entra
   in cache di pagina *senza* aver avanzato il contesto (summary + glossario).
3. Quando l'utente naviga **davvero** su N+1 (`updateContext: true`), il cache-hit di pagina
   ritorna subito (`translate.rs:704-715`) — prima del blocco percettore.
   `glossary::insert_terms_deduped` (`translate.rs:995`, gated da `summary_advanced`) **non viene
   mai raggiunto** → nessun nuovo termine.

Con prefetch ON di default, la lettura sequenziale fa prefetch di ogni pagina prima che venga
visitata: l'auto-popolamento è di fatto morto mentre la traduzione continua (servita da cache).

**Commit che ha attivato la regressione: `d0fe497`** ("fix(translate): il prefetch locale non
viene più cancellato da is_current", diagnosi sorella
[local-prefetch-cancellation-diagnosis.md](local-prefetch-cancellation-diagnosis.md)). Prima di
`d0fe497`, il prefetch locale veniva cancellato all'unità 0 (`Err(Cancelled)`), quindi non
scaldava mai la cache → alla navigazione reale c'era un cache miss → il percettore girava → il
glossario cresceva. `d0fe497` ha reso il prefetch locale efficace (`should_attach_is_current`,
`lib.rs:85`/`627`), trasformando ogni navigazione successiva in un cache-hit che salta il
percettore. Il bug è latente da quando esiste il prefetch (ticket 12) anche su cloud; è diventato
**visibile** perché il provider di default è ora il `llamaserver` locale (`settings.rs:124`) ed è
lì che `d0fe497` ha "acceso" il prefetch.

## Evidence

- `src-tauri/src/translate.rs:704-715` — cache-hit di pagina: ritorna `from_cache: true`
  immediatamente, senza notion di "contesto non ancora avanzato per questa riga".
- `src-tauri/src/translate.rs:937-984` — blocco percettore gated da `if p.update_context`; su
  prefetch (`false`) è interamente saltato.
- `src-tauri/src/translate.rs:991` — `cache_insert` è **fuori** dalla guardia `if
  p.update_context`; solo `set_rolling_summary` + `insert_terms_deduped` (`:993-996`) sono gated
  da `summary_advanced`. Quindi il prefetch scrive la cache di pagina ma non il glossario.
- `src/routes/+page.svelte:450-463` — `prefetchNextPage()` invoca `translate_page` per
  `currentPage + 1` con `updateContext: false`, scaldando la cache prima dell'arrivo.
- `src-tauri/src/settings.rs:40` `DEFAULT_PREFETCH_ENABLED = true`; `:124` `DEFAULT_PROVIDER_ID =
  "llamaserver"`.
- **Due repro deterministici indipendenti** (lenti repro-first e recent-change), stesso test:
  prefetch di pagina P (`update_context=false`) → navigazione on-demand su P
  (`update_context=true`, stesso testo) con mock client il cui percettore propone un termine.
  Risultato: `from_cache == true`, `client.calls() == 0` (percettore mai chiamato), glossario a 0
  invece di 1. Fallimento esattamente sull'asserzione "il glossario dovrebbe essere cresciuto".
- Test esistente `prefetch_caches_translation_without_touching_summary_or_glossary`
  (`translate.rs:~2173`) codifica proprio il fatto che il prefetch scrive la cache di pagina e non
  tocca il glossario — coerente con la catena sopra.
- Bisect: `git show d0fe497` è il trigger della regressione per il provider locale ora di default.

## Decision / Solution

Il cache-hit di pagina non deve corto-circuitare l'avanzamento del contesto quando la
navigazione è reale e quella riga è stata scaldata da un prefetch (contesto non ancora avanzato).

**Opzione B — Il prefetch non scrive la cache di *pagina* (scelta)**: guardare `cache_insert`
(`translate.rs:991`) dietro `if p.update_context`, così il prefetch scalda solo la cache
**per-unità** (STC-09) e non quella di pagina. Alla navigazione reale la pagina è un miss di
pagina → `translate_page` prosegue, le unità vengono servite dalla cache per-unità (nessuna
ri-traduzione, STC-09), il percettore gira una volta, il glossario cresce, e la riga di pagina
viene scritta come "completa". Minimale, nessuna migrazione di schema, sfrutta STC-09.
Endorsed da entrambe le diagnosi con repro.

**Opzione A — Flag `context_advanced` sulla cache di pagina (alternativa)**: aggiungere una
colonna `context_advanced` a `translations_cache` (`db.rs` + `cache_insert`/`cache_lookup`), true
solo quando `update_context && summary_advanced`. Nel ramo del cache-hit, se `update_context ==
true` e la riga ha `context_advanced == false`, non corto-circuitare: girare il percettore-update
una volta (summary + `insert_terms_deduped`), marcare la riga avanzata, servendo comunque il
`translated_text` cachato. Più esplicito e robusto (serve la traduzione senza dipendere
dall'assemblaggio per-unità), ma richiede migrazione di schema e gestione delle righe esistenti.

Entrambe preservano: check riga avvelenata (ticket 16), riuso cache per-unità (STC-09),
immutabilità dei termini locked, e l'invariante "il prefetch non avanza il contesto fuori
ordine".

## Options Considered

- **B (scelta)** — prefetch non scrive la cache di pagina: minimale, zero migrazione, si appoggia
  a STC-09 per evitare la ri-traduzione.
- **A** — flag `context_advanced`: più esplicito ma con costo di schema/migrazione.
- **Scartata — rendere resiliente/robusto il percettore** (vedi insight secondario): risolve un
  problema diverso; NON risolve il primario, perché sulle pagine prefetchate il percettore non
  viene proprio chiamato.

## Evidence — Insight secondario (bug reale, non primario)

STC-10 **ingoia silenziosamente** i fallimenti del perceptor-update (`translate.rs:971-983`, solo
`eprintln!`, `summary_advanced` resta `false`). Anche sulle pagine **non** prefetchate (cache
miss), la chiamata percettore a JSON stretto (`content()` + `parse_perceptor_update`) può fallire
sul modello locale piccolo (budget EC08 / contenuto vuoto/reasoning-only / JSON non conforme anche
dopo il retry di correzione), mentre la traduzione per-unità sopravvive perché usa il fallback
testo-libero robusto (`content_complete()`/`parse_translation`). Effetto: perdita silenziosa di
termini su quelle pagine. È un contributore secondario e un gap di osservabilità, tracciato nel
ticket 02.

## Testing Decisions

- Gap che ha fatto passare il bug: nessun test copre il caso composto "prefetch di N, poi
  navigazione on-demand reale su N che deve avanzare il contesto". I test esistenti verificano il
  prefetch in isolamento (`prefetch_caches_translation_without_touching_summary_or_glossary`) ma
  non la sua interazione con la navigazione reale successiva.
- Aggiungere un test di regressione a livello `translate_page`: prefetch di P
  (`update_context=false`) → navigazione su P (`update_context=true`, stesso testo) ⇒ il percettore
  gira una volta e il glossario cresce, servendo comunque la traduzione dalla cache (nessuna
  ri-traduzione delle unità).

## Follow-up work

- Ticket 01 — Fix primario: avanzare il contesto su navigazione reale anche quando la pagina è
  stata scaldata dal prefetch (opzione B). `docs/tickets/glossary-not-updating/01-*`.
- Ticket 02 — Fix secondario: rendere osservabile/resiliente il fallimento del perceptor-update
  sul provider locale (STC-10). `docs/tickets/glossary-not-updating/02-*`.

## Open Questions

- Nessuna bloccante. Da confermare nel ticket 01: scegliere definitivamente fra opzione B
  (consigliata) e A in base al costo migrazione vs esplicitezza.

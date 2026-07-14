# Traduzione entro un contesto piccolo (~4k) — Wayfinding Spec

## Type

Wayfinding spec

## Status

Active

## Destination

Far funzionare la traduzione di translate-lector **in modo affidabile entro una context window piccola
(~4k token)**, tipica di un LLM locale, ri-architettando il motore in modo **budget-aware**:

- **unità di traduzione piccole** (paragrafo, non pagina intera);
- **summary compatto** e prompt minimale;
- **selezione deterministica del glossario** per unità (solo i termini rilevanti a quel paragrafo, scelti da
  **codice**, non inviando l'intero glossario);
- **più chiamate piccole** riassemblate, mantenendo la coerenza del percettore.

Obiettivo: rimuovere le cause di `EC08` (budget token esaurito) e dei timeout con modelli locali, riducendo
drasticamente la dimensione di ogni prompt.

Contesto: eredita l'epica provider locale
([local-llm-provider-wayfinder.md](./local-llm-provider-wayfinder.md)) e la diagnosi
([local-llm-empty-content-diagnosis.md](./local-llm-empty-content-diagnosis.md)).

## Decisions So Far

- **Strategia budget-aware, non "una chiamata per pagina"** (idea utente, 2026-07-14): dividere la pagina in
  unità piccole entro un budget di token derivato dal context window del provider.
- **Selezione glossario = codice deterministico** (idea utente esplicita: "funzioni programmate per
  selezionare il glossario giusto"): per ogni unità, includere nel prompt solo i termini il cui
  `source_term` compare (con varianti semplici) nel testo dell'unità; i **locked** restano vincolo assoluto.
  Un'eventuale selezione via LLM è alternativa considerata, non preferita.
- **Riuso dell'infra esistente**: `split_into_chunks` (`src-tauri/src/translate.rs:72`), `est_tokens`
  (`src-tauri/src/llm.rs:730`, chars/4 calibrabile), compressione summary (EC05), e `max_tokens`
  per-provider (Ticket 02 empty-content, già fatto) sono i mattoni.
- **Assunzione (esplicita)**: la pipeline chunked/budget-aware si attiva in base al **budget derivato dal
  context** (configurabile). I provider a contesto grande possono usare unità più grandi / il percorso
  attuale; i piccoli (4k locali) usano unità paragrafo + glossario selettivo. Da confermare nel grilling.

## Fatti di codebase rilevanti (grounding)

- Chunking già presente ma **troppo grosso**: `CHUNK_CHAR_THRESHOLD = 8000` char (`translate.rs:31`); i
  chunk sono tradotti in sequenza portando avanti il summary.
- **Il glossario intero** (locked + unlocked, `glossary::render_locked_unlocked`) è iniettato nel prompt a
  ogni chunk (`translate.rs:229-230`) → grande consumo di contesto.
- Contratto per chiamata: JSON `{ translated_text, updated_summary, new_glossary_terms }`
  (`llm.rs` `response_format` ~700) → pesante da produrre per un modello piccolo entro 4k.
- Token: `est_tokens` (chars/4) e `calibrate_chars_per_token` esistono; il summary ha un limite con
  auto-compressione (EC05).

## Not Yet Specified

- **Modello di budget per-chiamata**: formula `budget_input = n_ctx − max_tokens(output) − margine`, e come
  ripartirlo tra system + summary + glossario selezionato + testo unità. Dove n_ctx viene noto/configurato
  (per-provider). → Ticket 01.
- **Chunking a livello paragrafo**: come dividere una pagina in unità (paragrafo/frase) entro il budget,
  riusando la ricostruzione di `src/lib/pdfExtract.ts` e `split_into_chunks`; riassemblaggio corretto. → Ticket 02.
- **Selezione deterministica del glossario**: funzione (chunk + glossario) → sottoinsieme rilevante
  (match `source_term`, case-insensitive, multiword, morfologia semplice), con cap e priorità ai locked;
  garanzia di copertura dei termini locked presenti nel chunk. → Ticket 03.
- **Percettore con molte chiamate piccole**: come mantenere coerenza di summary/glossario senza esplodere
  numero di chiamate/latenza; es. tradurre per-unità con contesto minimo e **aggiornare summary+glossario
  una volta per pagina** (step separato e compatto) vs incrementale. Split del contratto (translate-only vs
  perceptor-update). Granularità di cache (per-unità). → Ticket 04.
- **Decisioni umane**: granularità unità (paragrafo/frase/finestra-N-token), default vs solo-provider-piccoli,
  tolleranza latenza per molte chiamate, severità/cap del match glossario, split delle chiamate. → Ticket 05.

## Out of Scope

- Cambiare provider o modello (resta OpenRouter | locale; qui si cambia **come** si costruiscono i prompt).
- Rimuovere il percettore (summary + glossario restano; si cambia come vengono dosati/selezionati).
- Epica OCR (mappa separata).
- Streaming delle risposte (possibile follow-up per la percezione di velocità, non necessario qui).

## Frontier / Blocking Edges

La frontiera è **indagine**: la ri-architettura ha incognite di fattibilità/qualità. Ordine:

1. **Ticket 01 (research/design) — Modello di budget token** *(ready, foundational)*: definisce il budget
   per-chiamata e la ripartizione; senza, non si dimensionano chunk né glossario.
2. **Ticket 02 (prototype) — Chunking a paragrafo entro budget** *(ready)*: prova che una pagina reale si
   divide in unità piccole traducibili e riassemblabili.
3. **Ticket 03 (prototype) — Selezione deterministica del glossario** *(ready, idea-chiave utente)*: prova
   che scegliere solo i termini rilevanti taglia drasticamente il prompt mantenendo i locked.
4. **Ticket 04 (research/design) — Percettore multi-chiamata + contratto + cache** *(blocked by 01,02,03)*:
   lega tutto; decide split chiamate e granularità cache.
5. **Ticket 05 (grilling) — Decisioni umane** *(ready, gate prima della build)*.

Dopo 01-05: **rivedere la mappa** e derivare i ticket di build verticali (budget → chunking paragrafo →
selezione glossario → chiamate piccole + update percettore per pagina → cache per-unità → reassemble) con
`to-tickets`.

## Ticket Plan

Cartella: `docs/tickets/small-context-translation/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Modello di budget token per-chiamata (da n_ctx) | ready |
| 02 | prototype | Chunking a livello paragrafo entro budget | ready |
| 03 | prototype | Selezione deterministica del glossario per unità | ready |
| 04 | research | Percettore multi-chiamata + split contratto + cache per-unità | blocked by 01,02,03 |
| 05 | grilling | Decisioni: granularità, default, latenza, match glossario | ready (gate) |

## Next Review

Quando 01-05 sono chiusi e 05 è deciso:
1. Ripiegare evidenze/decisioni nella mappa.
2. Aggiornare SPECIFICATION.md §3.2 (unità di traduzione), §3.3 (percettore), §4.4 (contratto) per la
   strategia budget-aware.
3. Derivare i ticket di build verticali con `to-tickets`.

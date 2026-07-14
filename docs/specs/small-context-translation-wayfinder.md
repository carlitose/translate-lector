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
- **Modello di budget token = DEFINITO** (Ticket 01, 2026-07-14; vedi sezione dedicata sotto).
- **Chunking a paragrafo = PROTOTIPO OK** (Ticket 02): `split_into_units(text, budget, ratio)` in
  `translate.rs` (non cablato), round-trip garantito, fallback a frase per paragrafi oltre budget. Paragrafi
  reali ~40-90 token ≪ budget → una chiamata per paragrafo è ampiamente fattibile. **Default = paragrafo**,
  fallback frase. *Finding per la build*: `pdfExtract.linesToText` (`src/lib/pdfExtract.ts`) unisce le righe
  con un singolo `\n` e non emette righe vuote → una pagina ricostruita è **un unico paragrafo**; la build
  deve far emettere separatori di paragrafo (da y-gap tra righe) perché emergano unità-paragrafo vere.
- **Selezione deterministica glossario = PROTOTIPO OK** (Ticket 03, idea chiave): `select_glossary(unit,
  entries, unlocked_cap)` in `glossary.rs` (non cablato); match word-boundary/case-insensitive/multiword +
  morfologia semplice (EN -s, IT vocale finale); **locked sempre inclusi**, cap sugli unlocked.
  **Riduzione token misurata: 98.2%** (1651→29 su glossario da 123 termini). Falsi negativi noti (plurali
  irregolari, invarianti, flessioni) → mitigazione build: indice lemma/alias per termine, morfologia su
  tutte le parole dei multiword. Severità raccomandata: word-boundary + morfologia sull'ultima parola,
  cap unlocked ~10-20, locked uncapped (input al Ticket 05).

## Modello di budget token (Ticket 01)

Parametri (tutti per-provider, riuso `ProviderConfig`/settings dei Ticket 07/08):
- `n_ctx` — context window del provider (nuovo setting per-provider; default locale 4096; opz. letto da
  `/props`/`/v1/models` del server). Per il cloud, molto grande → il budget non vincola.
- `out_unit` — `max_tokens` **per-unità** dedicato al percorso chunked: piccolo (es. 512-768), perché
  tradurre un paragrafo produce poco output. NB: **più piccolo** del `max_tokens` per-pagina odierno
  (local 2048, Ticket 02 empty-content) → libera budget di input **e** riduce il rischio EC08.
- `margine` — cuscinetto per l'imprecisione dell'euristica `chars/4` (`est_tokens`, `llm.rs:730`,
  calibrabile via `calibrate_chars_per_token`): ~15%.

Formula:
```
budget_input = floor( (n_ctx − out_unit) × (1 − margine) )
budget_unit_text = budget_input − est(system_prompt) − est(summary_compatto) − est(glossario_selezionato)
```
`budget_unit_text` è la dimensione massima (in token) di un'unità di traduzione → parametro per il chunking
(Ticket 02). Esempio n_ctx=4096, out_unit=640, margine=0.15 → budget_input ≈ (3456)×0.85 ≈ **2937**; tolti
system (~250) + summary (~300) + glossario selezionato (~120) → **budget_unit_text ≈ 2260 token** per
paragrafo — ampiamente sufficiente per un paragrafo, con margine.

Aggancio codice: calcolare `budget_input`/`budget_unit_text` in `translate.rs` prima del loop di chunk,
usando `est_tokens` per system/summary/glossario; sostituire la soglia fissa `CHUNK_CHAR_THRESHOLD`
(`translate.rs:31`) con un limite derivato dal budget (in token, non char). Il summary resta sotto il suo
limite (EC05); se il budget è stretto, ridurre prima il glossario selezionato (Ticket 03), poi il summary.

- **Decisioni strategiche = CONFERMATE** (Ticket 05, gate risolto 2026-07-14;
  [decision-brief-stc-05](./decision-brief-stc-05.md)): D1 unità = **paragrafo** (fallback frase); D2
  attivazione **condizionale sul budget** (cloud degrada a pagina intera); D3 latenza **accettabile** con
  cache+prefetch; D4 match glossario **bilanciato** (word-boundary + morfologia ultima parola, cap unlocked
  10-20, locked uncapped); D5 **split contratto** (translate-only per unità + perceptor-update per pagina);
  D6 update percettore **una volta per pagina**.

## Fatti di codebase rilevanti (grounding)

- Chunking già presente ma **troppo grosso**: `CHUNK_CHAR_THRESHOLD = 8000` char (`translate.rs:31`); i
  chunk sono tradotti in sequenza portando avanti il summary.
- **Il glossario intero** (locked + unlocked, `glossary::render_locked_unlocked`) è iniettato nel prompt a
  ogni chunk (`translate.rs:229-230`) → grande consumo di contesto.
- Contratto per chiamata: JSON `{ translated_text, updated_summary, new_glossary_terms }`
  (`llm.rs` `response_format` ~700) → pesante da produrre per un modello piccolo entro 4k.
- Token: `est_tokens` (chars/4) e `calibrate_chars_per_token` esistono; il summary ha un limite con
  auto-compressione (EC05).

## Design multi-chiamata + contratto + cache (Ticket 04)

Sintesi di 01/02/03 in una pipeline per-pagina budget-aware:

**Orchestrazione (per pagina):**
1. `split_into_units(page_text, budget_unit_text, ratio)` (Ticket 02) → unità paragrafo (fallback frase).
2. Per ogni unità **mancante in cache**, una chiamata **translate-only**: prompt = system minimale + summary
   **read-only compatto** + `select_glossary(unit, entries, cap)` (Ticket 03, locked-first) + testo unità;
   `max_tokens = out_unit` piccolo (Ticket 01). Output: **solo** il testo tradotto.
3. Riassemblaggio delle unità in ordine → traduzione di pagina.
4. **Una** chiamata **perceptor-update** per pagina (solo su navigazione reale, non prefetch): input =
   testo pagina (o le unità) + summary corrente + glossario; output = `{ updated_summary, new_glossary_terms }`
   con compressione EC05. Salta del tutto quando `update_context=false` (prefetch, come oggi).

**Split del contratto** (vs l'attuale unico `{translated_text, updated_summary, new_glossary_terms}`):
- `TranslateUnit` → `{ translated_text }` (JSON minimo o testo puro + fallback extraction). Prompt piccolo =
  meno budget e meno rischio EC08.
- `PerceptorUpdate` → `{ updated_summary, new_glossary_terms }`. Una sola volta per pagina.
- Vantaggio: le N chiamate pesanti diventano N chiamate leggere + 1 percettore; il grosso del contesto
  (glossario intero, contratto ricco) sparisce dalle chiamate di traduzione.

**Cache (granularità unità):** estendere `translations_cache` (§4.3) con `unit_index` e un hash del
`source_text` dell'unità (chiave `document_id, page_number, unit_index, target_language`), oppure nuova
tabella `unit_translations`. Riassemblaggio per `unit_index`. Cache **parziale** ammessa: si traducono solo
le unità mancanti (utile su errori/timeout a metà pagina). La cache per-pagina esistente può restare come
vista derivata o essere sostituita — decisione di build.

**Coerenza:** il summary è passato read-only a tutte le unità della pagina (stessa versione); l'update del
summary avviene dopo le unità, una volta. Cross-pagina invariato (summary portato avanti).

**Costo/latenza:** N chiamate piccole + 1 percettore vs 1 grande. Su modello locale lento il numero di
round-trip cresce, ma ogni chiamata è breve e a basso rischio (niente EC08/timeout da prompt enorme); la
cache per-unità riduce il rework. Streaming = follow-up per la percezione di velocità (fuori scope).

## Not Yet Specified

- ~~Modello di budget per-chiamata~~ → **DEFINITO dal Ticket 01** (sezione "Modello di budget token").
- ~~Chunking a livello paragrafo~~ → **PROTOTIPO OK dal Ticket 02** (vedi Decisions So Far). *Aperto per la
  build*: far emettere i separatori di paragrafo in `pdfExtract.ts` (da y-gap).
- ~~Selezione deterministica del glossario~~ → **PROTOTIPO OK dal Ticket 03** (98.2% riduzione). *Aperto per
  la build*: indice lemma/alias per ridurre i falsi negativi morfologici.
- ~~Percettore con molte chiamate piccole~~ → **PROGETTATO dal Ticket 04** (sezione "Design multi-chiamata").
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
| 01 | research | Modello di budget token per-chiamata (da n_ctx) | ✅ done (`done/`) — sezione "Modello di budget token" |
| 02 | prototype | Chunking a livello paragrafo entro budget | ✅ done (`done/`) — `split_into_units`, round-trip OK |
| 03 | prototype | Selezione deterministica del glossario per unità | ✅ done (`done/`) — `select_glossary`, −98% token |
| 04 | research | Percettore multi-chiamata + split contratto + cache per-unità | ✅ done (`done/`) — sezione "Design multi-chiamata" |
| 05 | grilling | Decisioni: granularità, default, latenza, match glossario | ✅ done (`done/`) — D1-D6 confermate |

## Next Review

Quando 01-05 sono chiusi e 05 è deciso:
1. Ripiegare evidenze/decisioni nella mappa.
2. Aggiornare SPECIFICATION.md §3.2 (unità di traduzione), §3.3 (percettore), §4.4 (contratto) per la
   strategia budget-aware.
3. Derivare i ticket di build verticali con `to-tickets`.

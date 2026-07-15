# Latenza traduzione locale — Wayfinding Spec

## Type

Wayfinding spec

## Status

**Completed** (2026-07-15 — build 13/04/06 mergiata in main, destinazione raggiunta; resta solo la
verifica GUI del ticket 10 di local-llm-provider, HITL)

## Destination

Rendere la **prima passata** di traduzione con provider locale (llama-server dietro Unsloth Studio,
`localhost:8888`) abbastanza veloce e robusta da **non andare mai in timeout** su pagine dense.
Target confermato dal grilling (decisione L5, [decision-brief-latency-03.md](./decision-brief-latency-03.md)):
**pagina densa a freddo ≤2 minuti, zero timeout**, più una lettura sequenziale fluida grazie al prefetch
serializzato. Il target originario "<10 s" non è raggiungibile con l'hardware/modello attuali (floor
misurato ~40 s solo di decode) ed è stato abbandonato.

La pipeline small-context (STC) ha risolto EC08 e i prompt enormi, ma ha introdotto il problema opposto:
**troppe chiamate piccole sequenziali**, ognuna con grande overhead fisso di prefill. Questa mappa governa
la riduzione della latenza senza regredire sulla robustezza conquistata da STC.

Contesto: eredita [small-context-translation-wayfinder.md](./small-context-translation-wayfinder.md) e la
diagnosi [local-translation-latency-diagnosis.md](./local-translation-latency-diagnosis.md).

## Decisions So Far

- **Diagnosi = RIVISTA DALLE MISURE** (2026-07-14, [local-translation-latency-diagnosis.md](./local-translation-latency-diagnosis.md),
  aggiornata dopo il Ticket 01): C2 (una chiamata per paragrafo), C4 (retry annidati) e C5 (nessuna
  cancellazione + prefetch concorrente) confermate; **C1 corretta** (il timeout è il default di **30 s**
  di `reqwest::blocking::Client`, non il proxy) e **C3 smentita** (il server fa prefix caching:
  l'overhead fisso costa ~0 entro la pagina).
- **Baseline misurata = FATTA** (Ticket 01, `done/01-research-latency-baseline.md`): decode ~20-27 tok/s;
  il collo di bottiglia è il **CoT del modello** (~500 token di "Thinking Process" scartati per OGNI
  chiamata → ~30 s per paragrafo, al pelo del timeout di 30 s). La soppressione via API
  (`reasoning_effort`, `chat_template_kwargs`, `/no_think`, istruzioni) **non funziona** su
  gemma-4-E2B-it-qat. Floor fisico ≈ 40 s per pagina densa fredda → il target "<10 s" va
  ridimensionato in L5.
- **Il fix del timeout (ticket 13) è PREREQUISITO del packing**: una finestra impacchettata dura 42-50 s
  > 30 s → col timeout attuale il packing peggiorerebbe il sintomo. Ordine obbligato: 13 prima di 04.
- **La leva più grande è il modello, non la pipeline**: un modello di traduzione senza reasoning
  (GemmaX2-28-2B, già nella libreria del server, `loaded:false`) eliminerebbe il CoT (~10× su unità
  piccole). Il proxy serve silenziosamente il modello caricato ignorando il `model` richiesto → il test
  richiede load manuale in Unsloth Studio (HITL → grilling 03, nuova decisione L6).
- **Packing = PROTOTIPO OK** (Ticket 02, `done/02-prototype-paragraph-packing.md`): `pack_units` in
  `translate.rs` (non cablato), composto dopo `split_into_units`; round-trip esatto, 18 paragrafi →
  1-2 finestre (≥9×), suite 210/210 verde. **Analisi cache misurata**: finestre a budget dinamico =
  0 stabili dopo un repack 900→700 (cache azzerata); finestre a **taglia fissa** (`PACK_TARGET_TOKENS ≈
  512`, clampata al budget solo se più stretto) = stabili per costruzione, chiave `(unit_index,
  source_hash)` attuale riutilizzabile senza migrazioni. **Raccomandazione per L1: taglia fissa.**
- **Il timeout applicativo per-provider ha già un ticket**:
  [13-local-inference-timeout.md](../tickets/local-llm-provider/13-local-inference-timeout.md) (aperto,
  ready, epica local-llm-provider). Questa mappa **non lo duplica**; il grilling qui decide solo la
  policy retry-on-timeout locale che quel ticket lascia aperta.
- **Ereditate da STC** (decision-brief-stc-05): D1 unità = paragrafo; D5 split contratto
  (translate-only + perceptor); D6 perceptor una volta per pagina. Il packing di più paragrafi per
  finestra **rimette in discussione D1** (unità di *chiamata* ≠ unità di *cache*?) → decisione umana nel
  grilling.
- **Assunzione (esplicita)**: il collo di bottiglia dominante è il **prefill ripetuto** (C2+C3), non il
  decode. Da validare con misure reali (Ticket 01) prima di investire nella build.
- **Grilling 03 = DECISO** (2026-07-14, `done/03-grilling-latency-decisions.md`,
  [decision-brief-latency-03.md](./decision-brief-latency-03.md)): sei decisioni L1-L6.
  - **L6 — Modello: resta gemma-4-E2B-it-qat.** GemmaX2-28-2B validato in sessione (caricato in Unsloth
    Studio, `ctx=8192`): ~9× più veloce (2.5-4 s/chiamata) ma **incompatibile con la pipeline** — output
    vuoto/non tradotto col prompt app-like (system+summary+glossario, richiesto dal perceptor), fonde i
    paragrafi su finestre multiple col formato canonico che invece funziona. Glossario locked e
    perceptor sono requisiti non negoziabili → nessun cambio modello. Il CoT di gemma-4 resta accettato,
    mitigato dal packing (L1).
  - **L1 — Packing a taglia fissa**: `PACK_TARGET_TOKENS = 512` costante (non derivata dal budget
    dinamico), clampata al budget se più stretto. Rivede D1 di STC: unità di **chiamata** = finestra;
    unità di **split** resta il paragrafo.
  - **L2 — Cap summary: NON implementato**, ticket 05 chiuso (prefix caching annulla il guadagno).
  - **L3 — Prefetch locale serializzato**, priorità on-demand, cede al confine di finestra (non
    disattivato).
  - **L4 — Retry-on-timeout locale: 0 retry**, fail-fast; altri transient restano ×3; cloud invariato.
  - **L5 — Target: ≤2 min a freddo su pagina densa, zero timeout** + prefetch fluido in lettura
    sequenziale (vedi Destination).

## Not Yet Specified

- ~~Ripartizione reale prefill/decode~~ → **MISURATA dal Ticket 01**.
- ~~Semantica cache col packing~~ → **RISOLTA dal Ticket 02**.
- ~~Resa reale di GemmaX2-28-2B~~ → **VALIDATA nel grilling 03**: incompatibile con la pipeline (L6),
  nessun cambio modello.
- ~~Decisioni umane L1-L6~~ → **TUTTE DECISE** nel grilling 03 (vedi Decisions So Far).
- ~~Riaprire L6 in futuro~~ → **SUPERATA** (2026-07-15): non serve cambiare modello — bypassando il
  proxy di Studio, llama-server diretto con `--reasoning off` elimina il CoT di gemma-4 (pagina
  densa ~21 s vs ~99 s). Nuova mappa: [llamacpp-direct-wayfinder.md](./llamacpp-direct-wayfinder.md).
- **Conferma e2e del ticket 04**: la stima "CoT pagato 1-2 volte per pagina invece di 18" è su finestre
  sintetiche; va verificata su pagine reali durante la build (criterio d'accettazione L5).

## Out of Scope

- Il timeout esplicito per-provider e il messaggio d'errore locale azionabile (= ticket 13 esistente,
  epica local-llm-provider).
- Streaming delle risposte (follow-up di percezione, non riduce il lavoro del server).
- Cambiare modello/quantizzazione o parametri del server (n_gpu_layers, ecc.) — mitigazione utente, non
  dell'app.
- **Cambio del modello di traduzione locale** (GemmaX2 o simili): validato e scartato (L6) — richiederebbe
  ripensare glossario/perceptor per un modello senza prompt system, fuori scope di questa mappa.
- Cap del summary translate-only (L2): valutato e chiuso, nessun guadagno misurato.
- Parallelizzare le chiamate verso il server locale (mono-modello: la concorrenza non aiuta, anzi — vedi C5).
- Epica OCR (mappa separata).

## Frontier / Blocking Edges

1. **Ticket 13 (timeout esplicito) è l'unico prerequisito rimasto**: le finestre impacchettate durano
   42-50 s > del default di 30 s di `reqwest::blocking` — cablare il packing (Ticket 04) senza il
   timeout configurabile peggiorerebbe il sintomo. Il ticket 13 va aggiornato con la correzione di C1
   (il taglio è il default dell'app, non il proxy: `Client::builder().timeout(...)` è il fix certo) e
   con la policy L4 (0 retry sul timeout locale).
2. **Build** *(Ticket 04, 06 — ready non appena il 13 è chiuso; 05 già chiuso)*: packing cablato a
   `PACK_TARGET_TOKENS = 512` (L1), serializzazione prefetch con priorità on-demand (L3) + cancellazione
   job stantii.

## Ticket Plan

Cartella: `docs/tickets/local-translation-latency/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Baseline misurata: prefill/decode e secondi per chiamata sul server locale | ✅ done (`done/`) — C3 smentita, C1 corretta, CoT è il collo di bottiglia |
| 02 | prototype | Packing di paragrafi per finestra entro budget + semantica cache | ✅ done (`done/`) — `pack_units`, ≥9×, raccomandata taglia fissa |
| 03 | grilling | Decisioni: packing/D1, cambio modello (L6), prefetch locale, retry-on-timeout, target | ✅ done (`done/`) — L1-L6 tutte decise |
| 04 | task | Cablare il packing nella pipeline (`PACK_TARGET_TOKENS=512`) | ✅ done (`done/`) — **misura reale: 18 chiamate/~9 min → 2 chiamate/99 s, zero timeout, L5 PASS** |
| 05 | task | Cap del summary nelle chiamate translate-only | ✅ done (`done/`) — **chiuso senza implementazione** (L2) |
| 06 | task | Serializzare prefetch (priorità on-demand) + cancellare i job stantii | ✅ done (`done/`) — slot singolo + cursore, serializzazione dimostrata live |

Anche il ticket 13 (timeout, epica local-llm-provider) è ✅ done. Tutto mergiato in main il 2026-07-15
(commit `6b645e8`), suite 239/239 verde.

Fuori cartella ma correlato: [13-local-inference-timeout.md](../tickets/local-llm-provider/13-local-inference-timeout.md)
(ready, **prioritario**: è il fix diretto del sintomo e prerequisito del ticket 04; la sua ipotesi
"timeout del proxy" va corretta col dato del Ticket 01 — default 30 s del client blocking — e integrata
con la policy L4, 0 retry sul timeout locale).

## Next Review

Grilling chiuso (2026-07-14); prossimo passo esecutivo: risolvere il ticket 13, poi eseguire 04 e 06
(indipendenti tra loro, 06 può partire subito).

Dopo la build (04, 06):
1. Misura di conferma con lo stesso protocollo del Ticket 01 (stessa pagina densa, prima/dopo) e verifica
   esplicita del target L5 (≤2 min a freddo, zero timeout).
2. Aggiornare `SPECIFICATION.md` §3.2 se D1 cambia definitivamente (unità di chiamata = finestra da 512
   token, non più il singolo paragrafo).
3. Se in futuro emerge un modello di traduzione locale compatibile con prompt system + glossario/summary,
   riaprire L6 come nuova voce di Not Yet Specified. Nota: L6 come *problema di latenza* è **chiuso**
   (vedi Not Yet Specified) — risolto bypassando il proxy di Studio con llama-server diretto
   (`--reasoning off`), **non** cambiando modello. Mappa: [llamacpp-direct-wayfinder.md](./llamacpp-direct-wayfinder.md).

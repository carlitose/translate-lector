# 03 — Grilling: decisioni su packing/D1, cap summary, prefetch locale, retry, target

## Parent Spec

[local-translation-latency-wayfinder.md](../../specs/local-translation-latency-wayfinder.md)

## Type

grilling

## Outcome

Decisioni umane registrate (decision brief in `docs/specs/decision-brief-latency-03.md`) che sbloccano i
ticket di build 04-06:

- **L1 — Packing**: attivare il packing di paragrafi per finestra? Rivede D1 di STC: l'unità di *chiamata*
  diventa la finestra impacchettata. **Raccomandazione dal Ticket 02: taglia FISSA**
  (`PACK_TARGET_TOKENS ≈ 512`, clampata al budget solo se più stretto) — cache stabile ai repack
  (misurato 2/2 vs 0 col budget dinamico), chiave attuale riutilizzabile, ~9× meno chiamate.
- **L2 — Cap summary**: DECLASSATA dalle misure del Ticket 01 (il prefisso è prefix-cached: nessun
  guadagno di latenza). Resta solo come leva per allargare `budget_unit_text`: decidere se farla o
  chiudere il ticket 05.
- **L3 — Prefetch su provider locale**: serializzato dietro le richieste on-demand, disattivato di
  default, o invariato?
- **L4 — Retry-on-timeout locale**: 0 retry (fail-fast con messaggio azionabile) o mantenere ×3?
  (coordinare con il ticket 13 di local-llm-provider, che implementa il timeout esplicito; nota dal
  Ticket 01: il timeout osservato è il default di 30 s del client blocking, non il proxy).
- **L5 — Target di latenza**: il floor fisico misurato è ~40 s per pagina densa fredda (decode 20-27
  tok/s) — il "<10 s" della mappa non è raggiungibile senza cambiare modello/hardware: quale soglia è
  accettabile con cache + prefetch?
- **L6 — Cambio modello (NUOVA, la leva più grande)**: gemma-4-E2B-it-qat genera ~500 token di CoT per
  chiamata, insopprimibile via API (misurato). Caricare GemmaX2-28-2B (modello di traduzione senza
  reasoning, già in libreria) in Unsloth Studio e validare qualità/velocità (HITL: il proxy ignora il
  `model` richiesto e serve quello caricato). Se la qualità regge, ~10× su unità piccole.

## Esito (2026-07-14)

Sei decisioni prese, registrate in [decision-brief-latency-03.md](../../specs/decision-brief-latency-03.md):

- **L6 — Modello: resta gemma-4-E2B-it-qat.** GemmaX2-28-2B validato in sessione (caricato in Unsloth
  Studio): ~9× più veloce ma **incompatibile con la pipeline** — output vuoto/non tradotto col prompt
  app-like (system+summary+glossario), fonde i paragrafi su finestre multiple col formato canonico. Il
  glossario locked e il perceptor sono requisiti non negoziabili → nessun cambio modello.
- **L1 — Packing a taglia fissa**: `PACK_TARGET_TOKENS = 512`, clampato al budget se più stretto. Rivede
  D1 (unità di chiamata = finestra; unità di split = paragrafo).
- **L2 — Cap summary: ticket 05 chiuso**, nessuna implementazione (prefix caching già annulla il costo).
- **L3 — Prefetch locale serializzato**, priorità on-demand, cede al confine di finestra.
- **L4 — Retry-on-timeout locale: 0 retry**, fail-fast; altri transient restano ×3; cloud invariato.
- **L5 — Target: ≤2 min a freddo su pagina densa, zero timeout** + prefetch fluido in lettura sequenziale
  (hit-rate ≥80% atteso, non bloccante per l'accettazione tecnica). Il "<10 s" originario abbandonato.

## Acceptance Criteria

- [x] Ogni decisione L1-L6 registrata con motivazione nel decision brief e linkata dalla mappa.
- [x] La mappa aggiornata: Decisions So Far, Not Yet Specified ripulita, ticket 04-06 confermati o
      ridimensionati (05 chiuso).

## Blocked By

- None — eseguito.

## Frontier

Gate umano prima della build: L1 rivede una decisione STC esistente (D1) e L3/L4 cambiano comportamenti
visibili all'utente. Costruire prima di decidere rischia rework.

## Work Plan

1. Preparare per ogni decisione un riassunto di 3-5 righe con le evidenze di 01/02 e l'opzione
   raccomandata.
2. Porre le domande una alla volta (stile grilling), registrando risposta e motivazione.
3. Scrivere `docs/specs/decision-brief-latency-03.md` e aggiornare la mappa.

## Evidence to Capture

- Risposte dell'utente, decision brief, diff della mappa.

## Out of Scope

- Implementazione di qualsiasi decisione (ticket 04-06).
- Le decisioni già di competenza del ticket 13 (valore del timeout, messaggio d'errore).

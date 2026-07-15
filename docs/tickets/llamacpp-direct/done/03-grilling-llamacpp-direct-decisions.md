# 03 — Grilling: decisioni su distribuzione, modello, preset e parametri

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Status

**Done** (2026-07-15). Decisioni D0-D5 registrate in
[decision-brief-llamacpp-direct-03.md](../../specs/decision-brief-llamacpp-direct-03.md). Sintesi:
uso personale (niente bundling/firma); app-managed spawn/kill; path binario+GGUF espliciti in ⚙️ con
default; preset `unsloth` mantenuto normale; parametri `8080/-ngl 99/-c 4096/--reasoning off/
--parallel 1`; default = llama.cpp diretto con spawn on-demand. Ticket 04-06 aggiornati con le scelte.

## Type

grilling

## Outcome

Decisioni umane registrate (decision brief in `docs/specs/decision-brief-llamacpp-direct-03.md`)
su:

- **D1 — Distribuzione**: sidecar impacchettato nell'installer vs binario gestito in app-data
  (scaricato/copiato al primo uso) vs solo script/docs per lancio manuale.
- **D2 — Modello GGUF**: riuso della cache HuggingFace esistente vs download gestito dall'app vs
  path configurabile in ⚙️ (e cosa succede se il file manca).
- **D3 — Preset `unsloth`**: tenerlo, rimuoverlo, o tenerlo deprecato; migrazione di eventuali
  override utente.
- **D4 — Parametri default del server**: porta (8080?), `-ngl`, `-c` (allineato a n_ctx 4096),
  `--reasoning off` sempre?, `--parallel`.
- **D5 — Come rendere il provider diretto il default consigliato**: il prerequisito di qualità è
  soddisfatto (ticket 07 HITL chiuso positivo, 2026-07-15). Resta da decidere *come*: solo
  raccomandazione nel README vs default automatico all'avvio, in funzione di D1-D4.

## Acceptance Criteria

- [ ] Ogni decisione D1-D5 ha una scelta esplicita, la motivazione e le alternative scartate.
- [ ] Decision brief scritto e linkato dalla mappa; "Not Yet Specified" della mappa ripulita.
- [ ] I ticket di build 04-06 sono aggiornati/ri-scoperti in base alle scelte.

## Blocked By

- ~~[01-research-binary-sourcing.md](./done/01-research-binary-sourcing.md)~~ → **done** (release
  ufficiale llama.cpp CUDA, ~1.1 GB).
- ~~[02-research-tauri-sidecar-contract.md](./done/02-research-tauri-sidecar-contract.md)~~ →
  **done** (externalBin + plugin-shell + kill via RunEvent).
- **Nessun blocco residuo: ready per il grilling.**

## Frontier

Le decisioni D1-D2 cambiano radicalmente la forma dei ticket di build (installer vs downloader vs
docs): procedere senza grilling significherebbe costruire al buio.

## Work Plan

1. Presentare le evidenze dei ticket 01-02 (tabella binari, contratto Tauri).
2. Una domanda alla volta (skill grilling), con raccomandazione per ciascuna.
3. Scrivere il decision brief e aggiornare mappa + ticket di build.

## Evidence to Capture

- Risposte dell'utente, trade-off accettati, assunzioni residue.

## Out of Scope

- Implementazione (ticket 04-06).

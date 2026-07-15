# Provider locale diretto llama.cpp (rimozione Unsloth Studio) — Wayfinding Spec

## Type

Wayfinding spec

## Status

**Build completata** (2026-07-15) — tutti i 7 ticket in `done/`, suite 274 verde, 3 PR di build
aperti (impilati). Restano solo: merge dei PR + verifica e2e GUI del ticket 04.

## Destination

L'app usa **llama-server (llama.cpp) direttamente** come provider locale, senza Unsloth Studio in
mezzo: niente proxy, niente auto-unload del modello, niente token, e soprattutto **CoT soppresso
alla fonte** (`--reasoning off`). Il server è gestito dall'app (sidecar Tauri) o, come minimo, da un
lancio a un click documentato. Risultato misurato da raggiungere in produzione: **pagina densa a
freddo ~20-30 s** (vs ~99 s via Studio col packing, ~9 min prima del packing), zero timeout, zero
"No model loaded".

Eredita e supera la mappa [local-translation-latency-wayfinder.md](./local-translation-latency-wayfinder.md)
(Completed): quella mappa aveva accettato il CoT come insopprimibile (L6) perché tutti i tentativi
passavano dal proxy di Studio. L'esperimento del 2026-07-15 ha dimostrato che il proxy era il
problema, non il modello.

## Decisions So Far

- **Esperimento chiave = FATTO** (2026-07-15, in sessione, protocollo identico a
  `measure_ticket04.py`): llama-server build 9987 (compilato da Unsloth, CUDA, già sul disco in
  `~/.unsloth/llama.cpp/build/bin/Release/llama-server.exe`) lanciato direttamente con
  `--reasoning off` su gemma-4-E2B-it-qat-UD-Q4_K_XL:
  - **CoT completamente eliminato** (`reasoning_content` vuoto, `completion_tokens` = sola
    traduzione). La conclusione del ticket 01 latenza ("soppressione via API non funziona") vale
    solo *attraverso il proxy di Studio*.
  - Paragrafo singolo app-like: **3.4 s / 46 tok** (baseline Studio: 29.7 s / 559 tok) ≈ 9×.
  - Pagina densa (2 finestre da 512): **~21 s** con GPU (CPU-only: ~58 s; Studio+packing: ~99 s).
  - Modello caricato in **4 s**, resta residente (vs minuti + auto-unload di Studio).
- **Dipendenza DLL CUDA**: il binario Unsloth da solo NON vede la GPU (`--list-devices` vuoto);
  funziona mettendo nel PATH le DLL CUDA 13 del venv di Studio
  (`~/.unsloth/studio/unsloth_studio/Lib/site-packages/torch/lib`). Fragile: dipende
  dall'installazione di Studio che vogliamo rimuovere.
- **Sourcing binario = RISOLTO** (2026-07-15, ticket 01, `done/`): la **release ufficiale llama.cpp
  Windows CUDA** (b10016, build cuda-12.4) vede la GPU standalone (cudart affiancato, nessuna DLL di
  Studio) con resa identica (3.5 s/paragrafo, ~20 s/pagina densa, zero CoT). MIT, aggiornamento via
  tag GitHub. **Costo: ~1.1 GB** di runtime CUDA (ggml-cuda 509 MB + cublasLt 452 + cublas 95) —
  scelto build 12.4 perché il driver arriva a CUDA 13.2. Il build Unsloth è escluso (è la dipendenza
  da rimuovere); build proprio slim (solo sm_89) = opzione futura se la dimensione diventa il
  vincolo. → decisione bundle-vs-download al grilling (D1).
- **Contratto sidecar Tauri = RISOLTO** (2026-07-15, ticket 02, `done/`): `externalBin` per l'exe
  (suffisso target-triple obbligatorio) + DLL CUDA via `bundle.resources` o, più robusto,
  `current_dir`/`PATH` risolto in Rust da `resource_dir()`. Lifecycle con `tauri-plugin-shell`
  (da aggiungere): spawn `app.shell().sidecar(...)`, kill deterministico via
  `.build()?.run(|_, RunEvent::Exit|ExitRequested| child.kill())` con `CommandChild` in managed
  state (stesso pattern di `CurrentPage`/`LocalProviderSlot` già presenti). Health = riuso di
  `probe_reachable` (`llm.rs:809`). Orfani su hard-crash → pid persistito o Job Object (rischio per
  il grilling). Permesso `shell:allow-execute` col sidecar in `capabilities/default.json`.
- **L'app è già pronta lato client**: esiste il preset `llamaserver`
  (`http://127.0.0.1:8080/v1/chat/completions`, `settings.rs:229-236`), e `is_local_url`
  (`llm.rs:775-788`) copre `127.0.0.1` → timeout 180 s, 0 retry-on-timeout, serializzazione
  prefetch e cancellazione job stantii si applicano senza toccare codice. `n_ctx` default 4096 =
  `-c 4096` del server. Nessuna API key richiesta (llama-server senza `--api-key` ignora l'header).
- **Ereditate dalla mappa latenza** (decision-brief-latency-03): L1 packing 512 (in main), L3
  serializzazione (in main), L4 0 retry (in main), L5 target ≤2 min ora largamente battuto. L6
  ("resta gemma-4") resta valida — qui NON si cambia modello, si cambia il modo di servirlo.
- **Qualità senza reasoning = VALIDATA** (2026-07-15, ticket 07 HITL, `done/`): l'utente ha letto
  pagine reali col provider `llamaserver` + `--reasoning off` e ha giudicato la traduzione "molto
  buona". Nessuna regressione bloccante rispetto alla resa con CoT. Lo scivolone sintetico sugli
  articoli non si è ripresentato in pratica.
- **Grilling 03 = DECISO** (2026-07-15, `done/`,
  [decision-brief-llamacpp-direct-03.md](./decision-brief-llamacpp-direct-03.md)): sei decisioni
  D0-D5.
  - **D0 — Uso personale**: niente bundling/firma/download-manager; l'app lancia binario e modello
    già sul disco. Se un giorno si distribuirà, D1/D2 si riaprono.
  - **D1 — App-managed**: spawn on-demand + kill via `RunEvent`; binario e modello a path locali
    configurabili, non impacchettati.
  - **D2 — Path espliciti in ⚙️** (binario + GGUF) con default precompilati; errore azionabile se
    mancano; niente auto-detect (hash di snapshot HF fragile).
  - **D3 — Preset `unsloth` mantenuto** come opzione normale (non deprecato, nessuna migrazione).
  - **D4 — Parametri**: `8080`, `-ngl 99`, `-c 4096`, `--reasoning off`, `--parallel 1`, overridabili.
  - **D5 — Default = llama.cpp diretto, spawn on-demand** (prima traduzione, non all'avvio).

## Not Yet Specified

- ~~Sourcing del binario~~ → **RISOLTO** (ticket 01): release ufficiale llama.cpp CUDA.
- ~~Contratto sidecar Tauri 2~~ → **RISOLTO** (ticket 02): externalBin + plugin-shell + kill via
  RunEvent.
- **Distribuzione** (grilling 03, D1): sidecar impacchettato nell'installer (**+~1.1 GB** di runtime
  CUDA) vs "gestito ma esterno" (l'app scarica il pacchetto binari da GitHub al primo uso) vs build
  proprio slim (solo sm_89). Trade-off misurato: dimensione installer vs semplicità offline.
- **Gestione del modello GGUF** (grilling 03 + ticket 05): puntare alla cache HuggingFace esistente
  (`~/.cache/huggingface/hub/...Q4_K_XL.gguf`, 2.5 GB) vs download gestito dall'app vs path
  configurabile in ⚙️.
- **Destino del preset `unsloth`** (grilling 03): tenerlo per chi usa Studio, o rimuoverlo/migrare
  le impostazioni utente.
- **Parametri server di default** (grilling 03): porta (8080 = preset), `-ngl`, `-c`,
  `--reasoning off`, eventuale `--parallel 1` (oggi n_slots=4 con kv unificata).

## Out of Scope

- Cambio del modello di traduzione (L6 resta chiusa: gemma-4-E2B-it-qat).
- Modifiche alla pipeline di traduzione (packing, serializzazione, cache: già in main).
- Streaming delle risposte.
- Supporto ad altri backend locali (LM Studio, Ollama): i preset restano, ma questa mappa gestisce
  solo llama-server.
- Epica OCR (mappa separata).

## Frontier / Blocking Edges

1. ~~Sourcing del binario (ticket 01)~~ → **RISOLTO**: release ufficiale llama.cpp CUDA.
2. ~~Contratto sidecar Tauri (ticket 02)~~ → **RISOLTO**: externalBin + plugin-shell + kill via RunEvent.
3. ~~Grilling 03~~ → **DECISO** (D0-D5, decision brief).
4. ~~Build (ticket 04, 05, 06)~~ → **FATTA** (2026-07-15, autopilot): tre PR aperti su branch
   impilati (`autopilot/05-model-management` → `04-sidecar-lifecycle` → `06-default-and-docs`),
   suite 274 verde. Non ancora mergiati in main.
5. ~~**Qualità HITL** (ticket 07)~~ → **CHIUSA**: qualità "molto buona" senza reasoning (2026-07-15).
6. **Unica frontiera rimasta = verifica e2e umana con GUI** dei comportamenti runtime del ticket 04
   (spawn reale on-demand, kill senza orfani alla chiusura, reap, reuse) — non verificabili AFK;
   checklist nel ticket `done/04-task-sidecar-lifecycle.md`. + merge dei PR.

## Ticket Plan

Cartella: `docs/tickets/llamacpp-direct/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Sourcing del binario llama-server (Unsloth build vs release ufficiale) | ✅ done (`done/`) — release ufficiale CUDA, ~1.1 GB |
| 02 | research | Contratto sidecar Tauri 2 (spawn/kill, env, bundling, porta) | ✅ done (`done/`) — externalBin + plugin-shell + RunEvent |
| 03 | grilling | Decisioni: distribuzione, modello GGUF, preset unsloth, parametri default | ✅ done (`done/`) — D0-D5, decision brief |
| 04 | task | Sidecar lifecycle: spawn/health/kill di llama-server dall'app | ✅ done (`done/`) — PR `autopilot/04-sidecar-lifecycle`, e2e GUI da confermare |
| 05 | task | Gestione path binario + GGUF (default espliciti) | ✅ done (`done/`) — PR `autopilot/05-model-management` |
| 06 | task | Preset unsloth invariato + default llamaserver + docs | ✅ done (`done/`) — PR `autopilot/06-default-and-docs` |
| 07 | task (HITL) | Validazione qualità traduzione senza reasoning su pagine reali | ✅ done (`done/`) — qualità "molto buona", D5 risolto |

## Next Review

**Tutti i ticket sono in `done/`; build completata dall'autopilot** (2026-07-15). Restano due passi
umani:

1. **Merge dei PR** (impilati, in quest'ordine): `autopilot/05-model-management` →
   `autopilot/04-sidecar-lifecycle` → `autopilot/06-default-and-docs` (base: il branch della mappa
   `docs/llamacpp-direct-wayfinder`). La release ufficiale llama.cpp è già installata nella dir
   stabile `C:\Users\CGS03\.translate-lector\llama.cpp\`.
2. **Verifica e2e con GUI** (checklist nel ticket `done/04-task-sidecar-lifecycle.md`): spawn
   on-demand alla prima traduzione, kill senza orfani alla chiusura, reap all'avvio, reuse di un
   server già sano, errori azionabili su path mancante / base_url senza porta. Poi misura di
   conferma latenza su pagina reale.

Follow-up noto (non bloccante): assunzione single-instance (PID file condiviso) → valutare
`tauri-plugin-single-instance`.

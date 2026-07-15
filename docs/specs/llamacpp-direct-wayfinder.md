# Provider locale diretto llama.cpp (rimozione Unsloth Studio) — Wayfinding Spec

## Type

Wayfinding spec

## Status

Active

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
  dall'installazione di Studio che vogliamo rimuovere → il sourcing del binario è la prima frontiera
  (ticket 01).
- **L'app è già pronta lato client**: esiste il preset `llamaserver`
  (`http://127.0.0.1:8080/v1/chat/completions`, `settings.rs:229-236`), e `is_local_url`
  (`llm.rs:775-788`) copre `127.0.0.1` → timeout 180 s, 0 retry-on-timeout, serializzazione
  prefetch e cancellazione job stantii si applicano senza toccare codice. `n_ctx` default 4096 =
  `-c 4096` del server. Nessuna API key richiesta (llama-server senza `--api-key` ignora l'header).
- **Ereditate dalla mappa latenza** (decision-brief-latency-03): L1 packing 512 (in main), L3
  serializzazione (in main), L4 0 retry (in main), L5 target ≤2 min ora largamente battuto. L6
  ("resta gemma-4") resta valida — qui NON si cambia modello, si cambia il modo di servirlo.
- **Qualità senza reasoning: da validare a occhio** (HITL). Nel test sintetico un piccolo
  scivolone grammaticale ("I scienziati" invece di "Gli scienziati"); serve lettura umana di pagine
  reali prima di fare del provider diretto il default (ticket 07).

## Not Yet Specified

- **Sourcing del binario** (ticket 01): riusare il build Unsloth (già sul disco ma legato alle DLL
  del venv di Studio) vs release ufficiale llama.cpp Windows CUDA (zip `cudart` incluso, licenza
  MIT) vs build proprio. Dimensioni del bundle, story di aggiornamento.
- **Contratto sidecar Tauri 2** (ticket 02): `externalBin`/plugin shell, spawn/kill lifecycle,
  passaggio env/PATH, gestione porta occupata, health probe (l'app ha già il probe di reachability
  a 1.5 s), riavvio su crash.
- **Distribuzione** (grilling 03): sidecar impacchettato nell'installer (bundle più pesante:
  ggml-cuda ~170 MB + cudart) vs "gestito ma esterno" (l'app scarica/avvia un binario in una dir
  dati) vs solo script documentato.
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

1. **Sourcing del binario** (ticket 01, ready): senza una fonte del binario indipendente da Studio,
   "togliere Unsloth" è solo a metà. Sblocca il grilling.
2. **Contratto sidecar Tauri** (ticket 02, ready, parallelo a 01): serve sapere cosa Tauri 2
   permette (bundling, lifecycle, env) prima di decidere la distribuzione nel grilling.
3. **Grilling 03** (blocked da 01+02): distribuzione, modello, preset unsloth, parametri.
4. **Build** (ticket 04, 05, 06 — blocked dal grilling).
5. **Qualità HITL** (ticket 07, ready da subito: il server è avviabile a mano oggi stesso).

## Ticket Plan

Cartella: `docs/tickets/llamacpp-direct/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Sourcing del binario llama-server (Unsloth build vs release ufficiale) | ready |
| 02 | research | Contratto sidecar Tauri 2 (spawn/kill, env, bundling, porta) | ready |
| 03 | grilling | Decisioni: distribuzione, modello GGUF, preset unsloth, parametri default | blocked (01, 02) |
| 04 | task | Sidecar lifecycle: spawn/health/kill di llama-server dall'app | blocked (03) |
| 05 | task | Gestione del modello GGUF (path/download secondo grilling) | blocked (03) |
| 06 | task | Cleanup preset unsloth + documentazione | blocked (03) |
| 07 | task (HITL) | Validazione qualità traduzione senza reasoning su pagine reali | ready |

## Next Review

Dopo i ticket 01-02: eseguire il grilling 03 con l'utente. Dopo la build (04-06): misura di
conferma col protocollo del ticket 04 latenza (pagina densa reale, prima/dopo) e chiusura della
nota "riaprire L6" nella mappa latenza. Il ticket 07 (HITL) decide se il provider diretto diventa
il default consigliato nel README.

# 04 — Task: lifecycle del sidecar llama-server

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

task

## Decisioni dal grilling 03

Vedi [decision-brief-llamacpp-direct-03.md](../../specs/decision-brief-llamacpp-direct-03.md).
Rilevanti qui: **D1** (app-managed spawn/kill), **D4** (parametri), **D5** (spawn on-demand),
+ assunzione 2 (reap orfani).

## Outcome

L'app avvia, monitora e ferma llama-server da sola: selezionando il provider `llamaserver` in ⚙️ (o
avendolo come default, D5), la prima traduzione fa partire il server e la chiusura dell'app lo ferma,
senza che l'utente apra terminali o Studio.

## Acceptance Criteria

- [ ] **Spawn on-demand** (D5): alla prima traduzione col provider locale, non all'avvio dell'app.
      Parametri D4: `--port 8080 -ngl 99 -c 4096 --reasoning off --parallel 1` (letti dagli override
      ⚙️ se presenti). Binario dal path configurato (ticket 05).
- [ ] **Nessun doppio spawn**: se un server nostro è già sano sulla porta (probe ok), riuso; se la
      porta è occupata da altro, errore azionabile.
- [ ] **Reap orfani** (assunzione 2): al lancio dell'app, se esiste un llama-server nostro stantio
      (es. dopo un hard-crash precedente), va terminato prima di rilanciare.
- [ ] **Kill affidabile alla chiusura**: via `RunEvent::Exit | ExitRequested` con `CommandChild` in
      managed state; `.take()` + `.kill()`. Verificare nessun processo orfano nel caso normale.
- [ ] **Health** integrata col probe esistente `probe_reachable` (`llm.rs:809`): finché il server
      non risponde, messaggio azionabile (non il generico "non raggiungibile").
- [ ] `tauri-plugin-shell` aggiunto + `.plugin(tauri_plugin_shell::init())`; permesso
      `shell:allow-execute` col sidecar in `capabilities/default.json`; entrypoint migrato da
      `.run(generate_context!())` a `.build(ctx)?.run(|_, event| ...)`.
- [ ] Test unitari sulla logica pura di lifecycle (decisione spawn/riuso/kill/reap) con processo
      mockato; test manuale end-to-end registrato nel ticket. Suite completa verde.

## Blocked By

- [03-grilling-llamacpp-direct-decisions.md](./done/03-grilling-llamacpp-direct-decisions.md) →
  **done**. Sbloccato (dipende anche dal ticket 05 per il path del binario/modello — coordinare).

## Frontier

È il cuore della destinazione: senza lifecycle gestito, "togliere Unsloth" resta un comando
PowerShell da ricordare.

## Work Plan

1. Aggiungere `tauri-plugin-shell = "2"` e registrarlo; aggiungere il permesso shell nel capability.
2. TDD sulla logica pura: stato del server (spawnato/sano/assente), decisione spawn-vs-riuso, reap
   di un pid stantio. Managed state `Mutex<Option<CommandChild>>` accanto a `CurrentPage`/
   `LocalProviderSlot`.
3. Migrare l'entrypoint a `.build(ctx)?.run(|app, event| ...)`; nel callback gestire kill-on-exit.
4. Wiring del comando di traduzione: spawn on-demand se provider locale e server non pronto; poll di
   `probe_reachable` fino a readiness; messaggi d'errore azionabili.
5. Test manuale e2e con l'app dev (spawn alla prima traduzione, kill alla chiusura, reap).

## Evidence to Capture

- Log di spawn/kill/reap, esito del test manuale con l'app dev (nessun orfano dopo chiusura).

## Out of Scope

- Risoluzione dei path binario/GGUF e loro default (ticket 05).
- Preset unsloth e docs (ticket 06).

## Esito implementazione (2026-07-15)

**Meccanismo di spawn scelto**: `tauri-plugin-shell` (`app.shell().command(<abs_path>)`).
Confermato via ctx7 (`/websites/v2_tauri_app`) che l'API Rust accetta un programma
esterno arbitrario (es. `cargo`/`echo`) e che la *scope* `shell:allow-execute` vincola solo
la superficie IPC/JS, non lo spawn lato Rust → un path assoluto runtime funziona senza
scope-wrangling, e resta il `CommandChild` per il kill-on-exit (pattern ticket 02).

**Logica pura TDD** (in `src-tauri/src/sidecar.rs`, 13 test + 2 in `llm.rs`):
- `spawn_decision(is_local, reachable, paths_ok) -> SpawnAction` (Spawn / ReuseExisting /
  SkipRemote / ErrorMissingPaths).
- `is_managed_local_provider(is_local, binary_blank)` — solo `llamaserver` è gestito; unsloth/
  lmstudio/ollama (senza `binary_path`) restano lanciati dall'utente.
- `build_llama_args(port, n_ctx, model)` — asseriti `-m`, `--port`, `-ngl 99`, `-c`,
  `--reasoning off`, `--parallel 1` (D4).
- `port_from_base_url` (in `llm.rs`, IPv6-aware) per la porta dello spawn.
- `reap_decision(persisted_pid, alive)` + `binary_image_name` per il reap conservativo.

**Shell imperativo (sottile, non testato con processi reali)**: `ensure_local_server_ready`
(spawn on-demand dietro `LocalProviderSlot` → niente doppio spawn concorrente; poll
`probe_reachable` fino a ~30s; messaggi azionabili), `kill_llama_server_on_exit`
(`RunEvent::Exit | ExitRequested` → `.take()` + `.kill()`), `reap_stale_llama_server_on_startup`
(PID persistito in app-config, guardia per nome immagine via `tasklist`/`taskkill` su Windows).

**Verifica automatica**: `cargo build` OK; `cargo test` **271 passed, 0 failed** (era 256 dopo
ticket 05; +15). Frontend non toccato (nessun `npm run check` necessario).

**Criteri che richiedono conferma e2e umana** (non lanciabili AFK — serve la GUI + llama-server
reale, e c'era un llama-server manuale su :8080 da non toccare):
- Spawn reale del server alla prima traduzione (logica testata; processo reale no).
- Kill affidabile alla chiusura senza orfani (serve `RunEvent` con GUI reale).
- Reap di un orfano dopo hard-crash (serve simulare il crash con la GUI).
- Riuso di un server sano già in ascolto (probe→ReuseExisting) — verificabile puntando al :8080
  esistente senza rilanciare.

## Open follow-ups

- **Istanza singola**: l'app assume una sola istanza in esecuzione. Il PID file è condiviso, quindi
  lanciare una seconda istanza potrebbe reapare il server app-managed della prima. Fix futuro
  consigliato: registrare `tauri-plugin-single-instance` (non aggiunto ora).

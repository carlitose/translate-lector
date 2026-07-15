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

# 02 — Research: contratto sidecar Tauri 2

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Status

**Done** (2026-07-15). Fonti: ctx7 `/websites/v2_tauri_app` (mirror v2.tauri.app) + docs.rs per
`CommandChild::kill` e `RunEvent` (non coperti da ctx7). Vedi §Findings.

## Type

research

## Outcome

Il contratto tecnico per gestire llama-server dall'app Tauri 2: come si impacchetta
(`externalBin`/risorse), come si fa spawn/kill dal backend Rust, come si passano env e argomenti,
come si gestiscono porta occupata, crash e chiusura dell'app. Con citazioni della documentazione
ufficiale corrente (via ctx7).

## Acceptance Criteria

- [x] Documentate le opzioni Tauri 2 per binari esterni: `bundle.externalBin` (sidecar) vs
      `bundle.resources` vs binario in app-data scaricato al primo avvio — con vincoli di ognuna
      (naming per target-triple, firma, dimensioni installer). → §Findings A + F.
- [x] Documentato il lifecycle: spawn dal backend Rust (plugin shell o `std::process` diretto),
      kill garantito alla chiusura dell'app (anche crash), riavvio su exit inatteso. → §Findings B.
- [x] Documentata la strategia porta: porta fissa 8080 (= preset `llamaserver`) vs porta dinamica +
      override `provider.llamaserver.base_url`; interazione col probe di reachability esistente
      (1.5 s) e col caso "porta già occupata da altro processo". → §Findings C + F.6.
- [x] Nota su come le DLL accanto all'exe vengono risolte su Windows (nessun PATH necessario se
      sono nella stessa dir del sidecar?). → §Findings A (adiacenza; mitigazione `current_dir`/`PATH`).
- [x] Esito ripiegato nella mappa. → §Findings + mappa aggiornata.

## Blocked By

- None — can start immediately (parallelo al ticket 01).

## Frontier

Il grilling 03 deve scegliere la distribuzione (bundled vs esterno gestito vs script): senza sapere
cosa Tauri 2 rende facile/difficile, la decisione sarebbe al buio.

## Work Plan

1. `npx ctx7@latest library "Tauri" "bundle external sidecar binary spawn kill lifecycle"` e
   fetch docs mirate (sidecar/shell plugin, bundle config).
2. Verificare nel repo come l'app fa oggi spawn/gestione processi (se mai) e dove vive la config
   bundle (`src-tauri/tauri.conf.json`).
3. Rispondere ai punti dei criteri di accettazione con riferimenti alle docs.
4. Stimare l'impatto sull'installer (dimensioni attuali vs + sidecar CUDA).

## Evidence to Capture

- Estratti/URL delle docs Tauri 2 per ogni claim.
- Config `tauri.conf.json` attuale rilevante.

## Out of Scope

- L'implementazione (ticket 04).
- Il download del modello GGUF (ticket 05).

## Findings

### A. Bundling — `externalBin` per l'exe + `resources` (o env/cwd) per le DLL CUDA

- **`externalBin` (sidecar)** per `llama-server.exe`. Regola dura: ogni sidecar va suffissato col
  target-triple dell'host (`rustc --print host-tuple` → `x86_64-pc-windows-msvc`), quindi il file
  in `src-tauri/binaries/` deve chiamarsi `llama-server-x86_64-pc-windows-msvc.exe`; in
  `tauri.conf.json` si elenca senza triple/estensione (`"externalBin": ["binaries/llama-server"]`)
  e da Rust si chiama `sidecar("llama-server")`.
- **Il sidecar NON viene estratto in una temp dir**: viene installato accanto all'exe principale
  ed eseguito lì. Quindi le DLL CUDA (`ggml-cuda.dll`, `cudart64_*.dll`, `cublas64_*.dll`) devono
  finire nella stessa dir. `externalBin` accetta solo eseguibili → le DLL vanno in
  `bundle.resources` mappate nella root d'installazione **oppure**, più robusto, non dipendere
  dall'adiacenza: risolvere `app.path().resource_dir()` in Rust e passare al sidecar
  `.current_dir(dir)` o `PATH` prependato. **Pattern (2) raccomandato** perché non dipende dal
  layout che l'installer NSIS/MSI produce (da verificare comunque su un install reale, non solo
  `tauri dev`).
- **Download-at-first-run** = alternativa per tenere l'installer piccolo (le DLL CUDA sono
  centinaia di MB), ma aggiunge flusso download/verify/extract, versioning e stessa questione di
  adiacenza in app-data. Raccomandato: bundle per la v1, download come fallback di dimensione.

### B. Lifecycle — spawn con `tauri-plugin-shell`, kill garantito via `RunEvent`

- Spawn: `app.shell().sidecar("llama-server")?.args([...]).spawn()?` → `(Receiver<CommandEvent>,
  CommandChild)`; leggere `CommandEvent::Stdout/Stderr/Terminated` in un `tauri::async_runtime::spawn`.
- Kill: `CommandChild::kill(self) -> Result<(), Error>` (consuma l'handle; c'è anche `pid()`).
- **Kill-on-exit deterministico**: tenere il `CommandChild` in managed state
  `Mutex<Option<CommandChild>>`, passare da `.run(generate_context!())` a
  `.build(ctx)?.run(|app, event| ...)` e nel callback su `RunEvent::ExitRequested | RunEvent::Exit`
  fare `.take()` + `.kill()`. `Drop` come backup, ma NON scatta su force-kill dell'app stessa.
- **Orfani su hard-crash**: `RunEvent`/`Drop` non scattano se l'app Tauri è killata a forza →
  persistere il pid e reap del llama-server stantio al lancio successivo, oppure Windows Job Object.
  (Rischio per il grilling.)

### C. Porta — nessuna guida Tauri, è logica app

Tauri non contribuisce nulla. Il `base_url` è già configurabile e `llm.rs:781` estrae la porta
(IPv6-aware). Raccomandato: porta fissa di default (allineata a `base_url`) con override; porta
dinamica possibile ma è logica applicativa.

### D. Permessi — `shell:allow-execute` col sidecar in `capabilities/default.json`

```json
{
  "identifier": "shell:allow-execute",
  "allow": [ { "name": "binaries/llama-server", "sidecar": true, "args": true } ]
}
```

Il `name` deve combaciare col path `externalBin`. Il kill lato Rust non richiede il permesso
JS-facing `allow-kill`.

### E. Stato attuale del repo

- `tauri = "2"`, config schema v2. **`tauri-plugin-shell` NON presente** (ci sono opener + dialog):
  va aggiunto `tauri-plugin-shell = "2"` + `.plugin(tauri_plugin_shell::init())`.
- `bundle` (`tauri.conf.json`): `active:true`, `targets:"all"`, solo icone. **Niente `externalBin`,
  `resources`, `plugins`.** `identifier: com.translatelector.app`.
- Builder (`lib.rs:637-689`): usa `.run(generate_context!())` → va migrato a `.build()?.run(|..|)`.
  Il `setup` già fa `manage` di `CurrentPage` e `LocalProviderSlot`: l'handle del child segue lo
  stesso pattern di managed state.
- **Nessun codice che fa spawn di processi oggi** (solo `std::process::id()` per temp file).
- **Health probe già pronto**: `llm::probe_reachable(base_url) -> bool` (`llm.rs:809-819`), GET a
  `/v1/models` timeout 1500 ms, esposto come `check_provider_reachable` (`lib.rs:550-562`). È il
  segnale di readiness da pollare dopo lo spawn.
- `capabilities/default.json` esiste (`core/opener/dialog:default`): aggiungere la voce shell.

### F. Rischi / domande aperte per il grilling 03

1. **Dimensione installer**: DLL CUDA centinaia di MB → bundle vs download-on-first-run.
2. **Adiacenza DLL** da verificare su install reale; mitigazione sicura = `current_dir`/`PATH` in Rust.
3. **Orfani su hard-crash** dell'app: pid persistito + reap, o Job Object.
4. **Code signing** del sidecar e delle DLL di terze parti (SmartScreen).
5. **Assenza GPU/driver** sul target: eventuale fallback CPU o errore chiaro.
6. **Collisione porta** 8888/8080: verificare che il responder sia davvero il nostro llama-server.
7. **Migrazione `build()` vs `run()`**: tocca l'entrypoint (`lib.rs:687`), cambio piccolo ma necessario.

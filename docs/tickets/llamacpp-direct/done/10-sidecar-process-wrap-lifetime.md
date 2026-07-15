# 10 — Sidecar cross-platform via process-wrap: kill garantito del figlio alla morte del padre

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## What to Build

Oggi il processo `llama-server` gestito dall'app resta **orfano** ogni volta che l'app non si chiude
in modo pulito: force-kill da Task Manager, crash, o un lancio `tauri dev` che fallisce dopo aver già
avviato l'exe. Il `kill-on-exit` via `RunEvent::Exit` scatta solo alla chiusura pulita, e il
`reap-on-startup` (ticket 04) è una rete best-effort basata su PID-file + nome immagine. L'orfano
tiene occupata la GPU (~1.5 GB) e causa i conflitti di porta ricorrenti.

Legare il ciclo di vita di `llama-server` a quello dell'app usando il crate **`process-wrap`** (ex
`command-group`, stesso autore di watchexec) così che il figlio — e il suo albero — venga terminato
in modo affidabile quando il padre muore. `translate-lector` gira su **Windows e macOS**, quindi la
soluzione deve essere cross-platform tramite l'astrazione del wrapper, non codice Win32 a mano.

Garanzie attese per OS (essere onesti nei messaggi/nei commenti, non promettere più del reale):

- **Windows**: Job Object con `KILL_ON_JOB_CLOSE` → il kernel uccide il figlio quando il processo
  padre muore *comunque* (chiusura pulita, crash, force-kill). Buco dell'orfano **chiuso**.
- **macOS / Linux (Unix)**: il wrapper mette il figlio in un process-group così che il kill esplicito
  (via `RunEvent`) termini l'intero albero in modo affidabile. Il caso "l'app crasha di brutto → il
  figlio muore da solo" **non** è garantito dal wrapper su Unix; per quello resta il **reap-on-startup
  esistente** come fallback (portabile). Un watchdog kqueue (`NOTE_EXIT`) per macOS è fuori scope
  (follow-up eventuale).

Nota di design: adottando `process-wrap` si spawna via `std::process::Command` (wrappato) invece che
via `tauri-plugin-shell`, quindi si perde lo stream `CommandEvent` (stdout) del plugin. È accettabile:
la readiness è già via HTTP (`probe_reachable` / `probe_model_ready` su `/health`), non via stdout.
Valutare se `tauri-plugin-shell` resta necessario altrove; se non serve più, si può rimuovere.

## Acceptance Criteria

- [ ] `llama-server` viene spawnato tramite `process-wrap` (o `command-group`), con il figlio legato
      al ciclo di vita dell'app: su Windows tramite Job Object `KILL_ON_JOB_CLOSE`, su Unix tramite
      process-group.
- [ ] **Windows**: force-killando l'app (Task Manager) mentre `llama-server` gira, il processo
      `llama-server` **termina da solo** senza bisogno del reap al lancio successivo. (Verifica manuale.)
- [ ] Chiusura pulita dell'app (X della finestra) continua a terminare `llama-server` su tutti gli OS
      (nessuna regressione del `kill-on-exit` via `RunEvent`).
- [ ] Il `reap-on-startup` esistente resta come fallback e continua a funzionare (utile su Unix per il
      caso crash e come rete di sicurezza).
- [ ] La readiness (`probe_reachable`/`probe_model_ready`), la serializzazione L3 (`LocalProviderSlot`)
      e lo spawn on-demand (D5) restano invariati nel comportamento.
- [ ] Messaggi/commenti onesti sulle garanzie per OS (nessuna promessa di crash-safety su macOS).
- [ ] Test unitari sulla logica pura toccata (decisione spawn/kill, eventuale adattatore) con processo
      mockato; nessun test che spawna un processo reale. Suite completa verde.

## Blocked By

- None - can start immediately. (Tutta la base — spawn on-demand, RunEvent kill, reap, readiness — è
  già in main dai ticket 04/08.)

## Frontier

Ready now. È il fix definitivo del buco orfano segnalato come follow-up nel ticket 04 (nota
"single-instance / Job Object"), esteso a cross-platform perché l'app girerà anche su macOS.

## Step-by-Step Implementation Plan

1. **Verificare l'API corrente del crate** (docs.rs / ctx7 se disponibile): scegliere tra
   `process-wrap` (preferito, più modulare, supporta `JobObject` su Windows e process-group su Unix,
   std e tokio) e `command-group`. Confermare come si ottiene il Job Object con kill-on-close su
   Windows e il process-group su Unix, e come si ottiene un handle per il kill esplicito. Perché
   prima: l'API decide la forma dell'adattatore.
2. **Aggiungere la dipendenza** in `src-tauri/Cargo.toml` e sostituire lo spawn in
   `src-tauri/src/sidecar.rs` (`spawn_llama_server`) con lo spawn wrappato. Mantenere gli stessi
   parametri D4 (`--port/-ngl/-c/--reasoning off/--parallel 1`) e il path del binario configurato.
   Perché qui: è il cuore del cambiamento.
3. **Aggiornare il managed state**: oggi si tiene un `CommandChild` di plugin-shell in
   `LlamaServerProcess(Mutex<Option<...>>)`; sostituirlo con l'handle del wrapper. Aggiornare
   `kill_llama_server_on_exit` (chiamato da `RunEvent::Exit | ExitRequested`) per uccidere l'albero
   tramite il nuovo handle. Verificare: chiusura pulita termina il figlio (test/logica pura dove
   possibile).
4. **Mantenere il reap-on-startup** invariato come fallback; verificare che continui a compilare e a
   passare i test (utile su Unix e come rete). Non rimuoverlo.
5. **Isolare le parti OS-specifiche** dietro l'astrazione del wrapper; se restano `#[cfg]`, tenerli
   minimi e coerenti con gli stub già presenti dal ticket 04. Commenti onesti sulle garanzie per OS.
6. **Verifica**: `cargo build` + `cargo test` in `src-tauri` (usare `rtk proxy cargo test` se l'hook
   nasconde l'output); suite verde. Poi verifica manuale su Windows (force-kill → nessun orfano) e,
   se disponibile, su macOS (chiusura pulita → nessun orfano; nota che il crash su Mac resta coperto
   solo dal reap).

## Testing Plan

- Unit sulla logica pura toccata (costruzione args invariata, eventuale adattatore di spawn/kill) con
  processo mockato; nessuno spawn reale nei test.
- Regressione: test esistenti di `sidecar`/`is_current`/`LocalProviderSlot`/readiness verdi; suite
  completa invariata o superiore.
- Manuale (non automatizzabile AFK):
  - Windows: avvia app → traduci (spawn) → force-kill app da Task Manager → `Get-Process llama-server`
    deve dare 0 **senza** attendere il reap del lancio successivo.
  - Chiusura pulita (X) su Windows/macOS → nessun orfano.
  - Verifica che spawn on-demand, prefetch e readiness funzionino come prima.

## Out of Scope

- Watchdog kqueue/`NOTE_EXIT` per la crash-safety su macOS (follow-up eventuale; per ora coperto dal
  reap-on-startup).
- `PR_SET_PDEATHSIG` su Linux (idem, follow-up se mai servisse Linux).
- Gli orfani di dev-mode non-llama (`node`/Vite su 1420, `translate-lector.exe` da lancio fallito):
  sono quirk di `tauri dev`, non toccati da questo ticket.
- Single-instance dell'app (`tauri-plugin-single-instance`): follow-up separato già annotato nel
  ticket 04.

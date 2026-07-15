# 08 — Readiness attende il modello caricato (non solo il socket)

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## What to Build

Il controllo di prontezza del server locale app-managed deve aspettare che il **modello sia
effettivamente caricato in VRAM**, non solo che il processo risponda sulla porta. Oggi
`ensure_local_server_ready` fa spawn e poi `wait_until_ready`, che fa poll di
`probe_reachable`; ma `probe_reachable` ritorna `true` appena l'endpoint risponde "con qualsiasi
status HTTP". Durante il caricamento del modello, llama-server risponde già su `/v1/models` mentre
`chat/completions` restituisce `503 {"error":{"message":"Loading model","code":503}}`. Risultato: la
**primissima traduzione a freddo** fallisce col 503, e solo un retry manuale funziona.

Comportamento voluto (primo test e2e, 2026-07-15): dopo lo spawn, l'app attende finché il modello è
caricato e poi procede con la traduzione senza errori visibili. La leva corretta è l'endpoint
`/health` di llama.cpp, che risponde **503 mentre carica** e **200 quando è pronto** — segnale di
readiness che riflette lo stato del modello, non solo il socket. In più, il 503 "Loading model" che
dovesse comunque arrivare da `chat/completions` va trattato come **transitorio/ritentabile**, non
come errore terminale mostrato all'utente.

Questo ticket riguarda solo l'affidabilità del primo avvio; la messaggistica del banner è il
ticket 09.

## Acceptance Criteria

- [ ] Dopo lo spawn del server app-managed (provider `llamaserver`), la **prima traduzione a freddo
      riesce senza errore 503** (nessun retry manuale richiesto), entro il timeout esistente
      (`SERVER_READY_TIMEOUT`).
- [ ] La readiness distingue "socket in ascolto" da "modello caricato": il poll considera il server
      pronto solo quando llama.cpp segnala il modello caricato (endpoint `/health` → 200, o segnale
      equivalente), non appena `/v1/models` risponde.
- [ ] Un `503 {"message":"Loading model"}` proveniente da `chat/completions` (se sfugge alla
      readiness) è classificato come **transitorio** e ritentato entro il timeout, non propagato come
      errore terminale all'utente.
- [ ] Su timeout reale (modello che non carica, path errato, porta occupata) resta il messaggio
      azionabile esistente — nessuna regressione del caso di errore genuino.
- [ ] Il provider cloud e i provider locali user-launched non sono toccati.
- [ ] Test unitari sul parsing/classificazione della readiness e del 503; suite verde.

## Blocked By

- None - can start immediately.

## Frontier

Ready now. È il fix di affidabilità del primo avvio a freddo emerso dal test e2e: senza, ogni cold
start del provider app-managed mostra un 503 alla prima pagina.

## Step-by-Step Implementation Plan

1. **Verificare il contratto di `/health` di llama.cpp** (usa `ctx7`/docs se serve conferma della
   forma esatta: `{"status":"loading model"}` con 503 mentre carica, `{"status":"ok"}` 200 quando
   pronto). Perché prima: la readiness corretta dipende da questo segnale. Superficie: nessun codice
   ancora, solo conferma del contratto.
2. **Aggiungere una funzione di probe di readiness del modello** accanto a `probe_reachable`
   (`llm.rs`), es. `probe_model_ready(base_url) -> bool` che interroga `/health` e ritorna `true`
   solo su 200. Tenerla pura/isolata come `probe_reachable` (stesso client con timeout breve). Perché
   qui: introduce il segnale mancante senza toccare la logica di spawn. Verificare: unit test con
   endpoint fittizio 503→false, 200→true, connection-refused→false.
3. **Aggiornare `wait_until_ready`** (`sidecar.rs`) per fare poll del nuovo segnale di readiness del
   modello invece del solo `probe_reachable`. Mantenere `SERVER_READY_TIMEOUT`/`SERVER_POLL_INTERVAL`
   e il messaggio di timeout azionabile esistente. Perché dopo il passo 2: consuma il nuovo probe.
   Verificare: la logica di attesa/timeout resta testabile (estrarre l'eventuale decisione pura se
   utile).
4. **Rete di sicurezza sul 503 "Loading model"**: nel percorso di classificazione errori della
   traduzione (`llm.rs`, dove il 503/`unavailable_error` viene mappato), riconoscere il body
   `{"message":"Loading model"}` / code 503 come **transitorio** così che il retry esistente
   (`RetryingChatClient`) lo ritenti entro il budget, invece di propagarlo. Perché: difesa in
   profondità se la readiness lascia passare un caso limite. Verificare: unit test che il 503
   "Loading model" è classificato transitorio mentre altri 503 restano com'erano.
5. **Test e2e manuale** (cold start): chiudere il server, avviare l'app, tradurre una pagina →
   nessun 503 visibile, la traduzione parte da sola dopo il caricamento. Registrare l'esito.

## Testing Plan

- Unit: `probe_model_ready` (503/200/refused); classificazione transitoria del 503 "Loading model";
  eventuale logica pura di `wait_until_ready`.
- Regressione: i test esistenti di `probe_reachable`, del timeout azionabile, e della
  classificazione errori restano verdi; suite completa (attualmente 274) invariata o superiore.
- Manuale (non automatizzabile AFK): cold start reale col provider `llamaserver` → prima traduzione
  senza 503. Confronto: prima del fix il 503 appariva alla prima pagina.

## Out of Scope

- Testo del banner/errore "server non raggiungibile" (ticket 09).
- Assunzione single-instance / `tauri-plugin-single-instance` (follow-up già documentato nel
  ticket 04).
- Qualsiasi cambiamento ai provider cloud o locali user-launched (unsloth/lmstudio/ollama).

## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## What to Build

Rendere l'app robusta all'**inferenza locale lenta**. Sintomo segnalato in test:
`Errore di rete/servizio LLM (timeout): error sending request for url (http://localhost:8888/v1/chat/completions)`.

Diagnosi iniziale (da confermare nel work plan): il client chat usa `reqwest::blocking::Client::new()`
**senza timeout esplicito** (`src-tauri/src/llm.rs:427`) — solo `probe_reachable` ha un timeout (1.5s,
`llm.rs:620`). Con `n_ctx` alto e un modello locale, una pagina può richiedere minuti; il timeout osservato
proviene quindi verosimilmente dal **proxy/server** (Unsloth Studio fa da proxy davanti a llama-server) che
chiude la connessione lenta, non dall'app (che oggi non ha timeout → anche un rischio latente di hang).

## Acceptance Criteria

- [x] Confermata la sorgente del timeout (app vs server/proxy) con una prova: una richiesta lenta reale al
      server locale, osservando dopo quanto e da chi la connessione cade. **Confermato senza dover ripetere
      la prova dal vivo**: `docs/tickets/local-translation-latency/done/01-research-latency-baseline.md`
      (§3, punto 3) documenta con evidenza da docs.rs (`blocking/client.rs`) che
      `reqwest::blocking::Client::new()` ha un **timeout di default di 30s** — mai reso esplicito dall'app.
      Una chiamata reale misurata durava 29.7s, al pelo del limite. Non è il proxy/Unsloth Studio a tagliare
      (come ipotizzato inizialmente qui): è il default del client stesso.
- [x] Il client chat ha un **timeout esplicito e generoso**, **configurabile per-provider** (riuso esatto del
      pattern `ProviderConfig`/settings dei Ticket 07-08: `DEFAULT_TIMEOUT_SECS_CLOUD = 30` / `..._LOCAL =
      180`, chiave `provider.{id}.timeout_secs`, risoluzione in `get_provider_config`). Il campo `timeout_secs`
      è passato a `ChatCompletionsClient::new`, che ora costruisce il `reqwest::blocking::Client` con
      `.timeout(Duration::from_secs(timeout_secs))` (fallback sicuro, non-panicking, se il builder fallisse).
      Verificato end-to-end con un test che apre un `TcpListener` che accetta ma non risponde mai: con
      `timeout_secs=1` la chiamata fallisce con `LlmError::Timeout` in ~1s, non nei 30s impliciti di prima.
- [x] Messaggio d'errore timeout **azionabile per il caso locale**, distinto dal timeout cloud: riusa
      `is_local_url(base_url)` dentro `classify_send_error` per differenziare il testo portato da
      `LlmError::Timeout(msg)` (nessuna nuova variante); il messaggio remoto resta quello generico invariato.
- [x] Policy di **retry su timeout** rivalutata e decisa: **decisione L4** (vedi
      `docs/specs/decision-brief-latency-03.md` §L4) — **0 retry** sul timeout per i provider locali (un
      timeout locale segnala un problema reale; ritentare triplica l'attesa senza aiutare), retry invariato
      (×3 con backoff) per OpenRouter/cloud e per gli altri errori transient (`ServerError`/`RateLimited`/
      `Offline`) anche in locale. Implementato con `RetryPolicy.retry_on_timeout: bool` (default `true`,
      preserva il comportamento esistente/i test esistenti); `lib.rs` sceglie `retry_on_timeout: false` quando
      `llm::is_local_url(&cfg.base_url)`.
- [x] `cargo test` verde (test sul timeout configurato / classificazione / messaggio). Non toccata la UI/
      mapping errori frontend (fuori scope per questo ticket, vedi sotto) → `npm run check`/`vitest` non
      necessari.

## Blocked By

- None - can start immediately (server locale disponibile su `localhost:8888`).

## Frontier

Ready. Robustezza del provider locale, indipendente dai fix empty-content (che sono già in `done/`). Nota:
parte della causa può essere **fuori dall'app** (timeout del proxy Unsloth Studio) → in tal caso la parte
app è il timeout configurabile + messaggio azionabile, e la spec/onboarding documenta l'azione lato server.

## Step-by-Step Implementation Plan

1. Confermare la sorgente: inviare una richiesta lenta reale (pagina grande / n_ctx alto) e misurare quando
   la connessione cade e con quale errore reqwest (`is_timeout` vs connessione chiusa). Annotare l'esito.
2. Aggiungere un timeout esplicito al `reqwest::blocking::Client` del chat client via `Client::builder().timeout(...)`,
   con valore **per-provider** (riusare `ProviderConfig`/settings; default locale generoso, cloud invariato).
3. Rendere il messaggio del timeout azionabile per il caso locale (eventuale variante o testo dedicato +
   hint frontend in `src/lib/translation.ts` se serve un marker come EC02/EC08).
4. Decidere la policy di retry su timeout per i locali (limitare/no) e implementarla in modo bounded.
5. Test + verifica; documentare l'azione lato server (alzare il timeout del proxy/Studio) nella spec/onboarding.

## Testing Plan

- Unit (Rust): il client usa il timeout configurato; classificazione/messaggio del timeout locale; policy di
  retry attesa.
- Manuale: con una pagina lenta reale, l'app attende entro il timeout configurato e, se scade, mostra il
  messaggio azionabile (nessun hang infinito, nessun fallback cloud — coerente con D4).

## Out of Scope

- Fix empty-content / max_tokens (già in `docs/tickets/local-llm-empty-content/done/`).
- Configurazione lato server del proxy/Studio (azione utente; solo documentata).
- Streaming delle risposte (possibile miglioramento futuro per ridurre la percezione di lentezza).

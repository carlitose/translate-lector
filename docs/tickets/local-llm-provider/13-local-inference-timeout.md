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

- [ ] Confermata la sorgente del timeout (app vs server/proxy) con una prova: una richiesta lenta reale al
      server locale, osservando dopo quanto e da chi la connessione cade.
- [ ] Il client chat ha un **timeout esplicito e generoso**, **configurabile per-provider** (riusare il
      pattern `ProviderConfig`/settings dei Ticket 07-08; default locale alto, es. 180s; OpenRouter invariato).
      Elimina il rischio di hang infinito e accoglie l'inferenza locale lenta quando il server tiene aperta
      la connessione.
- [ ] Messaggio d'errore timeout **azionabile per il caso locale** (es.: "Il server locale è troppo lento o
      ha chiuso la connessione. Aumenta il timeout del server/proxy (Unsloth Studio/llama-server), usa un
      modello più veloce o riduci `n_ctx`."), distinto dal timeout cloud.
- [ ] Rivalutare la policy di **retry su timeout**: oggi `Timeout` è transient e viene ritentato con backoff
      (`is_transient`, `llm.rs`); per una richiesta locale sistematicamente lenta il retry raddoppia l'attesa
      senza aiutare. Decidere se per i provider locali limitare/annullare il retry su timeout, o mantenerlo.
- [ ] `cargo test` verde (test sul timeout configurato / classificazione / messaggio); `npm run check` +
      `vitest` se si tocca la UI/mapping errori.

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

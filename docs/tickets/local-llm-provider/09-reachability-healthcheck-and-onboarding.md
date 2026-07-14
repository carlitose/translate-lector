## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## What to Build

Gestire il caso in cui il provider **locale attivo non è raggiungibile** (server spento, porta sbagliata),
con un **errore chiaro e nessun fallback automatico** al cloud (decisione **D4**), e una minima
**onboarding al primo avvio** dato che il provider di default è locale (**D3**) e l'utente avvia il server
a mano (**D7**).

Dopo questo ticket, se selezioni un provider locale e il server non risponde, vedi un messaggio comprensibile
("server locale non raggiungibile a {base_url} — avvia Unsloth Studio / verifica la porta"), non un errore
opaco né una traduzione cloud a sorpresa.

## Acceptance Criteria

- [ ] Un **health-check** verso il provider locale attivo (es. `GET {origin}/v1/models` o una `complete`
      leggera) che distingue "irraggiungibile" (connessione rifiutata/timeout) da altri errori.
- [ ] Alla traduzione, se il provider locale è irraggiungibile → messaggio d'errore chiaro (stile NFR06),
      **senza** fallback automatico a OpenRouter (D4). Riusa i codici/stili errore esistenti dove sensato
      (cfr. EC02/EC03).
- [ ] Indicatore/onboarding al primo avvio quando il provider attivo è locale e non raggiungibile: un
      hint che invita ad avviare il server o aprire le impostazioni (D3/D7). Non deve bloccare l'uso di
      OpenRouter se l'utente cambia provider.
- [ ] Nessuna orchestrazione del processo server dall'app (fuori scope, D7): solo check + messaggi.
- [ ] `cargo test` / `npm run check` verdi; test sul mapping "connessione rifiutata → messaggio dedicato".

## Blocked By

- [07-provider-presets-and-active-provider.md](./07-provider-presets-and-active-provider.md)

## Frontier

Blocked by 07 (serve il provider attivo risolto e il percorso di traduzione provider-agnostico). Realizza le
decisioni D3/D4/D7 e l'edge case EC02 per il caso locale. Può procedere in parallelo al Ticket 08 (UI), ma
ha più senso verificarlo dopo che il selettore esiste.

## Step-by-Step Implementation Plan

1. **Rilevamento irraggiungibilità nel core**: nel client/`translate_page`, mappa gli errori di
   connessione (connection refused / timeout DNS-less su localhost) a una variante `LlmError` dedicata,
   distinta da 4xx/5xx. Perché prima: è la base del messaggio. Verifica: unit test che simula l'errore di
   connessione → variante attesa.
2. **Comando/health-check** opzionale (`check_provider_reachable(provider_id)`) usato dalla UI per un
   segnale rapido. Perché ora: consente onboarding non bloccante. Verifica: ritorna false con server spento.
3. **Messaggio utente**: `user_message` per la nuova variante → "server locale non raggiungibile a
   {base_url}…". Nessun fallback cloud. Verifica: il frontend mostra il messaggio, non uno generico.
4. **Onboarding/hint** nel frontend: se all'avvio il provider attivo è locale e `check_provider_reachable`
   è false, mostra un hint (banner/stato) che invita ad avviare il server o aprire ⚙️. Verifica: con server
   spento appare l'hint; avviando il server e ritentando, la traduzione procede.
5. **Test**: mapping errore connessione; assenza di chiamate a OpenRouter quando il locale fallisce (no
   fallback). Verifica: `cargo test` + `npm run check` verdi.

Pitfall: non introdurre un fallback silenzioso al cloud (esplicitamente vietato da D4 — genererebbe costi
API inattesi). L'hint di onboarding non deve impedire di passare a OpenRouter manualmente.

## Testing Plan

- Unit (Rust): errore di connessione → variante/messaggio dedicati; nessun tentativo cloud.
- Manuale: con Unsloth Studio spento, seleziona provider locale e prova a tradurre → messaggio chiaro;
  avvia il server, ritenta → funziona. Cambia a OpenRouter con chiave valida → traduce.

## Out of Scope

- Avvio/stop del server locale dall'app (D7, post-MVP).
- Retry/backoff avanzati oltre a quanto già esiste (NFR06) — riusare l'esistente.
- Validazione qualità della traduzione locale (Ticket 10).

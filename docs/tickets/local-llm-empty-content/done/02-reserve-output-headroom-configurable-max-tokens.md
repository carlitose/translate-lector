## Parent Spec

[local-llm-empty-content-diagnosis.md](../../specs/local-llm-empty-content-diagnosis.md)

## Type

task

## What to Build

Smettere di richiedere `max_tokens` pari all'intera context window e riservare margine per l'output, così
che un modello locale con finestra piccola (4096) abbia spazio per generare `content`. Oggi
`build_request` hardcoda `max_tokens: 4096` (`src-tauri/src/llm.rs:87, 829`).

## Acceptance Criteria

- [ ] `max_tokens` non è più fisso a 4096: default sensato con margine (es. ~1024) e/o **configurabile
      per-provider** (coerente con l'astrazione dei Ticket 07/08).
- [ ] Il valore usato lascia spazio all'output entro `n_ctx` per i provider locali; il comportamento
      OpenRouter resta invariato (o migliorato) e i test esistenti restano verdi.
- [ ] (Se fattibile) il client limita/disattiva il reasoning per la traduzione dove il server lo consente
      (param stile `reasoning_effort`/`think:false`), oppure lo documenta come raccomandazione.
- [ ] `cargo test` verde (aggiornare/aggiungere test su `build_request`/max_tokens); `npm run check` se la UI
      espone l'impostazione.

## Blocked By

- None (raccomandato dopo la conferma del Ticket 01, ma il fix è sicuro comunque).

## Frontier

Ready. Fix diretto e a basso rischio; riduce drasticamente la probabilità di empty-content.

## Work Plan

1. Sostituire l'hardcoded `max_tokens: 4096` con un default con margine e/o un'impostazione per-provider
   (riusare `ProviderConfig`/settings dei Ticket 07/08).
2. Valutare l'invio di un parametro per limitare il reasoning nella richiesta di traduzione.
3. Aggiornare i test di `build_request`; verificare che OpenRouter resti invariato.

## Testing Plan

- Unit: `build_request` usa il nuovo max_tokens; nessuna regressione OpenRouter.
- Manuale: traduzione di una pagina reale via provider locale non produce più empty-content (con Ticket 03).

## Out of Scope

- Gestione/messaggio dell'empty-content residuo (Ticket 03).
- Chunking del testo (possibile follow-up).

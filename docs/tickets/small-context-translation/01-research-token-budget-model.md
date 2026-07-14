## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## Type

research

## Outcome

Un modello di **budget di token per-chiamata** che garantisca `system + summary + glossario_selezionato +
testo_unità + output_riservato ≤ n_ctx` per il provider attivo, con la formula di ripartizione e i punti di
aggancio nel codice. Fondamento per dimensionare chunk (Ticket 02) e selezione glossario (Ticket 03).

## Acceptance Criteria

- [ ] Formula definita: `budget_input = n_ctx − max_tokens_output − margine_sicurezza`, e ripartizione tra
      system prompt, summary, glossario selezionato, testo unità.
- [ ] Definito **dove si conosce `n_ctx`**: impostazione per-provider (riuso `ProviderConfig`/settings dei
      Ticket 07/08), con default sensati (locale ~4096) e possibilità di lettura da `/props`/`/v1/models`
      del server se disponibile (opzionale).
- [ ] Riuso di `est_tokens` (`src-tauri/src/llm.rs:730`) e della calibrazione `chars/token` dal `usage`
      reale; nota su margine per l'imprecisione dell'euristica.
- [ ] Coerenza con `max_tokens` per-provider (Ticket 02 empty-content, già fatto) e con la compressione
      summary (EC05).
- [ ] Design registrato nella mappa, pronto per Ticket 02/03/04.

## Blocked By

- None - can start immediately.

## Frontier

Foundational: senza un budget quantificato non si dimensionano né i chunk né il glossario selezionato.

## Work Plan

1. Rivedere `est_tokens`/`calibrate_chars_per_token` (`llm.rs`), il flusso in `translate.rs`, e come
   `max_tokens`/`ProviderConfig` sono già configurati per-provider.
2. Definire la formula di budget e la ripartizione; scegliere un margine di sicurezza per l'euristica token.
3. Decidere come/da dove ottenere `n_ctx` (setting per-provider; opz. probe del server).
4. Scrivere il modello nella mappa con esempi numerici (es. n_ctx=4096, output=2048 → budget_input ≈ 1800).

## Evidence to Capture

- Formula + esempio numerico; punti di aggancio (file:funzione); nota su calibrazione/margine.

## Out of Scope

- Implementazione dello splitting (Ticket 02) e della selezione glossario (Ticket 03).

## Parent Spec

[local-llm-empty-content-diagnosis.md](../../specs/local-llm-empty-content-diagnosis.md)

## Type

research (verifica)

## Outcome

Conferma finale della causa radice riproducendo l'empty-content con la richiesta **esatta** dell'app
(`max_tokens: 4096` + `response_format` json_schema + temperature) su una **pagina reale** contro il server
locale, catturando `finish_reason` e `usage` (in particolare i token di reasoning). Chiude la conferma che i
subagenti non hanno completato per il limite di sessione.

## Acceptance Criteria

- [ ] Riproduzione documentata: stessa forma di richiesta di `build_request` (`src-tauri/src/llm.rs:829`) con
      una pagina reale (~600-1500 token) → risposta con `finish_reason: "length"` e `content` vuoto/null.
- [ ] Catturati `usage` (prompt_tokens, completion_tokens, eventuale reasoning) e `finish_reason` grezzi.
- [ ] Confermato l'effetto di `max_tokens` (4096 vs valore ridotto) e, se possibile, con/ senza reasoning.
- [ ] Esito annotato nella diagnostic spec (conferma o correzione della causa radice).

## Blocked By

- None - can start immediately (server locale in esecuzione su `localhost:8888`).

## Frontier

Ready. Verifica che sblocca con sicurezza i fix 02/03. Riusa/estende `prototypes/local-llm/validate-perceptor-contract.mjs`.

## Work Plan

1. Estendere l'harness per inviare una pagina reale grande con `max_tokens: 4096` + json_schema + temperature.
2. Registrare risposta grezza (`finish_reason`, `usage`, presenza `reasoning_content`, `content`).
3. Ripetere con `max_tokens` ridotto e (se supportato) reasoning disattivato.
4. Annotare l'esito nella diagnostic spec.

## Evidence to Capture

- Risposte JSON grezze; tabella max_tokens × dimensione prompt → content pieno/vuoto + finish_reason.

## Out of Scope

- Implementazione dei fix (Ticket 02/03).

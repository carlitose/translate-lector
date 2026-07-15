# 04 — Task: lifecycle del sidecar llama-server

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

task

## Outcome

L'app avvia, monitora e ferma llama-server da sola secondo il contratto deciso nel grilling 03:
selezionando il provider `llamaserver` in ⚙️, la traduzione funziona senza che l'utente apra
terminali o Studio.

## Acceptance Criteria

- [ ] Spawn del server (binario secondo D1, parametri secondo D4, `--reasoning off` incluso)
      quando serve; nessun doppio spawn se già attivo/porta occupata da un server sano.
- [ ] Kill affidabile alla chiusura dell'app (nessun processo orfano; verificare anche il caso
      crash del frontend).
- [ ] Health integrata col probe esistente: se il server non è pronto, messaggio azionabile (non
      il generico "non raggiungibile").
- [ ] Test unitari sulla logica di lifecycle (stato, decisioni spawn/riuso) con processo mockato;
      test manuale end-to-end registrato nel ticket.
- [ ] Suite completa verde.

## Blocked By

- [03-grilling-llamacpp-direct-decisions.md](./03-grilling-llamacpp-direct-decisions.md)

## Frontier

È il cuore della destinazione: senza lifecycle gestito, "togliere Unsloth" resta un comando
PowerShell da ricordare.

## Work Plan

1. Da dettagliare dopo il grilling (D1 decide dove vive il binario, D4 i parametri).
2. Implementare con TDD sulla logica pura (decisione spawn/riuso/kill).
3. Wiring in `lib.rs` (setup/teardown Tauri) e messaggi d'errore.

## Evidence to Capture

- Log di spawn/kill, esito del test manuale con l'app dev.

## Out of Scope

- Download/gestione del GGUF (ticket 05).
- Rimozione preset unsloth (ticket 06).

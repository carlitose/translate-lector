# 05 — Task: gestione del modello GGUF

## Parent Spec

[llamacpp-direct-wayfinder.md](../../specs/llamacpp-direct-wayfinder.md)

## Type

task

## Outcome

L'app sa quale file GGUF passare a llama-server secondo la decisione D2 del grilling: risoluzione
del path (cache HuggingFace esistente / path configurato / download gestito), con errore azionabile
se il modello manca.

## Acceptance Criteria

- [ ] Risoluzione del path del GGUF implementata secondo D2, con default che funziona
      sull'installazione attuale (cache HF di gemma-4-E2B-it-qat-UD-Q4_K_XL).
- [ ] Se il file manca: messaggio che dice esattamente cosa fare (non un errore di spawn opaco).
- [ ] Eventuale UI in ⚙️ (path picker o stato del modello) secondo D2.
- [ ] Test unitari sulla risoluzione del path; suite verde.

## Blocked By

- [03-grilling-llamacpp-direct-decisions.md](./03-grilling-llamacpp-direct-decisions.md)

## Frontier

Il sidecar (04) non parte senza un `-m <path>` valido: questa è la sua unica dipendenza dati.

## Work Plan

1. Da dettagliare dopo il grilling (D2).
2. TDD sulla risoluzione del path (esiste/non esiste/override).
3. Wiring col ticket 04.

## Evidence to Capture

- Path risolti nei tre casi; screenshot ⚙️ se c'è UI.

## Out of Scope

- Download di modelli diversi da gemma-4 / model picker generale.

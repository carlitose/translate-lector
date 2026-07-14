# 04 — Decisioni di prodotto OCR (vista, export, fedeltà, lingue)

## Parent Spec

[ocr-layout-translation-wayfinder.md](../../specs/ocr-layout-translation-wayfinder.md)

## Type

grilling

## Outcome

Le decisioni umane che il design a valle (Ticket 05 e le build verticali) non può indovinare, registrate
come risposte o assunzioni esplicite nel parent spec.

## Acceptance Criteria

- [ ] **Vista**: la ricostruzione facsimile è la **vista primaria** (sostituisce l'affiancata per i PDF
      scansionati) o una **modalità aggiuntiva** selezionabile? Vale anche per i PDF con testo estraibile?
- [ ] **Pavimento di fedeltà v1**: se il Ticket 03 mostra che la ricostruzione perfetta è costosa,
      l'overlay-su-immagine è accettabile come v1 (con re-typeset completo come v2)? Sì/No.
- [ ] **Export**: serve esportare la pagina/documento tradotto (PDF/immagine) o basta la visualizzazione
      in-app? Se sì, quale formato.
- [ ] **Lingue OCR al lancio**: quali `traineddata` bundlare di default (es. eng, ita, fra, deu, spa)?
      Selezione lingua sorgente manuale, auto-detect, o entrambi?
- [ ] **Performance/caching**: latenza OCR accettabile per pagina; l'OCR va cachato in DB come le
      traduzioni? Prefetch OCR della pagina successiva sì/no?
- [ ] **Documenti misti**: routing quando un PDF ha sia pagine con testo estraibile sia pagine scansionate
      (per-pagina automatico vs scelta utente).
- [ ] Ogni punto ha una **decisione o un'assunzione esplicita** registrata nel parent spec.

## Blocked By

- None per iniziare a raccogliere domande; ma le risposte su "pavimento di fedeltà" sono più informate
  **dopo** i Ticket 02/03. Idealmente condurre il grilling dopo aver visto gli spike.

## Frontier

È il gate umano prima di chiudere il design (Ticket 05). Alcune risposte (vista, export, pavimento di
fedeltà) cambiano radicalmente lo scope delle build verticali.

## Work Plan

1. Preparare un decision brief con le domande sopra e le opzioni (stile `decision-brief-grilling-03.md`).
2. Portare gli screenshot/verdetti dei Ticket 02/03 come evidenza a supporto.
3. Condurre il grilling; registrare risposte o assunzioni per ciascun punto.
4. Ripiegare le decisioni in "Decisions So Far" del parent spec.

## Evidence to Capture

- Decision brief con le risposte D-numerate.
- Eventuali assunzioni prese in assenza di risposta, marcate come tali.

## Out of Scope

- Implementazione. Questo ticket produce solo decisioni.

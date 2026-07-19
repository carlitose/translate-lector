# 04 — Implementare il guardrail approvato sui prerequisiti Git

## Parent Spec

[pdfjs-wkwebview-text-extraction-diagnosis.md](../../specs/pdfjs-wkwebview-text-extraction-diagnosis.md)

## Type

AFK

## What to Build

Implementare nel punto di ownership approvato dal ticket 03 un preflight che verifichi i
commit o PR prerequisiti prima di creare un branch di lavoro. Se un prerequisito dichiarato
non è antenato del baseline, il flusso deve fermarsi con una diagnosi chiara oppure seguire
esplicitamente la strategia stacked/incorporation scelta; non deve proseguire su una base
silenziosamente incompleta.

Questa slice realizza il goal di processo e la correzione organizzativa descritta in
**Decision / Solution** e **Follow-Up Tickets** della parent spec.

## Acceptance Criteria

- [ ] Il guardrail è implementato soltanto nell'owner e con l'autorità approvati dal
      ticket 03.
- [ ] Il preflight accetta prerequisiti identificabili come commit e, se deciso, PR/branch,
      e verifica in modo deterministico la loro presenza nel baseline.
- [ ] Un prerequisito già antenato permette di proseguire; uno assente produce un blocco
      esplicito con opzioni conformi alla policy approvata.
- [ ] Il flusso non confonde test verdi sul baseline con prova che il prerequisito sia
      presente.
- [ ] Test o dry-run coprono prerequisito presente, PR aperta/non antenata e identificatore
      invalido o irraggiungibile.
- [ ] La documentazione spiega come dichiarare dipendenze, come risolvere il blocco e come
      evitare di perdere insieme fix e regression test.

## Blocked By

- [03-decide-branch-prerequisite-guardrail-owner.md](./03-decide-branch-prerequisite-guardrail-owner.md)

## Frontier

Blocked by ticket 03. Diventa AFK e immediatamente eseguibile quando il decision record
specifica owner, autorità, input dei prerequisiti e policy fail/stack/incorporate.

## Step-by-Step Implementation Plan

1. Leggere il decision record del ticket 03 e tradurlo in un contratto di preflight con
   input, output, fallimenti e punto esatto del workflow in cui viene eseguito.
2. Implementare la verifica ancestry prima di qualsiasi branch creation o quality loop.
   Non modificare prodotto o repository fuori dall'owner autorizzato.
3. Gestire in modo esplicito prerequisiti assenti, remoti non disponibili e PR non
   mergiate; non usare fallback silenziosi a `main`.
4. Aggiungere test/dry-run sui tre scenari minimi e un caso che riproduca `da673dd` come
   baseline con `11c320b` non antenato.
5. Documentare dichiarazione e risoluzione dei prerequisiti, poi eseguire i quality gate
   previsti dall'owner scelto.

## Testing Plan

- Test automatici o harness del preflight per ancestry positiva e negativa.
- Dry-run con commit prerequisito su PR aperta e baseline che ne è il padre.
- Verifica che nessun branch venga creato quando la policy richiede il blocco.
- Se l'owner è una skill globale, testare anche che repository senza prerequisiti
  dichiarati mantengano il comportamento esistente.

## Out of Scope

- Fix PDF.js, QA Tauri, modifica della decisione del ticket 03 e merge automatico di PR
  prerequisiti.

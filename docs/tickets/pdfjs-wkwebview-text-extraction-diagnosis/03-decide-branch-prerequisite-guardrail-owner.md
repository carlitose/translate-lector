# 03 — Decidere ownership e policy del guardrail sui prerequisiti Git

## Parent Spec

[pdfjs-wkwebview-text-extraction-diagnosis.md](../../specs/pdfjs-wkwebview-text-extraction-diagnosis.md)

## Type

HITL

## What to Build

Produrre una decisione umana esplicita su dove deve vivere il controllo che impedisce a
un nuovo lavoro di partire da `main` quando dipende da un fix ancora non mergiato. La
decisione deve scegliere l'owner del guardrail — workflow/documentazione del repository,
skill globale `ticket-autopilot`, oppure entrambi — e definire il comportamento quando il
prerequisito non è antenato del branch di destinazione.

Questa slice copre il goal di processo, la seconda parte di **Decision / Solution** e la
scelta operativa indicata in **Open Questions** della parent spec.

## Acceptance Criteria

- [ ] Sono confrontate almeno le opzioni repo-local, skill globale e controllo combinato,
      con impatto, portabilità e manutenzione.
- [ ] Un umano sceglie owner, punto di enforcement e autorità necessaria per modificarlo.
- [ ] La policy definisce come dichiarare un commit/PR prerequisito e cosa fare quando non
      è antenato: fermarsi, creare branch stacked o incorporare esplicitamente il slice.
- [ ] È vietato dichiarare il ticket completo basandosi solo su test verdi se il baseline
      non contiene i prerequisiti dichiarati.
- [ ] La decisione viene registrata in un artefatto durevole e collegata dal ticket 04.

## Blocked By

- None - can start immediately.

## Frontier

Blocked by human input. La decisione richiesta è: **il guardrail deve essere specifico di
questo repository, una modifica alla skill globale di autopilot, o una difesa a due
livelli?** Il ticket può preparare una raccomandazione AFK, ma non può scegliere da solo
l'owner o autorizzare una modifica fuori dal repository.

## Step-by-Step Implementation Plan

1. Descrivere il failure mode osservato: branch creato dal padre del fix, con codice e
   test correttivi esclusi contemporaneamente.
2. Mappare i possibili punti di controllo: istruzioni/preflight repo-locali, logica della
   skill globale, oppure entrambi. Per ciascuno indicare copertura e casi non protetti.
3. Raccomandare una policy minima con identificatore del prerequisito, verifica ancestry e
   comportamento fail-closed prima della creazione del branch.
4. Ottenere la scelta umana su owner e scope. Registrare anche chi può modificare e
   distribuire il punto scelto.
5. Salvare la decisione in un breve decision record o nella sezione di completamento del
   ticket, con contratto sufficientemente preciso per il ticket 04.

## Testing Plan

- Validare la policy su tre scenari descritti: prerequisito già in `main`, prerequisito in
  PR aperta non antenata e prerequisito sconosciuto/non disponibile.
- Verificare che la scelta distingua chiaramente warning informativo e blocco fail-closed.
- Nessuna modifica eseguibile viene testata in questo ticket; l'enforcement è ticket 04.

## Out of Scope

- Implementazione del guardrail, port del fix PDF.js e QA Tauri.

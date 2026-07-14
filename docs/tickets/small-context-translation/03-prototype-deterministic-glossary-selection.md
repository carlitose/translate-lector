## Parent Spec

[small-context-translation-wayfinder.md](../../specs/small-context-translation-wayfinder.md)

## Type

prototype

## Outcome

Prova dell'**idea chiave**: una funzione **deterministica** (codice, nessuna chiamata LLM) che, dato il
testo di un'unità e il glossario completo del documento, restituisce **solo i termini rilevanti** a
quell'unità, tagliando drasticamente il prompt senza perdere i vincoli `locked`.

## Acceptance Criteria

- [ ] Funzione `select_glossary(unit_text, entries) -> Vec<GlossaryTerm>` che include un termine quando il
      suo `source_term` compare nell'unità: match **case-insensitive**, su **confini di parola**, con
      supporto **multiword** e varianti morfologiche semplici (plurale/maiuscole); documentare i limiti.
- [ ] I termini **locked** presenti nell'unità sono SEMPRE inclusi (vincolo assoluto preservato); cap
      opzionale sul numero di unlocked, con locked prioritari.
- [ ] Misurata la riduzione di dimensione del prompt-glossario su un glossario realistico (es. 50-200
      termini) vs "invia tutto": riportare token risparmiati per unità.
- [ ] Falsi negativi noti (termine rilevante non matchato per morfologia) elencati; strategia di mitigazione.
- [ ] Prototipo/test registrato; verdetto su severità/cap del match come input al Ticket 05.

## Blocked By

- None - can start immediately.

## Frontier

Ready. È l'idea centrale dell'utente ("funzioni programmate per selezionare il glossario giusto"); il
maggior risparmio di contesto viene da qui.

## Work Plan

1. Rivedere il modello glossario (`src-tauri/src/glossary.rs`, `render_locked_unlocked`) e come è iniettato
   oggi (`translate.rs:229-230`).
2. Implementare `select_glossary` deterministica (match parola/multiword/case, locked-first, cap).
3. Misurare la riduzione su un glossario realistico; elencare falsi negativi e mitigazioni.
4. Annotare la severità di match raccomandata (Ticket 05).

## Evidence to Capture

- Casi di test match/non-match; token risparmiati per unità; lista falsi negativi.

## Out of Scope

- Selezione via LLM (alternativa scartata salvo diverso esito); chunking (Ticket 02); orchestrazione (04).

# 03 — Frontend: form "Aggiungi termine" nel pannello glossario

## Parent Spec

[manual-glossary-entry-wayfinder.md](../../specs/manual-glossary-entry-wayfinder.md)

## Type

task

## Outcome

Dal pannello glossario l'utente aggiunge un nuovo termine end-to-end: compila un piccolo
form (termine, traduzione, tipo, nota, lucchetto), invia, e il termine compare nella lista.
Duplicato e validazione gestiti con messaggi non intrusivi.

## Acceptance Criteria

- [ ] `GlossaryPanel.svelte` mostra un form di aggiunta (una riga/area "Aggiungi termine")
      con input per `source_term`, `translation`, `type` (testo libero + datalist di
      suggerimenti se deciso in 01), `note`, e checkbox `locked` (default da 01), + pulsante
      "Aggiungi".
- [ ] Il pulsante è disabilitato finché `source_term` e `translation` non sono validi
      (riuso/estensione di `isValidTranslation` in `glossary.ts`; nuovo tipo `AddTermArgs` +
      helper `toAddArgs`/validazione, testati).
- [ ] Su invio chiama il comando `add_glossary_term`; a successo ricarica la lista
      (`list_glossary`) e mostra conferma ("«X» aggiunto"); il form si azzera.
- [ ] Su duplicato (**decisione 01 #2**: `AddOutcome::Duplicate(id)`) NON aggiunge righe:
      apre in modifica la riga esistente — scrolla/evidenzia quella riga nella tabella e ne
      attiva l'editing inline (translation/nota/lucchetto), con una nota non intrusiva del
      tipo "«X» esiste già — aperto in modifica".
- [ ] I termini a mano mostrano "manuale" (non uno `0`) nella colonna "Pag.".
- [ ] Test TS (vitest) per gli helper di `glossary.ts` (validazione, mapping argomenti,
      etichetta pagina "manuale"). `npm test` + `npm run check` verdi.

## Blocked By

- 02-task-backend-add-command.md

## Frontier

Ultimo passo: espone in UI il contratto backend del ticket 02. Dopo questo la destinazione
del map è raggiunta.

## Work Plan

1. RED: test in `src/lib/glossary.test.ts` per i nuovi helper (`toAddArgs`,
   validazione del nuovo termine, etichetta "manuale" per `first_seen_page===0`).
2. GREEN: helper/tipi in `glossary.ts`.
3. UI: form di aggiunta in `GlossaryPanel.svelte` (riuso dello stile esistente, nessun nuovo
   modale), wiring del comando, ricarica lista, gestione conferma/duplicato/validazione.
4. `npm test` + `npm run check`; verifica manuale nel modale.

## Evidence to Capture

- Screenshot/descrizione del form; risultati test; nota della verifica manuale end-to-end
  (aggiunta → compare in lista → se locked, entra nei vincoli assoluti del prompt).

## Out of Scope

- Backend (ticket 02). Eliminazione di termini. Import/export. Modifica del source_term.

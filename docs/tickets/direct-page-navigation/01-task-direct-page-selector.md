# 01 — Selettore diretto della pagina nel footer

## Parent Spec

[direct-page-navigation.md](../../specs/direct-page-navigation.md)

## Type

task

## Outcome

Con un PDF aperto, l'utente digita una pagina valida nel footer e la raggiunge direttamente
con Invio o uscendo dal campo, senza attraversare una alla volta le pagine intermedie.

## Acceptance Criteria

- [ ] Il footer conserva i pulsanti ◀/▶ e mostra un input numerico accessibile nella forma
      `Pag. [N] / totale`, con `min=1`, `max=totalPages`, `step=1` e label comprensibile
      agli screen reader.
- [ ] L'input usa uno stato bozza separato da `currentPage`; digitare non muta la pagina
      corrente né avvia una traduzione finché il valore non viene confermato.
- [ ] Invio e perdita del focus confermano un intero compreso tra 1 e `totalPages` tramite
      il `goTo(pageNo)` esistente. Il doppio evento submit/blur non causa una seconda
      navigazione effettiva alla stessa pagina.
- [ ] Un valore vuoto, decimale, non numerico o fuori intervallo non cambia pagina e il
      campo torna a mostrare `currentPage`.
- [ ] Dopo un salto valido, canvas, numero pagina, testo ricostruito, traduzione e sessione
      persistita si riferiscono tutti alla destinazione; le pagine intermedie non vengono
      elaborate.
- [ ] Il selettore è disabilitato o non azionabile quando non è aperto alcun PDF e torna
      sincronizzato quando si apre/ripristina un documento o si naviga con le frecce.
- [ ] La validazione/normalizzazione della destinazione è estratta in un helper puro e
      coperta da Vitest almeno per limiti, decimali, vuoto e pagina valida.
- [ ] `npm test` e `npm run check` sono verdi; una prova manuale copre prima pagina, ultima
      pagina, salto lungo, input invalido e ripristino della sessione.

## Blocked By

- None - can start immediately.

## Frontier

`goTo` supporta già una destinazione arbitraria. Il lavoro consiste nel raccogliere il
numero senza scrivere prematuramente `currentPage`, così da preservare l'allineamento tra
pagina renderizzata e testo tradotto.

## Work Plan

1. Aggiungere a `src/lib/session.ts` (o a un modulo UI dedicato, se emerge più coerente)
   un helper puro che accetti la bozza e `totalPages`, restituendo una pagina valida oppure
   nessuna destinazione; coprirlo in `src/lib/session.test.ts`.
2. Introdurre in `src/routes/+page.svelte` lo stato bozza e la sua sincronizzazione dopo
   apertura/ripristino documento e navigazione riuscita.
3. Sostituire l'indicatore statico nel footer con il controllo numerico; gestire Invio e
   blur facendo convergere entrambi su un'unica funzione di commit che riusa `goTo`.
4. Adeguare lo stile del footer per mantenere leggibili controllo, totale e azioni anche
   con molte cifre e a finestra stretta.
5. Eseguire test/check e la verifica manuale indicata nei criteri di accettazione.

## Evidence to Capture

- Risultati di `npm test` e `npm run check`.
- Nota o screenshot dei salti 1 → N e N → ultima pagina, inclusa la corrispondenza tra
  canvas, indicatore e traduzione.
- Esito del riavvio/ripristino sulla pagina raggiunta direttamente e della prova con input
  invalido.

## Out of Scope

- Backend/Tauri, schema dati, traduzione delle pagine saltate, nuove strategie di prefetch,
  miniature, indice e ricerca testuale.

## Progress

- Implementato il selettore con bozza separata, commit condiviso Invio/blur, validazione
  pura e sincronizzazione dopo apertura, ripristino e navigazione. Aggiunti anche
  cancellazione dei render PDF.js obsoleti e guardie di generazione documento.
- Verifica automatica completata: `npm test` (100 test verdi), `npm run check` (0 errori e
  0 warning), `npm run build` e `git diff --check` riusciti.
- Le prime due iterazioni di review hanno corretto la contesa sul canvas, i commit stale
  durante il cambio documento e la sintassi numerica permissiva.

## Autopilot Status

**Blocked — needs human (quality iteration limit reached, 3/3).**

- Finding `high` residuo in `src/routes/+page.svelte`: `loadDocument` pubblica il nuovo
  `pdfDoc` prima che `session`, `currentPage` e `totalPages` vengano sostituiti. Durante
  gli `await` successivi le frecce restano azionabili; `goTo` o un prefetch proveniente
  dalla traduzione precedente possono quindi combinare il nuovo PDF con la vecchia
  sessione/document ID.
- Fix raccomandato: non pubblicare lo stato del nuovo documento finché la relativa
  sessione non è pronta, oppure azzerare subito `session`/`totalPages` e bloccare ogni
  navigazione durante `loading`; prima del prefetch rivalidare anche request/sequence.
  Aggiungere un test di regressione per questa finestra di transizione.
- Resta inoltre da eseguire la prova manuale Tauri con PDF multipagina per prima/ultima
  pagina, salto lungo, input invalido, allineamento canvas/traduzione e ripristino.

# Navigazione diretta a una pagina

## Type

Wayfinding spec

## Status

Active

## Destination

L'utente può raggiungere direttamente una pagina specifica di un PDF aperto, senza
premere ripetutamente i pulsanti precedente/successiva. Nel footer può inserire un numero
`N` compreso tra 1 e il totale delle pagine e confermarlo; l'app mostra, traduce e salva
subito la pagina `N` come pagina corrente.

## Decisions So Far

- La navigazione resta **page-discrete e 1-based**. `currentPage` parte da 1,
  `totalPages` viene da PDF.js e i pulsanti esistenti invocano `goTo(currentPage ± 1)`
  (`src/routes/+page.svelte`; `src/lib/session.ts`).
- Il percorso applicativo necessario esiste già: `goTo(pageNo)` accetta qualunque pagina
  valida, azzera lo stato della traduzione precedente, aggiorna `currentPage`, chiama
  `showPage` e persiste la sessione. La feature è quindi frontend-only e deve riusare
  `goTo`, non introdurre un secondo flusso di navigazione.
- Il selettore sarà un controllo numerico inline nel footer, nella forma
  `Pag. [N] / totale`, mantenendo i pulsanti ◀ e ▶. L'utente conferma con Invio o uscendo
  dal campo; questa è un'assunzione UX reversibile basata sulla richiesta corrente.
- Il valore digitato deve vivere in uno stato bozza separato da `currentPage`. Collegare
  direttamente l'input a `currentPage` violerebbe l'invariante già protetta da
  `reconstructedPage === currentPage`: la reattività potrebbe avviare una traduzione
  prima che `showPage` abbia estratto il testo della destinazione.
- Il salto va direttamente a `N`: non renderizza, traduce o aggiorna in background le
  pagine intermedie. Il normale `advance_context` viene eseguito per la pagina visitata,
  come per qualsiasi altra navigazione reale.

## Not Yet Specified

- Nessuna decisione bloccante. Durante l'implementazione va solo confermato il dettaglio
  visuale del controllo alle larghezze ridotte; il comportamento funzionale è definito.
- Assunzione per input non valido (vuoto, decimale, minore di 1 o maggiore del totale):
  non navigare e ripristinare nel campo la pagina corrente, affidando anche `min`, `max`
  e `step` alla validazione nativa del controllo.

## Out of Scope

- Miniature, indice visuale, slider, dropdown con tutte le pagine o ricerca nel PDF.
- Elaborazione automatica delle pagine saltate per ricostruire un contesto strettamente
  sequenziale.
- Modifiche ai comandi Tauri, allo schema delle sessioni, alla cache di traduzione o alla
  logica di prefetch della sola pagina successiva.
- Scorciatoie globali aggiuntive oltre alla conferma con Invio nel selettore.

## Frontier / Blocking Edges

- **Edge — controllo numerico senza race reattive (ticket 01).** Serve introdurre e
  sincronizzare una bozza indipendente, validarla e passare la destinazione a `goTo` solo
  al commit. È l'unico bordo da attraversare; non ci sono dipendenze backend.

## Ticket Plan

- **01 — task — Selettore diretto della pagina nel footer.** Aggiungere il controllo,
  la validazione testabile e il wiring a `goTo`; preservare frecce, traduzione corretta,
  persistenza e accessibilità. Output: salto diretto a una pagina valida verificato da
  test automatici e controllo manuale.

## Next Review

Dopo il ticket 01 verificare con un PDF multipagina i salti 1 → N, N → 1 e verso prima/
ultima pagina; controllare che canvas, indicatore, traduzione e sessione ripristinata
rimangano allineati e che un valore invalido non cambi pagina.

# 01 — Correggere l'estrazione testo PDF su WKWebView

## Parent Spec

[pdfjs-wkwebview-text-extraction-diagnosis.md](../../../specs/pdfjs-wkwebview-text-extraction-diagnosis.md)

## What to Build

Correggere end-to-end l'apertura dei PDF testuali su macOS senza cambiare PDF.js o il
core Rust. Per i PDF ordinari il frontend deve evitare `PDFPageProxy.getTextContent()`,
che nella WebView macOS percorre internamente un `ReadableStream` con un iteratore
asincrono non invocabile. Il ramo anticipato pure XFA resta affidato a
`getTextContent()`, perché PDF.js vi ritorna prima di creare o iterare lo stream.

Come deciso nelle sezioni **Decision / Solution** e **Implementation Plan** della parent
spec, introdurre un adattatore TypeScript testabile che consumi
`PDFPageProxy.streamTextContent()` tramite `getReader()`, aggreghi gli item nello stesso
ordine e rilasci sempre il lock. Collegare l'adattatore all'unico flusso di estrazione
usato da apertura, guardia EC01, navigazione e prefetch, lasciando invariata la
ricostruzione del testo.

La slice è completa quando un PDF testuale arriva dai byte letti dal core fino al canvas
e al testo ricostruito su WKWebView, senza il `TypeError`, e il comportamento esistente
rimane valido su WebView2.

## Acceptance Criteria

- [x] Le pagine ordinarie non usano `PDFPageProxy.getTextContent()` e consumano i chunk
      di `streamTextContent()` esclusivamente tramite un reader esplicito; l'unica
      eccezione è il ramo strettamente guardato `isPureXfa`, che preserva l'estrazione
      XFA senza entrare nell'iterazione dello stream.
- [x] Gli item emessi da più chunk vengono restituiti nello stesso ordine, senza perdite
      o duplicazioni, nel formato già accettato dalla ricostruzione esistente.
- [x] Il lock del reader viene rilasciato sia al completamento sia quando `read()` fallisce;
      l'errore originale continua a propagarsi al gestore dell'apertura.
- [x] Apertura iniziale, controllo EC01, navigazione e prefetch passano tutti attraverso
      l'adattatore, senza duplicare la logica di consumo dello stream.
- [x] I test coprono stream multi-chunk, stream vuoto, errore di lettura e pure XFA; i
      test di ricostruzione PDF esistenti restano verdi.
- [x] `npm test`, `npm run check`, `npm run build` e i test Rust esistenti terminano con
      successo.
- [ ] QA macOS: un fixture PDF testuale e un PDF reale si aprono, mostrano canvas e testo
      ricostruito e permettono la navigazione senza il `TypeError`.
- [ ] QA EC01: un PDF senza testo estraibile mostra ancora
      `formato non supportato (no OCR)`.
- [ ] QA Windows, quando disponibile: lo stesso flusso non regredisce su WebView2. Se la
      macchina Windows non è disponibile durante l'esecuzione AFK, registrare il check
      come follow-up manuale senza cambiare la soluzione.

## Blocked By

- None - can start immediately.

## Frontier

**Done — automated scope complete.** Le verifiche native/manuali elencate nei criteri
non bloccano la correzione e restano follow-up espliciti.

## Step-by-Step Implementation Plan

1. **Definire il confine testabile dello stream.** Creare un modulo frontend dedicato
   all'estrazione degli item dai chunk PDF.js. Esporre una funzione che riceve il minimo
   contratto necessario di una pagina/stream, incluso il ramo anticipato `isPureXfa`;
   per le pagine ordinarie chiama `streamTextContent()` una sola volta e acquisisce il
   reader. Questo viene prima dell'integrazione per permettere un feedback loop unitario
   senza worker o WebView reali. Evitare di copiare tipi o implementazione interna di
   PDF.js oltre ai campi effettivamente consumati.
2. **Consumare e chiudere il reader.** Leggere in sequenza fino a `done`, concatenando gli
   `items` di ogni chunk. Racchiudere il ciclo in `try/finally` e rilasciare il lock nel
   `finally`; non ingoiare gli errori di `read()`. Verificare con un reader finto che
   ordine, terminazione e propagazione dell'errore siano corretti. Non usare
   `for await`, direttamente o attraverso un helper, perché riprodurrebbe il confine
   incompatibile.
3. **Aggiungere i test dell'adattatore.** Coprire almeno due chunk con item distinti,
   `done` immediato e un `read()` che rigetta. Verificare il rilascio del lock in tutti i
   percorsi. Coprire inoltre `isPureXfa`, verificando che usi `getTextContent()` senza
   aprire lo stream. Asserire il risultato pubblico, non il numero di dettagli interni
   non necessari alla correttezza.
4. **Integrare nel flusso PDF.** Sostituire nella funzione condivisa di estrazione pagina
   il consumo ordinario di `getTextContent()` con l'adattatore, mantenendo l'eccezione
   pure XFA al suo interno. Conservare viewport, mapping degli item e chiamata a
   `reconstruct`. Poiché apertura, campionamento EC01, navigazione e prefetch usano già la
   stessa funzione, verificare che non vengano introdotti percorsi alternativi o chiamate
   residue a `getTextContent()` fuori dal ramo pure XFA.
5. **Eseguire la regressione automatica.** Lanciare test frontend, type check e build,
   poi i test Rust come controllo che il confine IPC non sia stato alterato. Cercare nel
   frontend eventuali usi residui di `getTextContent()` e distinguere il prototipo
   storico dal percorso dell'app.
6. **Eseguire la QA live.** Su macOS aprire un fixture testuale del prototipo e un PDF
   reale, navigare tra più pagine e verificare canvas e testo. Aprire inoltre un PDF
   immagine per EC01. Ripetere su Windows se disponibile; altrimenti annotare chiaramente
   la verifica WebView2 ancora da eseguire, senza bloccare la consegna del fix.

## Testing Plan

- **Unit frontend:** nuovo test Vitest per l'adattatore con reader finto multi-chunk,
  vuoto e fallibile; verifica esplicita di ordine, propagazione errore e `releaseLock`.
  Il ramo pure XFA deve essere coperto separatamente e non deve aprire lo stream.
- **Regressione frontend:** mantenere verdi i test di ricostruzione in `pdfExtract` e
  l'intera suite; eseguire `npm test`, `npm run check` e `npm run build`.
- **Regressione core:** eseguire i test Rust esistenti; non sono previste modifiche al
  comando binario `read_pdf_bytes`.
- **Ispezione statica:** nessun uso di `getTextContent()` deve restare nel percorso
  ordinario di produzione; è ammesso solo dietro `isPureXfa`, oltre al prototipo che non
  viene eseguito nella WebView.
- **Manuale macOS:** fixture testuale, PDF reale multipagina e PDF immagine/EC01 tramite
  `npm run tauri dev`.
- **Manuale Windows:** ripetere apertura e navigazione su WebView2 quando l'ambiente è
  disponibile; registrare l'esito o il follow-up.

## Out of Scope

- Polyfill globali di `ReadableStream` o modifiche al runtime WKWebView.
- Upgrade, downgrade o sostituzione di `pdfjs-dist`.
- Estrazione PDF nel backend Rust.
- OCR, miglioramenti alla ricostruzione del layout o nuovi formati documento.
- Modifiche a traduzione, provider locali/cloud, cache o persistenza delle sessioni.

## Completion Record — 2026-07-19

- Implementazione e due pass di review completati senza finding bloccanti.
- Verifica automatica: frontend 98/98, `npm run check` senza diagnostiche, build riuscita
  e Rust 314/314. Il test Rust portabile usa fixture con separatori nativi; nessun codice
  di produzione Rust è cambiato.
- QA simulata: il helper di produzione estrae testo dai fixture reali del repository;
  passano anche i percorsi negativo e di recupero da PDF corrotto. Nessun fallimento
  dell'implementazione rilevato.
- Follow-up manuali: GUI Tauri/WKWebView su macOS con PDF testuale, reale e immagine/EC01;
  flusso live dipendente da provider; pure XFA quando sarà disponibile un fixture;
  regressione GUI su Windows/WebView2.

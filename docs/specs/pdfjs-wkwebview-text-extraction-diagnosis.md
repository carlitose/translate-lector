# Diagnosi — Estrazione testo PDF.js fallisce su macOS WKWebView

## Type

Diagnostic spec

## Status

Accepted / Implemented (verifica automatica e su fixture completata; QA GUI live
cross-platform ancora da registrare)

## Problem / Context

Su macOS l'app Tauri permette di scegliere un PDF, ma durante l'apertura mostra:

> Errore nell'apertura del PDF: TypeError: undefined is not a function (near '...value of readableStream...')

Lo stesso flusso non presenta l'errore su Windows. Tauri usa motori web forniti dal
sistema operativo: Microsoft Edge WebView2/Chromium su Windows e WKWebView/WebKit su
macOS. Il core Rust non renderizza né estrae il testo: legge i byte dal file e li passa
al frontend, che usa `pdfjs-dist` 6.1.200.

Il documento viene caricato correttamente da PDF.js. Il fallimento avviene subito dopo,
quando la guardia EC01 e il rendering della pagina chiedono il testo tramite
`PDFPageProxy.getTextContent()`. Per i PDF ordinari questa API aggrega internamente uno
stream con `for await (... of readableStream)`. Nel WKWebView osservato l'iteratore
asincrono dello stream non è invocabile, mentre WebView2 esegue lo stesso codice senza
errore. PDF.js gestisce invece le pagine pure XFA con un ritorno anticipato che non crea
né itera lo stream.

## Goals

- Aprire ed estrarre il testo dei PDF su macOS WKWebView senza dipendere
  dall'iterazione asincrona di `ReadableStream`.
- Conservare il comportamento esistente su Windows, inclusi ordine degli item,
  ricostruzione del testo, rendering e navigazione.
- Conservare l'estrazione pure XFA fornita da PDF.js senza esporre i PDF ordinari
  all'iteratore asincrono incompatibile.
- Mantenere invariata la guardia EC01 per i PDF senza testo estraibile.
- Rendere la compatibilità dello stream verificabile con test frontend mirati.

## Non-Goals

- Sostituire PDF.js o spostare l'estrazione PDF nel core Rust.
- Aggiungere OCR o supporto ai PDF immagine.
- Modificare traduzione, provider LLM, persistenza o gestione delle sessioni.
- Risolvere genericamente ogni possibile differenza tra WebView2 e WKWebView.

## Evidence

- Il messaggio osservato cita `value of readableStream`, che coincide con il corpo di
  `PDFPageProxy.getTextContent()` nella build installata di `pdfjs-dist` 6.1.200.
- Prima della correzione, `src/routes/+page.svelte` caricava i byte tramite il comando
  Tauri `read_pdf_bytes`, creava il documento PDF.js e poi chiamava `getTextContent()` da
  `extractPageText`; l'errore era catturato dal `catch` dell'intero flusso di apertura.
- `src-tauri/src/lib.rs` implementa `read_pdf_bytes` con una lettura del file e una
  risposta IPC binaria. Il selettore nativo si apre e il flusso raggiunge PDF.js, quindi
  non si tratta di un backend non compilato.
- `svelte-check` termina con zero errori e zero warning; `cargo check` completa la build
  del core e delle sue dipendenze.
- La build `legacy` inclusa nella stessa versione di PDF.js usa ancora il medesimo
  `for await (... of readableStream)`, quindi cambiare soltanto l'import non elimina
  questo specifico punto di incompatibilità.
- Tauri documenta che Windows usa WebView2 basato su Chromium e macOS usa WKWebView
  basato su WebKit; le differenze di runtime sono quindi parte del confine supportato
  dall'app multipiattaforma.

## Decision / Solution

Evitare `PDFPageProxy.getTextContent()` nel percorso ordinario di produzione. Introdurre
un piccolo adattatore frontend che:

1. se `PDFPageProxy.isPureXfa` è vero, usa il ramo `getTextContent()` di PDF.js, che
   ritorna prima di creare o iterare un `ReadableStream`;
2. per tutte le altre pagine ottiene lo stream con `PDFPageProxy.streamTextContent()`;
3. acquisisce un reader con `stream.getReader()`;
4. legge i chunk fino a `done`, preservando il loro ordine e concatenando gli item;
5. rilascia sempre il lock del reader, anche in caso di errore;
6. restituisce gli item nel formato già consumato dalla ricostruzione esistente.

`extractPageText` userà l'adattatore prima di passare gli item a `reconstruct`. In questo
modo cambia solo il meccanismo di consumo dello stream: coordinate, trasformazioni,
ricostruzione delle colonne e rilevamento EC01 restano invariati.

L'adattatore deve stare in un modulo TypeScript testabile, non direttamente nel
componente Svelte. Non va aggiunta una polyfill globale: la correzione deve restare
limitata al confine PDF.js che ha prodotto il problema.

## Options Considered

### Opzione 1: consumare `streamTextContent()` con `getReader()` — scelta

- Evita esattamente l'operazione non supportata e usa l'API reader già disponibile sullo
  stream.
- Mantiene la versione di PDF.js e limita il cambiamento all'estrazione testo.
- Richiede di aggregare esplicitamente i chunk e di gestire correttamente il lock.

### Opzione 2: aggiungere una polyfill globale per l'iteratore asincrono

- Permetterebbe di continuare a chiamare `getTextContent()`.
- Modifica globalmente `ReadableStream` e può interferire con PDF.js o altre dipendenze.
- Amplia il rischio oltre il singolo flusso che necessita della correzione.

### Opzione 3: usare la build legacy o ridurre la versione di PDF.js

- Potrebbe aumentare la compatibilità generale con runtime meno recenti.
- La build legacy 6.1.200 contiene lo stesso ciclo `for await`; un downgrade introdurrebbe
  inoltre un vincolo di versione senza correggere esplicitamente il confine difettoso.

### Opzione 4: estrarre il testo nel backend

- Eliminerebbe la dipendenza dalla WebView per l'estrazione.
- È una riscrittura architetturale sproporzionata e rischia di divergere dal rendering
  PDF.js e dalla logica di ricostruzione già validata.

## Implementation Plan

1. Estrarre il consumo dei chunk in un modulo frontend dedicato. Definire un contratto
   minimo basato sul reader, così i test non devono istanziare un worker PDF.js reale.
   Verificare ordine degli item, terminazione e rilascio del lock prima di integrarlo.
2. Collegare l'adattatore a `extractPageText` nel componente principale. Conservare il
   ramo anticipato `isPureXfa` di PDF.js; per le pagine ordinarie sostituire
   `getTextContent()` con il reader esplicito. Conservare mapping degli item, viewport e
   chiamata a `reconstruct`. Verificare che la guardia EC01 continui a campionare le
   pagine nello stesso modo.
3. Eseguire i test frontend esistenti, il type check e la build. Non modificare il core
   Rust; i suoi test devono restare verdi come controllo di regressione.
4. Avviare l'app Tauri su macOS e aprire almeno un fixture testuale del prototipo e un PDF
   reale. Verificare apertura, testo ricostruito, canvas e navigazione. Ripetere o far
   ripetere il flusso su Windows per confermare che WebView2 non regredisca.
5. Aprire un PDF senza testo estraibile e confermare che venga ancora mostrato
   `formato non supportato (no OCR)` invece di un errore generico.

## Testing Decisions

- Unit test dell'adattatore con un reader finto che emette più chunk: gli item devono
  essere concatenati nello stesso ordine e il lock deve essere rilasciato.
- Unit test del ramo pure XFA: deve usare `getTextContent()` senza aprire lo stream.
- Unit test dei percorsi `done` immediato ed errore di lettura, incluso il rilascio del
  lock nel `finally`.
- Mantenere verdi i test di `pdfExtract`, che validano la ricostruzione indipendentemente
  dal meccanismo di streaming.
- Eseguire `npm test`, `npm run check`, `npm run build` e i test Rust esistenti.
- QA manuale obbligatoria su macOS WKWebView perché il difetto dipende dal motore web
  incorporato; controllo di regressione su Windows WebView2 quando disponibile.
- Non testare dettagli interni di PDF.js né copiare la sua implementazione: i test devono
  coprire il contratto del reader e il comportamento visibile dell'app.

## Follow-Up Tickets

- Un ticket AFK sotto `docs/tickets/pdfjs-wkwebview-text-extraction-diagnosis/` per
  introdurre l'adattatore `getReader()`, integrarlo nel flusso PDF e aggiungere i test.
  La QA live su macOS resta un criterio manuale del ticket, non un secondo intervento
  architetturale.

## Open Questions

- Nessuna domanda bloccante. Se non è disponibile una macchina Windows durante
  l'implementazione, la verifica WebView2 può essere registrata come follow-up manuale
  senza bloccare la correzione macOS.

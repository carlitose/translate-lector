# Diagnosi — Regressione dell'estrazione PDF.js su macOS WKWebView

## Type

Diagnostic spec

## Status

Accepted / Implemented in PR #15 (consenso 3/3, confidenza alta; scope automatico
completato, QA nativa macOS ancora nel ticket 02)

## Problem / Context

Nel branch `autopilot/direct-page-selector`, aprendo un PDF nell'app Tauri su macOS,
l'utente riceve nuovamente:

> Errore nell'apertura del PDF: TypeError: undefined is not a function (near '...value of readableStream...')

Il difetto era già stato diagnosticato e corretto nel commit `11c320b` del branch
`autopilot/fix-wkwebview-pdf-text-extraction` (PR #14). La regressione non è stata
introdotta dal comportamento del selettore di pagina: è una **regressione di composizione
del branch**. Per isolare il nuovo lavoro, l'orchestrazione è passata dal branch corretto a
`main` e ha creato `autopilot/direct-page-selector` da `da673dd`; quel commit è esattamente
il padre di `11c320b`, quindi non contiene né l'adattatore WKWebView né i suoi test.

Nel codice corrente `extractPageText` chiama di nuovo `PDFPageProxy.getTextContent()`.
Con `pdfjs-dist` 6.1.200, per i PDF ordinari questa API aggrega internamente
`streamTextContent()` usando `for await (... of readableStream)`. Il WKWebView interessato
espone `ReadableStream.getReader()` ma non un iteratore asincrono invocabile. L'eccezione
si verifica durante il campionamento EC01 di `hasExtractableText` e viene trasformata dal
`catch` di `loadDocument` nel messaggio italiano osservato.

## Goals

- Ripristinare l'estrazione testo compatibile con macOS WKWebView senza perdere il lavoro
  corrente sulla navigazione diretta, le guardie di generazione documento e la
  cancellazione dei render PDF.js.
- Rendere esplicito perché 100 test verdi, type check e build non hanno protetto questa
  compatibilità runtime.
- Aggiungere una copertura che verifichi sia l'adattatore reader-based sia il suo wiring
  nel percorso di produzione.
- Introdurre un controllo di processo per non costruire nuovi branch sopra una base che
  esclude fix prerequisiti ancora non mergiati.

## Non-Goals

- Sostituire PDF.js, spostare l'estrazione nel core Rust o aggiungere OCR.
- Modificare traduzione, provider LLM, cache, schema o persistenza delle sessioni.
- Revertire il selettore diretto della pagina o le correzioni di concorrenza già presenti
  nel working tree.
- Usare una polyfill globale di `ReadableStream` o affidarsi alla build legacy di PDF.js.

## Evidence

### Consenso triangolato

Tre diagnosi indipendenti — repro-first, data-flow e recent-change/environment — hanno
identificato lo stesso meccanismo e costruito lo stesso feedback loop. Non sono emerse
divergenze sulla causa.

### Topologia Git e perdita del fix

- `git rev-parse HEAD 11c320b^` restituisce in entrambi i casi `da673dd`: il branch
  corrente è basato sul padre del fix.
- Il reflog registra il passaggio da `autopilot/fix-wkwebview-pdf-text-extraction` a
  `main`, poi la creazione di `autopilot/direct-page-selector` da `main`.
- Il diff verso `11c320b` mostra assenti `src/lib/pdfTextStream.ts`,
  `src/lib/pdfTextStream.test.ts` e il relativo wiring in `src/routes/+page.svelte`.
- Nel branch corrente `extractPageText` contiene ancora la chiamata diretta a
  `page.getTextContent()`; non esistono riferimenti di produzione a
  `collectTextContentItems`.

### Meccanismo runtime

- La build installata di PDF.js 6.1.200 implementa il percorso ordinario di
  `getTextContent()` iterando asincronamente il `ReadableStream` restituito da
  `streamTextContent()`.
- Un harness read-only con un vero fixture PDF e uno stream dotato di `getReader()` ma
  privo di iteratore asincrono riproduce il fallimento sul percorso `getTextContent()`;
  lo stesso contenuto viene letto correttamente con `reader.read()`.
- La documentazione corrente PDF.js usa anch'essa `streamTextContent().getReader()` e un
  ciclo/pump di `reader.read()` per consumare i chunk `TextContent`.
- La build legacy contiene lo stesso `for await`, quindi cambiare solo import non risolve
  il confine incompatibile.

### Perché i quality gate non hanno rilevato la regressione

1. **I test correttivi sono stati esclusi insieme al fix.** I quattro test
   `pdfTextStream.test.ts` vivevano soltanto in `11c320b`; creando il branch dal suo padre,
   la suite ha perso contemporaneamente codice e test che avrebbero descritto il
   requisito.
2. **Vitest usa Node, non WKWebView.** `vitest.config.ts` configura `environment: 'node'`.
   Il runtime Node usato dai test espone sia `ReadableStream.getReader()` sia
   `ReadableStream[Symbol.asyncIterator]`, quindi non riproduce la capability mancante di
   WebKit.
3. **La suite corrente non attraversa il confine PDF.js/Tauri.** I test di
   `pdfExtract` partono da item già estratti; quelli del coordinatore di rendering usano
   promise/task finti. Nessun test apre un PDF tramite `loadDocument` in WKWebView.
4. **Check e build sono statici.** `svelte-check` vede un'API PDF.js tipata e valida;
   Vite compila e impacchetta il codice ma non esegue l'estrazione dentro WebKit.
5. **Le review avevano il fixed point sbagliato.** Il quality loop confrontava il diff
   del selettore con `HEAD/main`. Una review del diff non può segnalare un fix assente che
   esiste soltanto su un altro branch/PR non mergiato, se quel prerequisito non viene
   incluso esplicitamente nel baseline.

Il `package-lock.json` modificato non è causale: la versione di PDF.js resta 6.1.200 e il
diff preesistente non modifica questa dipendenza.

## Decision / Solution

Integrare nel working tree corrente il **solo slice frontend** della correzione `11c320b`,
adattandolo alla nuova forma di `extractPageText(page)`:

1. ripristinare `src/lib/pdfTextStream.ts` e il relativo test;
2. per le pagine ordinarie, consumare `page.streamTextContent()` con
   `getReader()`/`read()`, concatenando gli item in ordine e rilasciando sempre il lock;
3. mantenere `page.getTextContent()` soltanto per il ramo `isPureXfa`, che in PDF.js non
   attraversa lo stream incompatibile;
4. collegare l'adattatore all'attuale `extractPageText(page)` senza sovrascrivere
   selettore, `LatestRenderCoordinator`, guardie di generazione o fix di concorrenza;
5. aggiungere un controllo del wiring di produzione e una prova Tauri/macOS obbligatoria.

Non fare cherry-pick cieco dell'intero commit `11c320b`: contiene altri file e una versione
precedente di `+page.svelte`. Il port deve essere mirato per preservare entrambe le
funzionalità.

Come correzione di processo, un ticket basato su lavoro non ancora mergiato deve dichiarare
il commit/PR prerequisito. Prima di creare il branch, l'autopilot deve controllare che il
baseline contenga quel prerequisito oppure creare esplicitamente uno stack di branch.

## Options Considered

### Opzione 1: port mirato dell'adattatore reader-based — scelta

- Ripristina esattamente la compatibilità già verificata.
- Mantiene la versione PDF.js e limita la modifica al confine difettoso.
- Richiede la risoluzione manuale del piccolo wiring con l'attuale route.

### Opzione 2: cherry-pick completo di `11c320b`

- Recupererebbe codice, test e documentazione originali in una sola operazione.
- Può sovrascrivere o confliggere con la route modificata dal selettore diretto e include
  cambiamenti non necessari al fix PDF.js.

### Opzione 3: polyfill globale dell'iteratore asincrono

- Permetterebbe di continuare a usare `getTextContent()`.
- Amplia il rischio a tutti i `ReadableStream` dell'app e maschera il confine reale.

### Opzione 4: affidarsi soltanto alla QA manuale

- Riproduce fedelmente il runtime macOS.
- Non impedisce regressioni future e non sostituisce un contratto automatico
  reader-without-async-iterator.

## Implementation Plan

1. Portare `pdfTextStream.ts` e i suoi test da `11c320b`, verificando prima isolatamente
   multi-chunk, `done`, errore, rilascio lock e ramo pure XFA.
2. Integrare `collectTextContentItems(page)` nell'attuale funzione di estrazione che usa
   un oggetto pagina catturato. Non reintrodurre letture dal `pdfDoc` globale e non
   modificare il coordinatore dei render.
3. Aggiungere un test di integrazione frontend o un contratto di modulo che attraversi il
   percorso usato da `extractPageText`, con uno stream che ha `getReader()` ma non
   `Symbol.asyncIterator`. Il test deve fallire se il percorso ordinario torna a chiamare
   direttamente `getTextContent()`.
4. Eseguire test frontend completi, `npm run check`, `npm run build` e `git diff --check`.
   Confrontare inoltre il diff risultante con `11c320b` per verificare che il slice di
   compatibilità sia presente senza perdere le modifiche di navigazione.
5. Avviare Tauri su macOS e aprire un fixture PDF testuale e un PDF reale. Verificare
   apertura, canvas, testo, traduzione, salto diretto e navigazione rapida. Aprire anche un
   PDF senza testo per confermare il messaggio EC01.
6. Prima di finalizzare la PR, documentare la dipendenza dalla PR #14 oppure incorporare
   il port nel nuovo branch; non dichiarare il lavoro completo finché il branch finale non
   contiene entrambi i comportamenti.

## Testing Decisions

- Mantenere unit test dell'adattatore per ordine dei chunk, `done` immediato, eccezione,
  `releaseLock()` in `finally` e pure XFA.
- Aggiungere un test del confine runtime con `getReader()` disponibile e iteratore
  asincrono assente: è la capability effettivamente diversa in WKWebView.
- Coprire il wiring di produzione, non solo l'helper isolato; un helper testato ma non
  importato dalla route non protegge il comportamento utente.
- Conservare i test di `pdfExtract`, selettore pagina e coordinatore render: validano
  responsabilità adiacenti ma non sostituiscono il test dello stream.
- Considerare `npm test`, check e build necessari ma non sufficienti. La QA Tauri su
  macOS è un gate esplicito, perché il difetto dipende dal motore incorporato.
- Non asserire dettagli interni non pubblici di PDF.js; modellare il contratto Web Stream
  osservabile (`getReader`, `read`, `done`, `releaseLock`).

## Follow-Up Tickets

- Port mirato dell'adattatore WKWebView e integrazione con la route del selettore diretto.
- Copertura del wiring/capability boundary e QA Tauri macOS del branch combinato.
- Guardrail dell'autopilot per verificare commit/PR prerequisiti prima di creare un branch
  da `main`.

I ticket sono stati creati sotto
`docs/tickets/pdfjs-wkwebview-text-extraction-diagnosis/`: 01 completato, 02 e 03 HITL,
04 bloccato dalla decisione del ticket 03.

## Open Questions

- Nessuna domanda tecnica bloccante. La scelta operativa è risolta: il fix `11c320b` è
  entrato in `main` e PR #15 mantiene il suo superset integrato con il selettore diretto.
  Restano la QA nativa del ticket 02 e la decisione di processo del ticket 03.

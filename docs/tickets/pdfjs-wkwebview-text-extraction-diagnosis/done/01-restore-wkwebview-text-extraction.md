# 01 — Ripristinare l'estrazione testo compatibile con WKWebView

## Parent Spec

[pdfjs-wkwebview-text-extraction-diagnosis.md](../../specs/pdfjs-wkwebview-text-extraction-diagnosis.md)

## Type

AFK

## What to Build

Ripristinare nel branch del selettore diretto il percorso di estrazione testo compatibile
con macOS WKWebView, portando in modo mirato l'adattatore reader-based già presente nel
commit `11c320b`. Il percorso produttivo deve consumare i chunk PDF.js tramite
`streamTextContent().getReader()` senza perdere il selettore, le guardie di generazione
documento o la cancellazione dei render PDF.js presenti nel working tree corrente.

Questa slice copre **Goals**, **Decision / Solution**, i passi 1–4 di **Implementation
Plan** e la parte automatizzabile di **Testing Decisions** della parent spec.

## Acceptance Criteria

- [x] Le pagine PDF ordinarie vengono lette tramite `streamTextContent()` e il reader
      esplicito; il percorso non richiede `ReadableStream[Symbol.asyncIterator]`.
- [x] Le pagine pure XFA continuano a usare il ramo `getTextContent()` previsto da PDF.js.
- [x] Gli item di più chunk mantengono l'ordine e il reader rilascia sempre il lock, anche
      quando `read()` fallisce o termina immediatamente.
- [x] Il percorso di produzione usato da `extractPageText` invoca l'adattatore; un test
      fallisce se le pagine ordinarie tornano alla chiamata diretta a `getTextContent()`.
- [x] Un test riproduce il capability boundary di WKWebView: `getReader()` disponibile,
      iteratore asincrono assente, estrazione comunque riuscita.
- [x] Restano intatti selettore diretto, `LatestRenderCoordinator`, guardie di generazione,
      prefetch e allineamento pagina/testo/sessione.
- [x] `npm test`, `npm run check`, `npm run build` e `git diff --check` sono verdi; il diff
      è confrontato con `11c320b` per confermare il port del solo slice necessario.

## Blocked By

- None - can start immediately.

## Frontier

Ready now. Root cause, fix precedente e contratto di compatibilità sono già confermati;
non serve una decisione umana per iniziare. Non fare cherry-pick cieco dell'intero commit
`11c320b`, perché la route corrente contiene lavoro successivo da preservare.

## Step-by-Step Implementation Plan

1. Confrontare la route e i moduli frontend correnti con il slice PDF di `11c320b`.
   Identificare soltanto adattatore, test e punto di wiring, senza importare modifiche
   adiacenti del vecchio branch.
2. Aggiungere prima i test del contratto reader: più chunk, `done` immediato, errore,
   rilascio lock, pure XFA e stream senza iteratore asincrono. Verificare che almeno il
   caso WKWebView fallisca prima del port.
3. Portare l'adattatore in un modulo TypeScript isolato e tipizzato sul contratto minimo
   PDF.js. Evitare polyfill globali e dipendenze dal componente Svelte.
4. Collegare l'adattatore all'attuale estrazione basata sull'oggetto pagina catturato.
   Conservare mapping degli item, viewport, ricostruzione EC01 e tutte le guardie di
   identità/generazione già presenti.
5. Aggiungere una verifica del wiring produttivo, così l'helper non può restare testato ma
   inutilizzato. Eseguire suite, check, build e diff-check.
6. Ispezionare il diff finale rispetto sia a `HEAD` sia a `11c320b`: deve contenere la
   compatibilità WKWebView senza rimuovere il selettore diretto o i fix di concorrenza.

## Testing Plan

- Unit test del reader per aggregazione ordinata, fine stream, errore e `releaseLock()`.
- Test pure XFA che verifica il solo uso consentito di `getTextContent()`.
- Test capability/wiring con reader valido e `Symbol.asyncIterator` assente.
- Suite frontend completa, Svelte/TypeScript check, build Vite e `git diff --check`.
- La prova nella GUI Tauri non appartiene a questa slice: è il ticket 02.

## Out of Scope

- QA manuale macOS, modifiche Rust, OCR, upgrade/downgrade di PDF.js, polyfill globali,
  guardrail di processo e cherry-pick completo di `11c320b`.

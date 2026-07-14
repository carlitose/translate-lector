# 02 — Fedeltà del layout da Tesseract (hOCR/TSV) per la ricostruzione

## Parent Spec

[ocr-layout-translation-wayfinder.md](../../specs/ocr-layout-translation-wayfinder.md)

## Type

prototype

## Outcome

Un **verdetto** sulla **tensione centrale** della mappa: l'output strutturato di Tesseract
(hOCR / TSV: gerarchia blocco→paragrafo→riga→parola, bounding box, confidenza, dimensione font stimata)
è **sufficiente** a guidare una ricostruzione tipografica credibile, **oppure serve uno step di layout
separato**? Il verdetto ridefinisce realisticamente lo scope della ricostruzione.

## Acceptance Criteria

- [ ] Prototipo in `prototypes/ocr/` che estrae da Tesseract l'output strutturato (hOCR o TSV) per almeno
      3 pagine scansionate rappresentative: una a colonna singola, una a due colonne, una con figura/immagine.
- [ ] Estrazione di: box dei blocchi/paragrafi/righe/parole, ordine di lettura, stima dimensione font,
      confidenza. Valutazione se le regioni non-testo (figure) sono identificabili.
- [ ] Verifica se la logica di colonne/riordino già presente in `src/lib/pdfExtract.ts`
      (`detectColumnSplit`, raggruppamento per riga, de-sillabazione) è **riutilizzabile** sui box OCR.
- [ ] **Verdetto scritto** nel parent spec: (a) Tesseract basta + euristiche → procedi; oppure
      (b) serve layout separato → elenca opzioni leggere e l'impatto sulla decisione "motore leggero".
- [ ] Casi limite catturati: testo ruotato/skew, qualità scan bassa, tabelle, testo su sfondo.

## Blocked By

- Ticket 01 (serve il percorso immagine→OCR scelto e funzionante).

## Frontier

Risolve l'incognita più grande della destinazione. Finché non sappiamo quanta struttura ci dà Tesseract,
non possiamo sapere se la "ricostruzione tipografica" è fattibile con il motore leggero scelto, né
progettare il rendering (03) o il contratto di traduzione strutturata (05).

## Work Plan

1. Dallo spike del Ticket 01, abilitare output hOCR/TSV di Tesseract.
2. Parsare la gerarchia e visualizzare i box (overlay di debug sull'immagine) per ispezione.
3. Confrontare l'ordine di lettura OCR con quello atteso; provare a riusare `detectColumnSplit`.
4. Giudicare la fedeltà su singola colonna, due colonne e pagina con figura; annotare dove crolla.
5. Scrivere il verdetto e le sue conseguenze sullo scope nel parent spec.

## Evidence to Capture

- Immagini di debug con box sovrapposti, per le 3+ pagine campione.
- Frammenti hOCR/TSV di esempio.
- Tabella pro/contro: cosa Tesseract dà bene vs dove serve euristica/step extra.
- Verdetto esplicito (a) o (b) con motivazione.

## Out of Scope

- Rendering/re-typeset della pagina tradotta (Ticket 03).
- Chiamata di traduzione (Ticket 05).
- Ottimizzazione performance.

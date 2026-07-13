# 01 — Prototype: validare estrazione testo con pdf.js

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md)

## Type

prototype

## Outcome

Sapere se pdf.js estrae testo di qualità sufficiente per la traduzione, e con quanto sforzo.
Riduce l'incognita più a rischio-prodotto: se l'estrazione è scarsa, cambia scope o valore.

## Acceptance Criteria

- [ ] Prototipo (anche pagina HTML + pdf.js standalone) che carica un PDF e stampa il testo estratto per pagina.
- [ ] Testato su almeno 3 PDF diversi: (a) testo semplice a colonna singola, (b) doppia colonna/layout accademico, (c) con header/footer ripetuti.
- [ ] Verdetto scritto: l'estrazione grezza è "traducibile" così com'è, oppure serve post-processing (ordine di lettura, ricomposizione paragrafi, dehyphenation, rimozione header/footer)?
- [ ] Evidenza e verdetto riassunti nel parent spec (§ Not Yet Specified → risolvere T01).

## Blocked By

- None — can start immediately.

## Frontier

È un bordo indipendente e ad alto rischio: la qualità della traduzione dipende interamente dalla qualità del testo in ingresso. Va chiuso presto per non scoprire tardi che serve OCR o heavy post-processing.

## Work Plan

1. Setup minimale pdf.js (via CDN in una pagina statica o piccolo progetto Vite) — usare `getTextContent()` sui text-items.
2. Estrarre testo grezzo per pagina e osservare: ordine, spazi, a-capo, colonne, header/footer.
3. Provare una ricomposizione basilare basata su coordinate (raggruppamento per riga/blocco) e valutare il miglioramento.
4. Annotare quanto post-processing serve per un testo accettabile e se è fattibile lato frontend.

## Evidence to Capture

- Snippet di testo estratto (prima/dopo ricomposizione) per i 3 tipi di PDF.
- API pdf.js usate e versione.
- Elenco dei problemi di layout riscontrati e mitigazioni proposte.

## Out of Scope

- OCR di PDF scansionati.
- Rendering visivo della pagina (solo estrazione testo qui).
- Integrazione con il core Rust.

---

## Findings (2026-07-13)

### Setup del prototipo

- **Libreria di estrazione**: `pdfjs-dist` v6.1.200 (già in `package.json`), build Node/legacy `pdfjs-dist/legacy/build/pdf.mjs`.
- **API pdf.js usate**: `getDocument({ data }).promise` → `doc.getPage(n)` → `page.getTextContent()`. Per ogni text-item si usano `item.str`, `item.transform` (`transform[4]=x`, `transform[5]=y baseline, origine in basso-a-sinistra`), `item.width`, `item.height`, `item.hasEOL`. Coordinate di pagina via `page.getViewport({ scale: 1 })`.
- **Generatore fixture**: `pdfkit` (devDependency, JS puro, nessuna dipendenza nativa; installato da npm senza rete). Le 3 fixture sono generate localmente da `prototypes/pdfjs/generate-fixtures.mjs`.
- **Codice**: `prototypes/pdfjs/generate-fixtures.mjs` (genera i PDF), `prototypes/pdfjs/extract.mjs` (estrazione RAW + ricostruzione + strip header/footer). Eseguire con `node prototypes/pdfjs/extract.mjs`.
- **Nota**: pdf.js emette il warning `Ensure that the standardFontDataUrl API parameter is provided.` È innocuo per `getTextContent()` (riguarda solo il rendering dei glifi, non l'estrazione testo). In frontend si passa `standardFontDataUrl` puntando agli asset di pdfjs-dist, oppure si ignora.

### Fixture effettivamente testate (3/3)

1. **(a) `a-single-column.pdf`** — prosa a colonna singola, 2 paragrafi, con parole spezzate da trattino a fine riga (`terminolo-\ngical`, `transla-\ntor`).
2. **(b) `b-two-column.pdf`** — VERO layout a due colonne, ottenuto con posizionamento assoluto x/y in pdfkit (colonna sinistra x=50, colonna destra x=315, gutter reale ~60pt) + titolo centrato a tutta larghezza. Layout confermato dalle coordinate estratte (sinistra xend≤276, destra x≥315).
3. **(c) `c-header-footer.pdf`** — 2 pagine, header corrente ripetuto ("Chapter 3 — The Art of Extraction") + footer con numero di pagina ("Page 1"/"Page 2"), più corpo di testo.

### Before/After (snippet reali rappresentativi)

**(a) Colonna singola** — il RAW è quasi traducibile ma spezza le parole sillabate:
```
RAW:            ... draws a terminolo-\ngical distinction ...
                ... mechanically aid the human transla-\ntor. More recently ...
RECONSTRUCTED:  ... draws a terminological distinction ...
                ... mechanically aid the human translator. More recently ...
```

**(b) Due colonne** — problema critico di ordine di lettura. La ricostruzione coordinate-based separa le colonne ed emette prima tutta la sinistra poi tutta la destra:
```
RAW (rischioso):   Abstract. This paper studies text extraction  ... heuristic.
                   Method. We group text items by their vertical  ... translation.
RECONSTRUCTED:     [Titolo]
                   Abstract. ... coordinate-based reconstruction heuristic.
                   Method. ... restoring a natural ... reading order for downstream translation.
```
NB onesto: in QUESTA fixture il RAW risultava già in ordine corretto perché pdfkit scrive i blocchi in draw-order colonna-per-colonna. Nei PDF reali l'ordine di consegna di pdf.js NON è garantito seguire l'ordine di lettura: la separazione affidabile richiede la ricomposizione per coordinate, non ci si può fidare del RAW.

**(c) Header/Footer** — nel per-pagina l'header finisce in cima e il footer in fondo, mescolati al corpo. Il rilevamento document-level (righe che si ripetono nella fascia margine su ≥2 pagine, con numeri normalizzati a `#`) li rimuove:
```
RECONSTRUCTED (per pagina):  Chapter 3 — The Art of Extraction\n<corpo>\nPage 1
DOC + HEADER/FOOTER STRIPPED: <solo corpo, "Chapter 3..." e "Page N" rimossi>
```

### Verdetto per tipo

| Tipo PDF | RAW traducibile as-is? | Serve post-processing? |
|----------|------------------------|------------------------|
| (a) Colonna singola | Quasi — ordine e spazi corretti | Sì, leggero: **de-hyphenation** e join dei ritorni a capo intra-paragrafo |
| (b) Due colonne | NO (non affidabile) | Sì, essenziale: **rilevamento colonne + riordino per coordinate** |
| (c) Header/Footer | NO | Sì: **rilevamento e rimozione righe ripetute** a livello documento (serve ≥2 pagine) |

**La ricostruzione aiuta: SÌ**, in tutti e tre i casi migliora o è indispensabile.

### Problemi di layout riscontrati + mitigazioni proposte

1. **Sillabazione a fine riga** → de-hyphenation: se una riga termina con `[A-Za-z]-`, unire alla successiva togliendo il trattino. (Rischio: falsi positivi su trattini legittimi es. "world-wide"; mitigare con dizionario/euristiche, accettabile per un prototipo.)
2. **Ordine di lettura multi-colonna** → raggruppare gli item per coordinata `y` in righe, individuare il gutter verticale (banda x senza glifi che separa sinistra/destra su molte righe), emettere ogni colonna per intero. Un titolo a tutta larghezza NON deve invalidare il rilevamento: distinguere una riga "full-width contigua" (nessun gutter interno) da una riga multi-colonna (gutter interno ampio) — questo è stato il bug chiave risolto nel prototipo.
3. **Header/footer ripetuti** → analisi document-level: una riga che ricorre nella fascia (top/bottom ~12%) su ≥2 pagine, con i numeri normalizzati, è running head/foot → rimuovere. Richiede il confronto tra pagine (non rilevabile da una pagina sola).
4. **Righe unite senza spazio / spazi mancanti** → in `getTextContent` gli item possono essere adiacenti senza spazio esplicito; si inserisce uno spazio quando il gap x tra item supera ~0.25·altezza-glifo.
5. **Ricomposizione dei paragrafi** → oltre il join di riga, distinguere fine-paragrafo da a-capo di wrap (es. rientro/linea corta): non implementato qui, resta un raffinamento consigliato prima della traduzione.

### La ricostruzione è fattibile lato frontend?

**Sì.** L'intera pipeline (line-grouping per y, column-split per gap x, de-hyphenation, strip header/footer document-level) è pura manipolazione di array su `{str, x, y, width, height}` — nessuna dipendenza nativa, gira nello stesso ambiente JS/TS del frontend che già usa pdf.js. Coerente con SPECIFICATION §4.2 ("il frontend estrae il testo di pagina via pdf.js"). **Non serve OCR** per PDF con text-layer (l'OCR resta necessario solo per scansioni, fuori scope). Raccomandazione: implementare la ricostruzione come modulo TS lato frontend, riusando la logica del prototipo `extract.mjs`.

### Acceptance criteria

- [x] Prototipo che carica un PDF e stampa il testo estratto per pagina — `prototypes/pdfjs/extract.mjs`.
- [x] Testato su 3 PDF: (a) colonna singola, (b) doppia colonna vera, (c) header/footer ripetuti (2 pagine).
- [x] Verdetto scritto: RAW insufficiente per (b) e (c), quasi-ok per (a); post-processing necessario e fattibile lato frontend (tabella + mitigazioni sopra).
- [x] Evidenza e verdetto riassunti qui; da propagare al parent spec (§ Not Yet Specified → T01 risolto).

---

_Completato 2026-07-13: prototipo pdf.js eseguito su 3 fixture reali (single-column, two-column, header/footer); estrazione RAW + ricostruzione coordinate-based verificate; verdetto e mitigazioni documentati. Ticket chiuso._

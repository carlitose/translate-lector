# OCR + Traduzione con conservazione del layout — Wayfinding Spec

## Type

Wayfinding spec

## Status

Active

## Destination

Estendere translate-lector (MVP completo) per supportare **PDF scansionati / testo stampato**
tramite **OCR locale**, con l'obiettivo finale di una **ricostruzione tipografica**: la pagina
tradotta viene **re-impaginata** conservando struttura, colonne, immagini e posizione dei blocchi
di testo — una "copia perfetta" della pagina originale ma con il testo nella lingua di destinazione.

Oggi i PDF scansionati sono un vicolo cieco: `EC01_MESSAGE = 'formato non supportato (no OCR)'`
(`src/routes/+page.svelte:105`). La destinazione sostituisce quel dead-end con una pipeline
OCR → modello di layout → traduzione strutturata → re-typeset della pagina.

Contesto di prodotto: [SPECIFICATION.md](../../SPECIFICATION.md) §6 (roadmap: "OCR per PDF scansionati").

## Decisions So Far

- **Ambizione output = Ricostruzione tipografica** (deciso dall'utente, 2026-07-14). Non un semplice
  pannello testo affiancato, né solo overlay: si ri-genera una pagina impaginata con il testo tradotto
  (font, colonne, immagini). L'overlay-su-immagine è comunque un **possibile stadio intermedio** verso
  questo traguardo (vedi Frontier).
- **Motore OCR = leggero, Tesseract / pure-Rust preferito** (deciso dall'utente, 2026-07-14). Nessun
  runtime Python pesante nell'MVP OCR; CPU; bundle contenuto; copertura lingue via `traineddata`.
- **Piattaforma e stack invariati**: Tauri v2 + Svelte 5 + Rust core + SQLite, tutto locale (eredità MVP,
  vedi [translate-lector-wayfinder.md](./translate-lector-wayfinder.md)).
- **Traduzione LLM invariata come gateway**: OpenRouter resta il motore di traduzione; l'OCR produce il
  testo/struttura, la traduzione continua a passare per il percettore (summary + glossario).

## Tensione centrale da risolvere

L'utente vuole **massima fedeltà** (ricostruzione tipografica) con un **motore leggero** (Tesseract).
Tesseract fornisce box di parola/riga/paragrafo/blocco e stime di dimensione, **non** un'analisi di
layout ricca (attributi di font, regioni-figura, ordine di lettura multi-colonna robusto). Il primo
lavoro della mappa è **provare se questo divario è colmabile sopra Tesseract** (con hOCR/TSV + euristiche,
riusando la logica di colonne già scritta in `src/lib/pdfExtract.ts`) **o se serve uno step di layout
separato**. Questo verdetto decide quanto è realistica la ricostruzione tipografica nell'MVP OCR.

## Not Yet Specified

- **Integrazione Tesseract su Windows/Tauri**: `leptess` (link a Leptonica/Tesseract C — build ostica su
  Windows) vs `rusty-tesseract`/sidecar che invoca `tesseract.exe` vs binario Tesseract bundled. Come
  passare l'immagine di pagina da pdf.js (canvas) al core Rust. Licenze, dimensione bundle, gestione dei
  `traineddata`. → Ticket 01.
- **Fedeltà del layout da Tesseract**: hOCR/TSV bastano (blocchi/paragrafi/righe/parole + font size stimato)
  per guidare la ricostruzione? Serve un rilevatore di layout separato? → Ticket 02.
- **Rendering della ricostruzione**: re-typeset in HTML/CSS assoluto sopra le regioni-immagine conservate;
  sostituzione/fallback dei font; gestione dell'**overflow** quando la traduzione è più lunga
  (shrink-to-fit vs crescita box); conservazione delle figure (regioni raster non-testo). → Ticket 03.
- **Decisioni umane di prodotto**: la vista facsimile è la vista primaria o una modalità aggiuntiva accanto
  all'attuale affiancata? Serve export in PDF tradotto? Qual è il "pavimento di fedeltà" accettabile per la
  v1? Quali lingue OCR al lancio? Tolleranza di latenza/caching OCR? Routing per documenti misti
  (pagine con testo estraibile + pagine scansionate). → Ticket 04.
- **Contratto di traduzione strutturata + percettore + schema dati**: come tradurre per-blocco preservando
  la struttura mantenendo coerenza di summary/glossario; estensione del modello dati SQLite per layout,
  cache OCR e (eventuali) immagini di pagina. → Ticket 05.

## Out of Scope

- **Riscrittura del motore di traduzione**: OpenRouter + percettore restano; qui si aggiunge un ramo OCR
  a monte e una vista/ricostruzione a valle.
- **OCR di manoscritti / grafia** (Tesseract non è adatto; fuori portata).
- **Traduzione di testo dentro immagini/diagrammi incorporati** oltre i blocchi di testo principali (v2).
- **Multi-provider OCR** (VLM locale, PaddleOCR): scartati per la scelta "leggero"; riconsiderabili solo se
  il Ticket 02 dimostra che Tesseract è insufficiente per una fedeltà accettabile.
- **Export/packaging distribuibile**: come per l'MVP, uso personale (`tauri dev`/build locale).

## Frontier / Blocking Edges

La frontiera è **la fase di indagine**: il traguardo "ricostruzione tipografica" ha troppe incognite di
fattibilità per derivare subito slice di build. Ordine di attraversamento:

1. **Ticket 01 (research) — Integrazione Tesseract locale** *(ready, primo edge)*: sblocca tutto il resto;
   senza un modo affidabile di far girare l'OCR nel core Rust su Windows e di passargli l'immagine di
   pagina, non c'è pipeline.
2. **Ticket 02 (prototype) — Fedeltà layout da Tesseract** *(dipende da 01)*: risolve la **tensione
   centrale**. Verdetto: hOCR/TSV bastano per la ricostruzione o serve layout separato? Definisce
   realisticamente lo scope della ricostruzione.
3. **Ticket 03 (prototype) — Rendering della ricostruzione** *(dipende da 02)*: prova disposable che, dati
   box + testo tradotto, re-impagina una pagina credibile (overflow, font, figure).
4. **Ticket 04 (grilling) — Decisioni umane** *(può partire in parallelo a 02/03; gate prima del design)*:
   vista primaria vs aggiuntiva, export, pavimento di fedeltà v1, lingue, performance, documenti misti.
5. **Ticket 05 (research/design) — Contratto traduzione strutturata + percettore + schema** *(dipende da
   02 e 04)*: chiude il design prima delle build verticali.

Dopo che 01-05 sono chiusi e il gate umano (04) è deciso: **rivedere questa mappa** e derivare i ticket di
build verticali (tracer-bullet OCR: rileva pagina scansionata → OCR+layout → traduci strutturato → re-typeset
→ cache/persisti) con `to-tickets`.

## Ticket Plan

Cartella: `docs/tickets/ocr-layout-translation/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Integrazione Tesseract locale in Tauri/Rust su Windows | ready |
| 02 | prototype | Fedeltà del layout da Tesseract (hOCR/TSV) per la ricostruzione | blocked by 01 |
| 03 | prototype | Rendering della ricostruzione tipografica (re-typeset pagina) | blocked by 02 |
| 04 | grilling | Decisioni di prodotto OCR (vista, export, fedeltà, lingue) | ready (gate) |
| 05 | research | Contratto traduzione strutturata + percettore + schema dati | blocked by 02, 04 |

## Next Review

Quando 01-05 sono chiusi e 04 è deciso:
1. Ripiegare evidenze e verdetti in questa mappa (aggiornare "Decisions So Far", svuotare "Not Yet Specified").
2. Aggiornare SPECIFICATION.md: nuova sezione OCR (rilevamento, pipeline, ricostruzione), estensione §4.3
   (schema dati) e §4.4 (contratto se cambia), spostare l'OCR da §6 roadmap a scope attivo, rivedere EC01.
3. Derivare i ticket di build verticali con `to-tickets`.

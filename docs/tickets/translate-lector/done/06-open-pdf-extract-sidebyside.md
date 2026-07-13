# 06 — Apri PDF: estrazione testo, vista affiancata e navigazione

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.1, §4.2, FR01/FR02/FR04, UC01, EC01

## What to Build

Slice end-to-end **senza traduzione**: l'utente clicca "Apri PDF", sceglie un file, e vede la **vista affiancata** — a sinistra la pagina PDF renderizzata (pdf.js), a destra il **testo estratto e ricostruito** della stessa pagina (placeholder della futura traduzione). Barra inferiore con navigazione a **pagina discreta** (◀ Pag. N / Tot ▶, decisione D1). All'apertura il core **registra il documento** (hash parziale + dimensione, D2) e **crea o carica la sessione** (lingua di destinazione di default = Italiano, pagina corrente).

La ricostruzione testo porta nel frontend reale la logica del prototipo `prototypes/pdfjs/` (raggruppa item per riga via coordinate, rileva colonne per x-gap, unisce sillabazioni di fine riga). Se il PDF non ha testo estraibile → messaggio "non supportato" (EC01, niente OCR).

## Acceptance Criteria

- [ ] "Apri PDF" apre un file picker; scelto un PDF con testo, la pagina 1 è renderizzata a sinistra.
- [ ] A destra compare il testo ricostruito della pagina corrente (ordine di lettura corretto su singola e doppia colonna; sillabazioni unite).
- [ ] Navigazione ◀ ▶ cambia pagina discreta; l'indicatore mostra "Pag. N / Tot".
- [ ] All'apertura il core registra/aggiorna una riga in `documents` (file_path, file_hash parziale, title, total_pages, last_opened_at) e crea/carica la `sessions` collegata (target_language, current_page).
- [ ] Riaprendo lo stesso file (anche rinominato/spostato) l'hash lo riconosce e riusa la stessa riga `documents`.
- [ ] PDF senza testo estraibile → messaggio "formato non supportato (no OCR)"; nessun crash.

## Blocked By

- None - can start immediately (lo scaffold, il DB §4.3 e pdfjs-dist esistono già).

## Frontier

**Ready now.** È il primo tracer-bullet: prova PDF→estrazione→UI→persistenza-base senza dipendere dall'LLM.

## Step-by-Step Implementation Plan

1. **Frontend — layout affiancato** (`src/`): componente pagina con due riquadri (sinistra render PDF, destra testo) + barra superiore (Apri PDF, selettore lingua da lista D4) + barra inferiore (navigazione). Usa runes Svelte 5 per lo stato (documento, pagina corrente, testo). *Verifica*: `npm run build` e `npm run check` puliti.
2. **Apertura file**: usa il dialog file di Tauri (plugin dialog) per ottenere il path; leggi i byte via comando Rust o API fs di Tauri. *Perché ora*: serve il path reale per hash e persistenza. *Pitfall*: nella webview la API key/percorsi passano dal core, ma il rendering PDF sta nel frontend.
3. **Render + estrazione pdf.js** (frontend): carica il documento con `pdfjs-dist`, renderizza la pagina su `<canvas>`, e per il testo usa `getTextContent()`. Porta il modulo di ricostruzione dal prototipo (`prototypes/pdfjs/extract.mjs`) in un modulo TS del frontend. *Verifica*: apri i fixture del prototipo e confronta l'output col prototipo.
4. **Comando Rust `register_document`** (`src-tauri`): riceve path (+ eventualmente metadati), calcola **hash parziale** (SHA-256 dei primi+ultimi N KB + dimensione file — aggiungi crate `sha2`), fa upsert in `documents` per (file_hash), ritorna `document_id` e `total_pages`. *Affects*: nuova funzione in un modulo `documents.rs`; test unit su hashing stabile e upsert idempotente.
5. **Comando Rust `open_or_create_session`**: dato `document_id`, crea/carica `sessions` (default target_language="it", current_page=1) e ritorna lo stato. *Verifica*: unit test crea→ricarica ritorna la stessa riga.
6. **Wire frontend↔core**: all'apertura, il frontend chiama `register_document` → `open_or_create_session`, poi renderizza la `current_page`. Navigazione aggiorna la pagina mostrata (la persistenza di current_page è ok aggiornarla qui o rimandarla al ticket 11 — minimale: aggiorna in memoria + salva `current_page`). *Pitfall*: non introdurre `scroll_position` (D1: inutilizzato).
7. **EC01**: se l'estrazione totale della pagina è vuota su tutte le pagine campionate → mostra messaggio non supportato.

## Testing Plan

- **Rust unit**: hashing parziale deterministico e stabile a rinomina; upsert `documents` idempotente per hash; `open_or_create_session` crea e ricarica. Mantieni verdi i 4 test esistenti (`cargo test`).
- **Frontend**: test del modulo di ricostruzione testo sui 3 fixture del prototipo (single/two-col/header-footer) — output atteso allineato al prototipo. `npm run check` pulito.
- **Manuale**: aprire un PDF reale, verificare render + testo + navigazione + messaggio EC01 su un PDF immagine.

## Out of Scope

- Traduzione (ticket 08) e percettore (09).
- Ripristino automatico all'avvio e cronologia recenti (ticket 11) — qui basta creare/registrare, non ripristinare al boot.
- Rimozione header/footer document-level (raffinamento post-MVP).


---

**Completato 2026-07-13.** Slice end-to-end senza traduzione. Rust: modulo `documents.rs` (partial hash SHA-256 head+tail 64KB + size, D2; `register_document` upsert idempotente per hash; `open_or_create_session` default it/pag.1, D1; `update_session`) + comandi Tauri `read_pdf_bytes`/`register_document`/`open_or_create_session`/`update_session`, plugin `tauri-plugin-dialog`, permesso `dialog:default`. Frontend: `src/lib/pdfExtract.ts` (port ricostruzione dal prototipo) + vista affiancata in `+page.svelte` (canvas pdf.js sx, testo ricostruito dx, selettore lingua D4 15+libero, navigazione pagina discreta, EC01 "formato non supportato (no OCR)"). Test: 9 cargo (4+5), 3 vitest, `npm run check` 0 errori, `npm run build` ok. QA manuale GUI (apertura PDF reale, render, navigazione, EC01 su PDF immagine) resta human-only.

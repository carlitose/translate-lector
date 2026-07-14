# 01 — Integrazione Tesseract locale in Tauri/Rust su Windows

## Parent Spec

[ocr-layout-translation-wayfinder.md](../../specs/ocr-layout-translation-wayfinder.md)

## Type

research

## Outcome

Un approccio di integrazione **scelto e giustificato** per far girare l'OCR Tesseract dentro il core Rust
di Tauri su Windows 11, incluso **come l'immagine di pagina arriva dal frontend (pdf.js canvas) al core**,
la gestione dei `traineddata`, l'impatto su bundle/licenze, e uno spike minimo che dimostra
`immagine → testo` end-to-end.

## Acceptance Criteria

- [ ] Confronto documentato di almeno: `leptess` (binding Leptonica/Tesseract), `rusty-tesseract`
      (wrapper su `tesseract.exe`), e **binario Tesseract bundled come sidecar Tauri**. Per ognuno:
      fattibilità build su Windows MSVC, dipendenze native, dimensione, manutenzione, licenza.
- [ ] Decisione motivata su **quale approccio** adottare, con i rischi principali.
- [ ] Decisione su **come passare l'immagine** dal frontend al core: opzioni valutate
      (canvas → PNG/base64 via comando Tauri; render della pagina lato Rust; percorso file temporaneo).
- [ ] Strategia `traineddata`: quali lingue di default, dove risiedono (bundle vs download vs cartella dati
      `%APPDATA%/translate-lector`), dimensione stimata.
- [ ] Spike funzionante commitato in `prototypes/ocr/` che fa OCR su almeno un'immagine di pagina
      scansionata reale e stampa il testo (anche fuori dall'app va bene, purché usi l'approccio scelto).
- [ ] Nota di licenza (Tesseract = Apache-2.0; Leptonica = BSD-like) confermata compatibile con l'uso.

## Blocked By

- None — can start immediately.

## Frontier

È l'edge che sblocca tutto: senza un percorso affidabile immagine→OCR nel core, nessun ticket a valle
(02 fedeltà layout, 03 rendering, 05 contratto) può partire.

## Work Plan

1. `find-docs` / ricerca su `leptess`, `rusty-tesseract`, Tauri sidecar (`externalBin`), e requisiti build
   Windows MSVC per Leptonica/Tesseract.
2. Provare l'approccio che sembra meno fragile su Windows (probabile candidato: sidecar `tesseract.exe`)
   con uno spike minimo in `prototypes/ocr/`.
3. Verificare il round-trip immagine: renderizzare una pagina pdf.js su canvas (già fatto nell'app a
   `RENDER_SCALE` in `src/routes/+page.svelte`), esportarla e passarla all'OCR; misurare a che DPI/scala
   l'OCR diventa affidabile.
4. Documentare confronto, decisione, rischi ed evidenze nel parent spec ("Decisions So Far" / "Not Yet
   Specified" del blocco integrazione).

## Evidence to Capture

- Comandi/versioni usate, output OCR dello spike, tempi indicativi per pagina.
- Dimensione dei `traineddata` per lingua e del binario/sidecar.
- Screenshot o dump del testo OCR su una pagina scansionata reale.
- Link ai file dello spike in `prototypes/ocr/`.

## Out of Scope

- Analisi/fedeltà del layout (è il Ticket 02).
- Qualsiasi integrazione UI nell'app vera e propria.
- Traduzione del testo OCR.

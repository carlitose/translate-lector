> ✅ Completato il 2026-07-13 — chiude il ciclo di sessione sopra 06/08/09 (nessuna riscrittura). **Rust** (`documents.rs`): `get_last_session` (sessione con `updated_at` più recente, JOIN documents → path/hash/titolo/pagine per un solo round-trip), `list_recent_documents(limit)` (ordina per `last_opened_at DESC, id DESC`, esclude righe con `last_opened_at IS NULL`, rispetta il limite), `file_exists` (check filesystem puro, EC06, ritorna `false` senza panic su path mancante), `relocate_document` (ri-abbina un file spostato per **partial hash** D2 — aggiorna `file_path` solo su match, `Ok(None)` su file diverso o id sconosciuto, non cancella mai la riga), `remove_recent` (azzera `last_opened_at` → esce dai "Recenti" **preservando** documents/cache/glossario/sessione). 5 comandi Tauri registrati in `lib.rs`. La persistenza di `current_page` + bump di `updated_at` a ogni navigazione era già in 06 (`update_session` in `goTo`/`setLanguage`); qui aggiunto un test del bump. **Frontend**: nuovo `src/lib/session.ts` (puro, testato) con `restoreDecision(none|restore|missing)`, `fileName`, `clampPage`; `+page.svelte` — `openPdf` rifattorizzato in `loadDocument(path)` condiviso da picker/recenti/restore, `onMount` fa restore FR10 (riapre ultimo doc alla pagina+lingua salvate; summary/glossario vivono nel DB e li usa il core / il pannello on-demand) o mostra lo stato EC06, lista "Recenti" (FR09) nell'empty-state con riapertura in un clic, stato file mancante con "Individua file…" (dialog → `relocate_document`) e "Rimuovi dai recenti". 67 test Rust (61 + 6 nuovi) + 28 vitest (21 + 7 nuovi) verdi; `npm run check` 0 errori; `cargo build` e `npm run build` ok. **QA live PENDENTE (solo umano)**: ripristino reale al lancio della GUI, click sui recenti, e spostare/cancellare fisicamente un file per lo stato EC06 (incluso "Individua file") richiedono `tauri dev` e non sono automatizzabili — verificato finora solo via unit test + build.

# 11 — Persistenza sessione, ripristino all'avvio, PDF recenti

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) FR08/FR09/FR10, UC04, EC06, QR02

## What to Build

Chiude il ciclo di sessione: alla chiusura lo stato è già persistito (documento, **pagina corrente** (D1), lingua, rolling_summary, glossario, cache — tutto salvato incrementalmente da 06/08/09); questo ticket aggiunge il **ripristino automatico all'avvio** (riapre l'ultimo documento alla sua pagina, FR10), la **cronologia PDF recenti** con riapertura in un clic (FR09), e la **gestione del file mancante/spostato** via hash (EC06).

## Acceptance Criteria

- [ ] All'avvio, l'app ricarica l'ultima sessione e apre il documento alla `current_page` salvata, con la lingua salvata; summary e glossario ricaricati.
- [ ] Esiste una lista "Recenti" (da `documents.last_opened_at`) che riapre un PDF in un clic.
- [ ] Se il file dell'ultima sessione/recente è stato spostato o cancellato: l'hash lo riconosce se ancora reperibile; se irreperibile, messaggio chiaro con opzione "individua file" o rimuovi dai recenti (EC06), senza crash.
- [ ] Navigare tra le pagine aggiorna e persiste `current_page` (ripresa affidabile, QR02).

## Blocked By

- [09-percettore-summary-glossary.md](./09-percettore-summary-glossary.md)

## Frontier

Bloccato da 09 (per ripristinare serve che tutti i tipi di stato — summary, glossario, cache — esistano). AFK: verificabile con DB locale, senza LLM.

## Step-by-Step Implementation Plan

1. **Comando `get_last_session`** e **`list_recent_documents(limit)`** (`src-tauri`): leggono l'ultima `sessions`/i `documents` per `last_opened_at`. Unit test su ordinamento e contenuto.
2. **Boot restore nel frontend**: allo startup, se esiste una sessione, ricarica documento+pagina+lingua+summary+glossario e renderizza. *Perché dopo 09*: ora tutti gli stati esistono.
3. **Verifica esistenza file** (EC06): prima di aprire, controlla che `file_path` esista; se no, prova a ricontrollare per hash i recenti; altrimenti UI di file mancante (rimuovi/individua). *Pitfall*: non cancellare la riga `documents`/cache quando il file manca — l'utente potrebbe rimetterlo.
4. **Persistenza `current_page`**: sposta qui la scrittura di `current_page` a ogni navigazione (se in 06 era solo in memoria) e `sessions.updated_at`. *Verifica*: chiudi e riapri → riparte dalla pagina giusta.

## Testing Plan

- **Rust unit**: `get_last_session`, `list_recent_documents` ordinati; update di `current_page`; comportamento con file_path inesistente.
- **Manuale**: leggere fino a pagina N, chiudere, riaprire → riparte da N con lingua/summary/glossario; spostare il file e verificare EC06; usare "Recenti".

## Out of Scope

- Sincronizzazione cloud (roadmap).
- Svuota cache / cartella dati (ticket 13).

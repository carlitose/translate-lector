# 10 — Pannello glossario (visualizza, modifica, blocca)

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.3, FR06, UC03, QR01

## What to Build

Pannello Glossario (dalla barra inferiore) che elenca i termini del documento corrente e permette all'utente di **modificare la traduzione**, **impostare/rimuovere il blocco** (`locked`), aggiungere una **nota** e vedere tipo e pagina di prima comparsa. Un termine **bloccato** diventa **vincolo assoluto**: da lì in poi il percettore (09) lo passa come tale e l'IA non lo cambia mai (UC03).

## Acceptance Criteria

- [ ] Il pannello mostra i termini di `glossary` del documento: source_term, translation, type, locked, note, first_seen_page.
- [ ] L'utente può modificare `translation` e `note` e salvarle (persistite).
- [ ] L'utente può attivare/disattivare `locked`; lo stato persiste.
- [ ] Dopo aver bloccato un termine, le traduzioni successive lo rispettano come vincolo assoluto (verificabile: il prompt lo marca come bloccato — riuso di 09).
- [ ] Modifiche a un termine non creano duplicati.

## Blocked By

- [09-percettore-summary-glossary.md](./09-percettore-summary-glossary.md)

## Frontier

Bloccato da 09 (serve un glossario popolato e il rispetto del flag `locked` nel prompt). AFK per UI+persistenza; l'effetto sulla traduzione è già coperto dalla logica di 09.

## Step-by-Step Implementation Plan

1. **Comandi core glossario** (`src-tauri`): `list_glossary(document_id)`, `update_glossary_term(id, translation, note, locked)`. Unit test su update e persistenza del flag.
2. **Frontend — pannello** (`src/`): tabella/lista editabile con toggle di blocco, campi translation/note, indicatore type/pagina. Salvataggio invoca i comandi. *Verifica*: `npm run check` pulito.
3. **Integrazione con il percettore**: nessun nuovo codice di prompt (09 già legge `locked`); verifica che un termine appena bloccato entri nel prompt della pagina successiva. *Pitfall*: assicurarsi che la lista bloccati passata al prompt rifletta l'ultimo stato salvato.

## Testing Plan

- **Rust unit**: list/update glossary; toggle locked persistente; nessun duplicato.
- **Manuale / QA gated**: bloccare un termine con una traduzione scelta, avanzare pagina, verificare (con key reale) che l'IA usi quella traduzione.

## Out of Scope

- Popolamento automatico dei termini (ticket 09).
- Modalità apprendimento/flashcard (roadmap).

## Completamento (2026-07-13)

Implementato end-to-end in TDD (RED→GREEN).

**Rust (`src-tauri/src/glossary.rs`, `lib.rs`)**
- `GlossaryEntry` ora espone `id` ed è `Serialize` (consumato dal pannello).
- Nuova `update_glossary_term(&Connection, id, translation, note, locked)`: UPDATE in-place sulla riga `id` — non tocca mai `source_term`/`type`/`first_seen_page` e non può creare duplicati.
- Comandi Tauri registrati: `list_glossary(document_id)`, `update_glossary_term(...)`.
- 6 nuovi unit test: id+campi in `list_glossary`; persistenza di translation/note/locked; toggle locked nei due sensi; nessun duplicato dopo update; **integrazione percettore**: un termine appena bloccato+modificato finisce nel blocco «vincolo assoluto» di `render_locked_unlocked` (riuso 09).

**Frontend (`src/lib/glossary.ts` + `.test.ts`, `src/lib/GlossaryPanel.svelte`, `src/routes/+page.svelte`)**
- Helper puri `isValidTranslation` / `toUpdateArgs` / `typeLabel` (6 vitest).
- `GlossaryPanel.svelte`: modale con tabella (source_term, translation editabile, tipo, toggle `locked`, nota editabile, `first_seen_page`), salvataggio per riga via `update_glossary_term`.
- Bottone `[Glossario]` nella barra inferiore (abilitato quando c'è una sessione), apre il pannello sul documento corrente.

**Verifica**: `cargo test` 61 passed (55+6) · `cargo build` ok · `vitest run` 21 passed (15+6) · `npm run check` 0 errori · `npm run build` ok.

**QA umana (GUI / effetto live — non automatizzabile)**
- Aprire un PDF, tradurre qualche pagina per popolare il glossario, aprire `[Glossario]`.
- Modificare una traduzione + nota, salvare, riaprire: i valori persistono.
- Attivare/disattivare `locked`: lo stato persiste.
- Bloccare un termine con una traduzione scelta, avanzare a una pagina non ancora tradotta (cache-miss) con una key OpenRouter reale e verificare che l'IA usi quella traduzione come vincolo assoluto (UC03).

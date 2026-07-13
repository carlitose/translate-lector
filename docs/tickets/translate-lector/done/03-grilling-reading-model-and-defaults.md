# 03 — Grilling: modello di lettura, hashing, versioni e default

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md)

## Type

grilling

## Outcome

Ottenere dall'utente le decisioni che sbloccano schema dati definitivo, UI e scaffolding.
Il nodo principale è la tensione **pagina vs scroll continuo** (la cache e il percettore sono per-pagina, ma `sessions.scroll_position REAL` implica scroll).

## Acceptance Criteria

- [x] Deciso il modello di lettura: **pagina discreta** → "pagina corrente" = unità di cache/percettore/ripristino; `scroll_position` inutilizzato nell'MVP.
- [x] Decisa la strategia di hashing: **hash parziale (SHA-256 primi+ultimi KB) + dimensione**.
- [x] Fissate le versioni: **Tauri v2, Svelte 5, `rusqlite`** (bundled).
- [x] Confermate: **lista di 15 lingue** (default, modificabile) + modello default `anthropic/claude-sonnet-5`, limite summary ~800-1000 token, prefetch ON, cartella `%APPDATA%/translate-lector`.
- [x] Registrate nel parent spec ("Decisions So Far" / "Risolte dall'autopilot").

## Resolution (2026-07-13)

Risposte dell'utente raccolte via domande strutturate (tutti i default consigliati accettati):
- **D1 = Pagina discreta.** Impatto schema: `sessions.scroll_position` diventa ridondante; mantenuto nello schema come no-op per futura opzione ibrida.
- **D2 = Hash parziale + dimensione.**
- **D3 = Tauri v2 + Svelte 5 + rusqlite** (nessuna rilavorazione: coincide con lo scaffold del ticket 05).
- **D4 = Lista di 15 lingue** adottata come default (assunzione: l'utente non ha personalizzato; campo libero resta per le altre).
- **D5 = Default consigliati** (modello `anthropic/claude-sonnet-5`, lingua Italiano, prefetch ON, limite summary ~800-1000 token, cartella `%APPDATA%/translate-lector`).

Dettaglio e trade-off: [decision-brief-grilling-03.md](../../specs/decision-brief-grilling-03.md).

## Blocked By

- None — can start immediately (idealmente informato da T01 sull'unità di testo).

## Frontier

È l'edge con più dipendenze a valle: senza queste decisioni umane, schema, UI e scaffold rischiano rilavorazione. Da attraversare presto.

## Work Plan

1. Preparare domande nette con opzioni e trade-off (usare la skill `grilling`).
2. Concentrarsi prima sul modello di lettura, poi hashing, poi versioni, poi default.
3. Registrare decisioni e assunzioni residue nel parent spec.

## Evidence to Capture

- Risposte dell'utente, verbatim dove conta.
- Assunzioni prese dove l'utente ha delegato la scelta.
- Impatti sullo schema SQLite (§4.3) e sulla UI (§3.1) se le decisioni divergono dalla spec.

## Out of Scope

- Implementazione. Qui si raccolgono solo decisioni.

# translate-lector — Wayfinding Spec

## Type

Wayfinding spec

## Status

Active

## Destination

Un'app desktop **Tauri + Svelte + Rust** funzionante (MVP) che:
apre un PDF con testo estraibile → mostra originale a sinistra e traduzione IA a destra →
mantiene coerenza via **percettore di contesto** (rolling summary + glossario bloccabile) →
persiste tutto in **SQLite locale** + API key nel keychain → ripristina la sessione alla riapertura,
usando **OpenRouter** come gateway LLM unico.

Contratto di dettaglio: [SPECIFICATION.md](../../SPECIFICATION.md).

## Decisions So Far

Tutte già fissate nella specifica (§7 "Riepilogo decisioni chiave"). In sintesi, e dove sono registrate:

- **Piattaforma**: desktop Tauri, single-user, tutto locale — SPECIFICATION.md §1, §4.1, NFR01-03.
- **Stack**: Svelte + TypeScript (webview) + pdf.js; core Rust; SQLite (`rusqlite`/`sqlx`); keychain via plugin Tauri — §4.1.
- **LLM**: OpenRouter, protocollo OpenAI chat-completions, un solo client, modello scelto dall'utente — §4.4.
- **Unità di traduzione**: pagina intera, on-demand + prefetch pagina successiva, con cache — §3.2, §7.
- **Coerenza**: rolling summary a limite fisso con auto-compressione + glossario dinamico con termini `locked` come vincolo assoluto — §3.3.
- **Chiamata IA**: una per pagina, output JSON `{ translated_text, updated_summary, new_glossary_terms }` — §4.4.
- **Sessione**: ripristino completo (PDF via hash, posizione, lingua, cache, glossario, summary) — §4.3, UC04.
- **Fuori ambito MVP**: OCR, TTS, account/cloud, modifica manuale della traduzione — §1.

### Risolte dall'autopilot (2026-07-13)

- **T02 — Contratto OpenRouter, structured output, token** → RISOLTO. Endpoint/auth confermati; `response_format: json_schema` (strict) con fallback a livelli indipendente dal modello; tokenizer = euristica `chars/4` calibrata su `usage` (no `tiktoken-rs` nell'MVP); prompt percettore redatto e round-trip-verificato vs §4.4. Dettaglio: [research-openrouter-contract.md](./research-openrouter-contract.md).
- **T04 — Keychain su Windows** → RISOLTO. `keyring` crate v4 → Windows Credential Manager; save/get/delete verificati su Win11 (scrittura silenziosa, no admin); Stronghold scartato (UX passphrase per un solo segreto). Implementato in `src-tauri/src/secrets.rs` (comandi `store/load/clear_api_key`).
- **T05 — Versioni stack** → FISSATO (in attesa di conferma umana su D3): Tauri 2.11, Svelte 5 (SvelteKit), `rusqlite` 0.32 (bundled), pdfjs-dist 6.1. Scaffold buildante a root.
- **T01 — Fedeltà estrazione pdf.js** → RISOLTO (verdetto). L'estrazione grezza NON basta: sillabazione a fine riga, ordine di lettura multi-colonna e header/footer ripetuti vanno gestiti. Una **ricostruzione basata su coordinate** (raggruppa per riga, rileva colonne per x-gap, unisce sillabazioni) rende il testo traducibile e va collocata **nel frontend**. Prototipo funzionante: `prototypes/pdfjs/`. Refinement noto (post-MVP): re-flow paragrafi vs a-capo di wrapping.
- **T03 — Decisioni umane (grilling)** → DECISO (2026-07-13). Vedi [decision-brief](./decision-brief-grilling-03.md). **D1 = pagina discreta** (⇒ `scroll_position` mantenuto ma inutilizzato nell'MVP); **D2 = hash parziale + dimensione**; **D3 = Tauri v2 + Svelte 5 + rusqlite**; **D4 = lista 15 lingue** come default; **D5 = default consigliati** (modello `anthropic/claude-sonnet-5`, lingua Italiano, prefetch ON, limite summary ~800-1000 token, cartella `%APPDATA%/translate-lector`).

## Not Yet Specified

*(Vuoto — tutte le incognite bloccanti sono state risolte. Le eventuali nuove emergeranno durante le build verticali.)*

## Out of Scope

- OCR di PDF scansionati (roadmap).
- Text-to-speech.
- Account, multi-utente, sincronizzazione cloud.
- Modifica manuale del testo tradotto.
- Multi-provider oltre OpenRouter.
- Packaging/firma/installer distribuibile (l'MVP è per uso personale; basta `tauri dev`/build locale).

## Frontier / Blocking Edges

Aggiornato 2026-07-13. **Tutte le indagini (T01-T05) sono chiuse e il gate umano (T03) è deciso.** La frontiera è ora la **build verticale dell'MVP**, in slice tracer-bullet:

1. **Slice 06 — Apri PDF ed estrai testo** (frontier attuale, ready): apri file → render pagina + estrazione+ricostruzione testo (da T01) → registra documento (hash parziale, D2) → crea/carica sessione (pagina discreta, D1). Nessuna traduzione ancora.
2. **Slice 07 — Traduci pagina + cache**: client OpenRouter nel core (da T02, prompt minimo senza percettore) → mostra traduzione a destra → salva in `translations_cache`. Usa API key (da T04) e modello di default (D5).
3. **Slice 08 — Percettore**: rolling summary + popolamento glossario dal JSON contract; passa contesto nel prompt; compressione summary alla soglia (D5).
4. **Slice 09 — Pannello glossario**: vedi/modifica/blocca termini; `locked` = vincolo assoluto nel prompt.
5. **Slice 10 — Persistenza & ripristino**: salva/ricarica sessione all'avvio (D1), cronologia PDF recenti, gestione file mancante (EC06).
6. **Slice 11 — Prefetch & stati/errori**: pre-traduzione pagina successiva (D5 ON), indicatori (spinner/cache/errore), retry+backoff (NFR06, EC07).
7. **Slice 12 — Impostazioni (⚙️)**: API key, modello, lingua, prefetch, limite summary, cartella dati, svuota cache.

Frontiera immediata = slice 06-07 (il tracer-bullet end-to-end). 08-12 si affinano dopo che il primo path passa.

## Ticket Plan

Cartella: `docs/tickets/translate-lector/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | prototype | Validare estrazione testo pdf.js | ✅ done (`done/`) — verdetto + prototipo `prototypes/pdfjs/` |
| 02 | research | Contratto OpenRouter + structured output + token | ✅ done (`done/`) — [research doc](./research-openrouter-contract.md) |
| 03 | grilling | Modello di lettura, hashing, versioni, default | ⛔ BLOCKED (human gate) — [decision brief](./decision-brief-grilling-03.md) pronto, attende conferma D1-D5 |
| 04 | prototype | Keychain API key su Windows 11 | ✅ done (`done/`) — `keyring` v4, verificato su Win11 |
| 05 | task | Scaffold Tauri + Svelte + TS + Rust | ✅ done (`done/`) — builda, 4/4 test, DB §4.3 |

Dopo la conferma di T03: rivedere la mappa e derivare i ticket di build verticali con `to-tickets`.

## Next Review

Quando T01-T04 sono chiusi e T05 builda:
1. Ripiegare evidenze e decisioni in questo spec (aggiornare "Decisions So Far", svuotare le incognite risolte).
2. Aggiornare eventualmente SPECIFICATION.md §3-4 se il modello di lettura o lo schema dati cambiano.
3. Generare i ticket di build verticali (tracer-bullet: apri PDF → estrai → traduci pagina → cache → persisti → ripristina) con `to-tickets`.

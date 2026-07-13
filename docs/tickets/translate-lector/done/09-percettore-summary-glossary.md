> ✅ Completato il 2026-07-13 — percettore completo esteso sopra il flusso 08 (nessuna riscrittura). Il prompt ora inietta il `rolling_summary` corrente e il glossario del documento (termini `locked` sotto l'intestazione "vincolo assoluto", `unlocked` come suggerimenti non vincolanti) tramite lo **stesso** builder di 08 (`llm::build_user_prompt`/`build_messages`, ora con contesto + flag `compress` + limite summary da settings). Euristica token `chars/ratio` (`est_tokens`, `needs_compression` a soglia 80%, `calibrate_chars_per_token` da `usage.prompt_tokens` persistito in `settings.chars_per_token`). Nuovo modulo `glossary.rs`: `list_glossary`, `render_locked_unlocked`, `insert_terms_deduped` (locked=0, `first_seen_page`=pagina, dedup case-insensitive per documento, termini `locked` mai toccati). `documents.rs`: `get/set_rolling_summary` sulla sessione. `settings.rs`: `summary_token_limit` (default D5 = 1000). `translate.rs`: `translate_page` orchestrata — cache-hit NON ri-esegue il percettore (documentato), su cache-miss carica summary+glossario, chunking EC04 (`split_into_chunks`, soglia 8000 char) con continuità del summary tra chunk e ricomposizione, poi persiste summary una volta (ricompresso se sopra soglia, EC05) e inserisce i nuovi termini deduplicati; `TranslationResult.updated_summary` aggiunto (frontend TS aggiornato, nessuna UI nuova — pannello glossario è il 10). 55 test Rust (33 preesistenti + 22 nuovi) + 15 vitest verdi; `npm run check` 0 errori; `cargo build` e `npm run build` ok. **QA live PENDENTE (solo umano)**: coerenza reale di summary/glossario su più pagine e crescita+compressione effettiva richiedono una API key OpenRouter valida dell'utente (non disponibile all'agente) — verificato finora solo con client MOCK (nessuna rete).

# 09 — Percettore: rolling summary + glossario automatico

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.3, §4.4, FR05/FR06, UC02, EC04/EC05 · contratto: [research-openrouter-contract.md](../../specs/research-openrouter-contract.md)

## What to Build

Estende la chiamata di traduzione (08) al **percettore completo**: il prompt ora include il **rolling summary** corrente e il **glossario** del documento; la risposta JSON (§4.4) fornisce `updated_summary` (salvato in `sessions.rolling_summary`) e `new_glossary_terms[]` (inseriti in `glossary`, non bloccati). Il summary ha **limite fisso con auto-compressione** (D5 ~800-1000 token; euristica `chars/4` calibrata su `usage`): oltre soglia il prompt istruisce il modello a ricomprimerlo (EC05). Pagine molto grandi → **chunking** e ricomposizione (EC04).

## Acceptance Criteria

- [ ] Il prompt di traduzione include summary attuale + glossario attuale (termini bloccati marcati come vincolo assoluto — anche se il locking UI arriva in 10, il flag `locked` è già rispettato nel prompt).
- [ ] Dopo ogni pagina, `sessions.rolling_summary` è aggiornato con `updated_summary`.
- [ ] `new_glossary_terms[]` sono inseriti in `glossary` con `locked=0`, `first_seen_page` = pagina corrente, senza duplicare termini già presenti.
- [ ] Quando la stima token del summary supera ~80% del limite (D5), la pagina successiva ne richiede la ricompressione e il summary risultante torna sotto soglia (EC05).
- [ ] Pagina oltre una soglia di caratteri → chunking in più chiamate e ricomposizione coerente di `translated_text` (EC04).

## Blocked By

- [08-translate-page-and-cache.md](./08-translate-page-and-cache.md)

## Frontier

Bloccato da 08. **Gate credenziali per QA reale** (API key OpenRouter dell'utente): logica e soglie testabili AFK con LLM mockato; coerenza reale del summary/glossario verificabile solo con chiamate vere.

## Step-by-Step Implementation Plan

1. **Estendi il prompt builder** (`llm.rs` da 08): inserisci sezioni summary + glossario (bloccati vs suggeriti) nel messaggio user, come da research doc. *Affects*: stesso builder, ora con contesto.
2. **Stima token** (`chars/4`): utility con ratio calibrabile da `usage.prompt_tokens`; funzione `needs_compression(summary, limit)`. Unit test su stringhe note.
3. **Persistenza summary**: dopo la risposta, aggiorna `sessions.rolling_summary`. *Verifica*: test che il valore persista e si ricarichi.
4. **Inserimento glossario deduplicato**: upsert in `glossary` evitando duplicati per (document_id, source_term); non toccare i termini `locked`. Unit test su dedup e su preservazione dei locked.
5. **Compressione (EC05)**: se `needs_compression`, il prompt della pagina successiva chiede esplicitamente di ricomprimere il summary mantenendo trama/entità/terminologia. *Verifica*: simulazione con summary lungo → mock ritorna summary compresso → torna sotto soglia.
6. **Chunking (EC04)**: se il testo pagina supera la soglia caratteri, spezza in chunk, traduci in sequenza mantenendo continuità (passa un mini-contesto tra chunk), ricomponi `translated_text`; aggiorna summary una volta a fine pagina. *Pitfall*: non esplodere i costi — chunk grandi ma sotto il budget di contesto del modello.

## Testing Plan

- **Rust unit** (client mockato): prompt include summary+glossario; `updated_summary` persistito; new terms deduplicati e con first_seen_page; soglia compressione scatta all'80%; chunking ricompone testo e non perde contenuto.
- **Manuale / QA gated**: con key reale, leggere 3-4 pagine di seguito e verificare coerenza terminologica e crescita+compressione del summary.

## Out of Scope

- UI di editing/blocco glossario (ticket 10) — qui solo popolamento automatico e rispetto del flag `locked`.
- Prefetch (ticket 12).

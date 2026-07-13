> ✅ Completato il 2026-07-13 — traduzione per pagina via OpenRouter con cache. Nuovi moduli Rust `llm.rs` (trait `ChatClient`, tipi §4.4, client `reqwest` bloccante con header `Authorization`/`HTTP-Referer`/`X-Title` + `response_format` json_schema strict, prompt builder estendibile dal 09, parsing a livelli a→b) e `translate.rs` (servizio `translate_page`: cache-hit senza chiamata, cache-miss chiama+parsa+salva, retry di correzione livello c, errore finale livello d, `usage.total_tokens` loggato per NFR04). Comando Tauri `translate_page` (key dal keychain, modello da settings, eseguito in `spawn_blocking`). Frontend: pannello destro mostra la traduzione (sola lettura) con stato di caricamento; EC03 → messaggio che rimanda a ⚙️. 33 test Rust (15 preesistenti + 18 nuovi) + 15 vitest (10 + 5 nuovi) verdi; `npm run check` 0 errori; `cargo build` e `npm run build` ok. **QA live PENDENTE (solo umano)**: la chiamata reale a OpenRouter richiede una API key valida dell'utente (non disponibile all'agente) — verificata finora solo con client MOCK.

# 08 — Traduci pagina via OpenRouter + cache

## Parent Spec

[translate-lector-wayfinder.md](../../specs/translate-lector-wayfinder.md) · design: [SPECIFICATION.md](../../../SPECIFICATION.md) §3.2, §4.4, FR03/FR07, UC02 · contratto: [research-openrouter-contract.md](../../specs/research-openrouter-contract.md)

## What to Build

All'arrivo su una pagina, se la traduzione è **in cache** (`translations_cache` per document_id+page+lingua) la mostra subito; altrimenti il **core Rust chiama OpenRouter** con un prompt **minimo** (testo pagina + lingua destinazione, **senza** summary/glossario — quelli arrivano al ticket 09), parsa la risposta e mostra `translated_text` a destra, salvandola in cache. Usa il client OpenRouter secondo il contratto della ricerca: `response_format: json_schema` con **fallback a livelli** (serde diretto → estrazione primo blocco `{...}` → 1 retry con prompt di correzione → errore). API key dal keychain (07), modello da `settings` (07).

## Acceptance Criteria

- [ ] Arrivando su una pagina non tradotta, parte una chiamata OpenRouter e la traduzione appare a destra (sola lettura).
- [ ] La traduzione è salvata in `translations_cache` (UNIQUE document_id+page_number+target_language) e riletta dalla cache alla riapertura della stessa pagina (nessuna seconda chiamata).
- [ ] Il client usa `Authorization: Bearer` (key dal keychain), header `HTTP-Referer`/`X-Title`, `response_format` json_schema con lo schema §4.4; parsing con fallback a livelli robusto.
- [ ] API key assente/invalida → messaggio chiaro che invita a configurarla (EC03), nessun crash.
- [ ] `usage.total_tokens` della risposta è registrato (per controllo costi NFR04).

## Blocked By

- [06-open-pdf-extract-sidebyside.md](./06-open-pdf-extract-sidebyside.md)
- [07-minimal-provider-config.md](./07-minimal-provider-config.md)

## Frontier

Bloccato da 06 (serve testo pagina + document/session) e 07 (serve key+modello). **Gate credenziali per la QA**: l'implementazione e i test unit girano AFK con LLM mockato, ma la verifica end-to-end reale richiede una **API key OpenRouter valida fornita dall'utente**.

## Step-by-Step Implementation Plan

1. **Client OpenRouter** (`src-tauri`, nuovo modulo `llm.rs`): funzione async `chat_completion(request) -> Result<Response>` con `reqwest` (aggiungi crate) verso `POST https://openrouter.ai/api/v1/chat/completions`. Header e body come da research doc. *Pitfall*: timeouts e TLS gestiti da reqwest; non loggare la key.
2. **Contratto percettore (versione minima)**: builder del prompt system+user con SOLO lingua+testo pagina (summary/glossario vuoti). Definisci i tipi Rust dell'output §4.4 (`translated_text`, `updated_summary`, `new_glossary_terms[]`) con serde. *Perché ora*: il ticket 09 estenderà lo stesso builder aggiungendo contesto.
3. **Parsing a livelli**: (a) `serde_json` diretto sul `content`; (b) estrazione primo blocco `{...}` bilanciato; (c) 1 retry con prompt di correzione; (d) errore. Unit test su ciascun livello con stringhe mock.
4. **Comando `translate_page`**: input document_id, page_number, target_language, page_text. Controlla cache → se assente chiama `llm` → salva in `translations_cache` → ritorna testo. *Affects*: `translations_cache`; test cache-hit/miss con client mock (astrai il client dietro un trait per iniettare un mock nei test).
5. **Frontend**: quando cambia pagina, invoca `translate_page`; mostra spinner durante la chiamata (stato base, raffinato in 12), poi il testo. Gestisci EC03 (key assente) con messaggio.
6. **Telemetria costi**: persisti/logga `usage.total_tokens` (colonna o log; minimale: log strutturato).

## Testing Plan

- **Rust unit** (AFK, client mockato dietro trait): cache-hit non chiama il client; cache-miss chiama e salva; parsing a livelli (json pulito, json con fence/preambolo, json malformato→retry→errore); errore key assente mappato a messaggio utente.
- **Manuale / QA gated su credenziali**: con una API key OpenRouter reale, tradurre una pagina vera e verificare qualità + salvataggio in cache + no-richiamo dalla cache.

## Out of Scope

- Rolling summary e glossario nel prompt (ticket 09).
- Prefetch e retry/backoff avanzato (ticket 12) — qui basta un errore chiaro.
- Chunking di pagine enormi (EC04) → introdotto in 09 con il percettore.

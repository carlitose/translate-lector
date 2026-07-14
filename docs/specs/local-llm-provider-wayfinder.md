# LLM locale (Unsloth Studio) come provider di traduzione — Wayfinding Spec

## Type

Wayfinding spec

## Status

Active

## Destination

Poter tradurre in translate-lector usando un **LLM locale servito via Unsloth Studio** (o un endpoint
locale OpenAI-compatible), **selezionabile come provider accanto a OpenRouter**, mantenendo intatto il
contratto del percettore (JSON strutturato `{ translated_text, updated_summary, new_glossary_terms }`,
SPECIFICATION.md §4.4) e la coerenza di summary + glossario.

Obiettivo: traduzione **offline / gratuita / privata** come alternativa al cloud, senza riscrivere il
motore di traduzione esistente.

## Decisions So Far

- **Provider aggiuntivo, non sostitutivo** (assunzione da "altra cosa voglio anche", 2026-07-14):
  OpenRouter resta; l'LLM locale è una scelta selezionabile in più.
- **Riuso del protocollo OpenAI chat-completions**: il client Rust esistente (`src-tauri/src/llm.rs`) parla
  già OpenAI chat-completions; un endpoint locale OpenAI-compatible si aggancia allo stesso client.
- **Setup Unsloth = CHIARITO** (Ticket 01, 2026-07-14). Unsloth Studio è un prodotto ufficiale (beta),
  UI locale no-code per training **e inferenza**; espone un endpoint **OpenAI-compatible**
  `/v1/chat/completions` (servito da `llama-server`), con **auth obbligatoria** (`Bearer sk-unsloth-…`) e
  **porta non fissa** (leggere dall'istanza; tipicamente 8000/8888). Il supporto `json_schema` non è
  documentato ma è ereditato da `llama-server` (con spigoli noti) e comunque coperto dalla ladder di
  degradazione dell'app. Alternative locali OpenAI-compat su Windows: **LM Studio** (miglior `json_schema`,
  porta 1234), **Ollama** (zero-config, 11434, ma ignora `response_format:json_schema` sul path `/v1`),
  **llama-server** (8080). **Conclusione**: nessun cambio di protocollo — basta rendere **base-URL + key**
  configurabili. Dettaglio + curl di verifica + fonti: [research-unsloth-serving.md](./research-unsloth-serving.md).
  *Aperto*: round-trip curl live non eseguibile AFK → verifica demandata all'utente (§6/§7 del doc).
- **Astrazione provider = PROGETTATA** (Ticket 02, 2026-07-14). Design chiuso e grounded nel codice:
  client rinominato `ChatCompletionsClient` con `base_url` + `requires_key` + key opzionale; provider
  built-in come preset in codice (openrouter | lmstudio | ollama | llama-server | unsloth), `base_url`/
  `model` sovrascrivibili e persistiti nella **tabella `settings` esistente** (nessuna nuova tabella);
  keychain **provider-scoped** con schema `"{provider_id}-api-key"` che rende `openrouter` →
  `openrouter-api-key` = account odierno (**zero migrazione**); guardia EC03 condizionale a `requires_key`;
  ladder di degradazione e fallback JSON invariati. Dettaglio + change-point (file:func) + slice di build:
  [design-provider-abstraction.md](./design-provider-abstraction.md). *Domande aperte per il gate umano*
  ripiegate nel Ticket 04.

## Fatti di codebase rilevanti (grounding)

- **Endpoint hardcoded**: `pub const OPENROUTER_URL` in `src-tauri/src/llm.rs:13`. Oggi la base-URL **non è
  configurabile** → il lavoro richiede un'astrazione di provider (base-URL + key opzionale + modello),
  non un semplice setting.
- **Ladder di degradazione già presente**: il client rimuove in ordine `provider` → `response_format`
  → `temperature` quando un endpoint non li supporta, poi ricade su **estrazione robusta del blocco JSON**
  (`llm.rs`, ricerca §2). Questo **de-rischia** il problema principale dei modelli locali piccoli che non
  onorano `response_format: json_schema` — il fallback esiste già.
- **API key oggi obbligatoria**: `isValidKey` richiede una chiave non vuota (`src/lib/providerConfig.ts`) e
  la key vive nel keychain (`src-tauri/src/secrets.rs`). Un provider locale spesso **non richiede chiave**
  → l'astrazione deve rendere la key opzionale per-provider.

## Not Yet Specified

- ~~Cos'è Unsloth Studio e come serve~~ → **RISOLTO dal Ticket 01** (vedi Decisions So Far).
- ~~Astrazione di provider nell'app~~ → **PROGETTATO dal Ticket 02** (vedi Decisions So Far +
  [design-provider-abstraction.md](./design-provider-abstraction.md)). Resta da *implementare* nelle build.
- **Tenuta del contratto percettore sul modello locale**: il modello locale onora `json_schema`? Se no, il
  fallback di parsing regge il JSON del percettore in modo affidabile? Qualità/latenza accettabili? → Ticket 03.
- **Decisioni umane**: quale modello/quantizzazione target, vincoli hardware (GPU/RAM), locale come default o
  opt-in, comportamento offline atteso. → Ticket 04.

## Out of Scope

- **Riscrittura del percettore o del contratto** (§4.4): si riusa così com'è; al più si adatta il parsing.
- **Fine-tuning di modelli con Unsloth**: qui interessa solo *servire e consumare* un modello locale, non
  addestrarlo.
- **Gestione del ciclo di vita del server locale dall'app** (avvio/stop di Unsloth dentro Tauri): l'MVP
  assume il server locale avviato dall'utente; l'orchestrazione in-app è post-MVP salvo diversa decisione (04).
- **Contratto di traduzione strutturata per blocchi OCR**: appartiene all'epica OCR
  ([ocr-layout-translation-wayfinder.md](./ocr-layout-translation-wayfinder.md), Ticket 05). Vedi
  "Interazione tra epiche".

## Interazione tra epiche

L'epica OCR introdurrà (suo Ticket 05) un contratto di **traduzione strutturata per-blocco**, più esigente
del testo semplice. Un modello locale piccolo potrebbe faticare su quel JSON più complesso. Il Ticket 03 di
questa mappa deve **annotare** la tenuta del contratto anche in vista di quell'output più ricco, così le due
epiche restano compatibili.

## Frontier / Blocking Edges

1. **Ticket 01 (research) — Unsloth Studio: cosa serve e come** *(ready, primo edge)*: senza sapere come
   Unsloth espone il modello (endpoint/protocollo/auth) non si può progettare l'integrazione né validare il
   contratto.
2. **Ticket 02 (research/design) — Astrazione provider nell'app** *(dipende da 01 per auth/endpoint)*:
   rende base-URL/key/modello configurabili per-provider e definisce il selettore + persistenza.
3. **Ticket 03 (prototype) — Validazione contratto percettore in locale** *(dipende da 01)*: prova che il
   modello locale produce il JSON del percettore (via json_schema o via fallback) con qualità/latenza usabili.
4. **Ticket 04 (grilling) — Decisioni umane** *(parallelo a 03; gate prima delle build)*: modello/quant,
   hardware, default vs opt-in, offline.

Dopo 01-04: **rivedere la mappa** e derivare i ticket di build verticali (selettore provider → chiamata a
endpoint locale → traduzione pagina con percettore → cache) con `to-tickets`.

## Ticket Plan

Cartella: `docs/tickets/local-llm-provider/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Unsloth Studio: come serve un LLM locale (endpoint/protocollo/auth) | ✅ done (`done/`) — [research](./research-unsloth-serving.md) |
| 02 | research | Astrazione di provider nell'app (base-URL/key/modello configurabili) | ✅ done (`done/`) — [design](./design-provider-abstraction.md) |
| 03 | prototype | Tenuta del contratto percettore su modello locale (json/fallback/qualità) | ⛔ blocked — richiede endpoint locale in esecuzione (non AFK) |
| 04 | grilling | Decisioni: modello/quant, hardware, default vs opt-in, offline | ready (gate umano) |

## Next Review

Quando 01-04 sono chiusi e 04 è deciso:
1. Ripiegare evidenze e decisioni in questa mappa.
2. Aggiornare SPECIFICATION.md §3.5/§4.1/§4.4: provider selezionabile (OpenRouter | Locale), endpoint
   configurabile, note sul contratto in locale; rivedere NFR sul funzionamento offline.
3. Derivare i ticket di build verticali con `to-tickets`, verificando la compatibilità con il contratto
   strutturato dell'epica OCR.

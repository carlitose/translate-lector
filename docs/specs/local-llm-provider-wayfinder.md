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
- **Validazione locale = ESEGUITA** (Ticket 03, 2026-07-14, endpoint reale dell'utente: Unsloth Studio
  `localhost:8888`, `gemma-4-E2B-it-qat-GGUF`; testato su quant QAT e poi Q4). **Contratto percettore 3/3
  in tutti e 4 i run** (schema/fallback × QAT/Q4) → integrazione tecnicamente validata, nessun blocco.
  **La qualità dipende dal quant**: QAT minuscolo → output rotto; **Q4 → traduzione fluente e corretta**.
  **`json_schema`**: dannoso sul QAT (meglio il fallback), **ok e utile sul Q4** (popola summary/glossario)
  → conferma il valore del toggle `response_format` per-provider. **Percettore inaffidabile su ~2B**
  (summary/glossario incostanti) → best-effort; per coerenza forte serve modello più grande. **Latenza
  ~38-42 s/pagina** sul Q4 in questo setup → lenta, mitigata da prefetch+cache; tenere il modello in GPU
  (D2). Dettaglio e tabella nel Ticket 03.
- **Decisioni umane = CONFERMATE** (Ticket 04, gate risolto 2026-07-14; dettaglio
  [decision-brief-local-llm-04.md](./decision-brief-local-llm-04.md)):
  **D1** modello = *da decidere dopo*, resta configurabile;
  **D2** hardware = GPU ~8GB → target **~7B Q4_K_M** in GPU;
  **D3** provider locale = **DEFAULT all'avvio** (non opt-in) → serve onboarding/health-check al primo avvio;
  **D4** server irraggiungibile = **errore chiaro, nessun fallback** automatico al cloud;
  **D5** auth = **sempre una chiave (anche finta)** → *semplifica il design*: si elimina il ramo
  key-opzionale/`requires_key=false`, `isValidKey` resta "non vuota", i server locali senza auth ricevono
  una chiave dummy;
  **D6** json_schema locale = **prova schema → fallback via ladder** (comportamento già esistente);
  **D7** ciclo di vita server = **utente avvia a mano + health-check** (no orchestrazione in-app nell'MVP).

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

*(Vuoto — tutte le incognite bloccanti sono state risolte dai Ticket 01-04. Le nuove emergeranno durante le
build verticali.)*

- ~~Cos'è Unsloth Studio e come serve~~ → **RISOLTO dal Ticket 01**.
- ~~Astrazione di provider nell'app~~ → **PROGETTATO dal Ticket 02** (da *implementare* nelle build; rivedere
  alla luce di D5/D3).
- ~~Tenuta del contratto percettore sul modello locale~~ → **VERIFICATO dal Ticket 03** (contratto 3/3;
  Q4 buono; percettore best-effort su ~2B).
- ~~Decisioni umane (modello/hardware/default/offline)~~ → **CONFERMATE dal Ticket 04** (D1-D7).

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

Aggiornato 2026-07-14: **TUTTE le indagini (01-04) sono chiuse.** Nessun edge di ricerca/decisione residuo.

1. ~~**Ticket 01** — Unsloth Studio: cosa serve e come~~ → ✅ **DONE** (research).
2. ~~**Ticket 02** — Astrazione provider nell'app~~ → ✅ **DONE** (design pronto per `to-tickets`).
3. ~~**Ticket 03** — Validazione contratto percettore in locale~~ → ✅ **DONE** (verdetto su endpoint reale).
4. ~~**Ticket 04** — Decisioni umane~~ → ✅ **DONE** (D1-D7 confermate).

**Build = FATTA (AFK, 2026-07-14).** I ticket di build 05-09 sono stati implementati in TDD e sono in
`done/`; il 10 (verifica e2e) è HITL e resta da fare manualmente contro il server locale.

| # | Build ticket | Stato |
|---|---|---|
| 05 | Core client con base-URL configurabile | ✅ done |
| 06 | Keychain provider-scoped | ✅ done |
| 07 | Preset provider + active-provider (default locale, D3) | ✅ done |
| 08 | UI selettore provider + base-URL + chiave/modello | ✅ done |
| 09 | Health-check + errore chiaro (no fallback, D4/D7) + onboarding | ✅ done |
| 10 | Validazione e2e su server reale + tuning default | ⛔ HITL — verifica manuale GUI |

Stato finale verde: `cargo test` 129, `svelte-check` 0 errori, `vitest` 66. Decisioni applicate: **D5** ha
semplificato il core (nessun ramo key-opzionale, chiave sempre richiesta), **D3** ha reso il provider locale
il default (`unsloth`), **D4/D7** realizzati come `LlmError::Unreachable` fail-fast + onboarding non
bloccante, senza fallback cloud. **Frontiera residua = solo il Ticket 10 (HITL)**: `npm run tauri dev`,
selezionare il provider locale, tradurre una pagina reale, affinare i default dei preset.

## Ticket Plan

Cartella: `docs/tickets/local-llm-provider/`

| # | Tipo | Titolo | Stato |
|---|------|--------|-------|
| 01 | research | Unsloth Studio: come serve un LLM locale (endpoint/protocollo/auth) | ✅ done (`done/`) — [research](./research-unsloth-serving.md) |
| 02 | research | Astrazione di provider nell'app (base-URL/key/modello configurabili) | ✅ done (`done/`) — [design](./design-provider-abstraction.md) |
| 03 | prototype | Tenuta del contratto percettore su modello locale (json/fallback/qualità) | ✅ done (`done/`) — verdetto: contratto 3/3; Q4 buono, percettore best-effort, ~40s/pag |
| 04 | grilling | Decisioni: modello/quant, hardware, default vs opt-in, offline | ✅ done (`done/`) — D1-D7 confermate: [decision-brief](./decision-brief-local-llm-04.md) |

## Next Review

Quando 01-04 sono chiusi e 04 è deciso:
1. Ripiegare evidenze e decisioni in questa mappa.
2. Aggiornare SPECIFICATION.md §3.5/§4.1/§4.4: provider selezionabile (OpenRouter | Locale), endpoint
   configurabile, note sul contratto in locale; rivedere NFR sul funzionamento offline.
3. Derivare i ticket di build verticali con `to-tickets`, verificando la compatibilità con il contratto
   strutturato dell'epica OCR.

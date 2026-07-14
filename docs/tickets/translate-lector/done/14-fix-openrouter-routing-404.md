# 14 — Fix: routing 404, deserializzazione risposta e modelli di default OpenRouter

## Parent Spec

[openrouter-404-routing-diagnosis.md](../../specs/openrouter-404-routing-diagnosis.md)

## What to Build

Rendere il client OpenRouter robusto e indipendente dal modello, risolvendo due bug confermati live (2026-07-14) e aggiornando i modelli di default:

- **Bug #1 — 404 routing**: il client manda sempre `provider:{require_parameters:true}` + `temperature`, e con modelli che non espongono `temperature` (es. `claude-sonnet-5`) il router risponde 404 "No endpoints found…". Inoltre manca il fallback model-agnostic (§2 ricerca) e il 404 è terminale.
- **Bug #2 — deserializzazione**: `ChatMessage.content: String` (riusato per la risposta) crasha quando `content` è `null`/assente → "error decoding response body".
- **Default modelli**: aggiornare a modelli attuali (luglio 2026) dal catalogo OpenRouter.

## Acceptance Criteria

- [ ] Il body di default NON include `provider:{require_parameters:true}` (o comunque non forza parametri opzionali che il modello può non supportare); una richiesta col modello di default non viene rifiutata dal router per `temperature`.
- [ ] Il messaggio di risposta usa `content: Option<String>` (envelope di risposta separato dalla richiesta); `content: null`/assente NON causa un errore di deserializzazione ma un esito gestito (contenuto vuoto → errore chiaro o fallback), verificato con un JSON di risposta reasoning-style.
- [ ] Fallback model-agnostic: un 404 con body contenente "No endpoints found"/"not supported" (e un 400 su parametro non supportato) innesca **un** retry con i parametri opzionali offensivi rimossi (`provider`, poi `response_format`, poi `temperature`), prima di mostrare errore. Coperto da test con MockClient.
- [ ] `DEFAULT_MODEL` aggiornato a un modello luglio-2026 valido con `structured_outputs` (default proposto `anthropic/claude-sonnet-4.6`); dropdown `COMMON_MODELS` aggiornato ai modelli attuali (vedi spec). Slug verificati esistenti sul catalogo live.
- [ ] Tutti i test esistenti restano verdi (85 Rust + 46 frontend) e si aggiungono regression per #1, #2 e il fallback.

## Blocked By

- None - can start immediately (diagnosi completata, consenso 3/3).

## Frontier

**Ready.** Diagnosi confermata (triangulate + QA live dell'utente). La verifica end-to-end reale contro OpenRouter resta human-only (serve la key dell'utente), ma il fix è interamente testabile con MockClient + JSON di risposta reali campione.

## Step-by-Step Implementation Plan

1. **Bug #2 (deserializzazione)** in `src-tauri/src/llm.rs`: introdurre un envelope di risposta dedicato (`ResponseMessage{ role:Option<String>, content:Option<String> }`, `#[serde(default)]`) usato da `Choice`; NON riusare `ChatMessage` (che resta per la richiesta con `content:String`). Cambiare `complete` a `resp.text()` + `serde_json::from_str` per errori diagnosticabili. `ChatResponse::content()` ritorna il testo o un errore chiaro se assente/vuoto. Test: deserializza una risposta con `content:null` senza panico/errore di decode; `content()` gestisce l'assenza.
2. **Bug #1 (routing)** in `build_request`: NON impostare `provider:{require_parameters:true}` di default; rendere `temperature` `Option<f32>` con `skip_serializing_if` (mantenerla per qualità ma ometterla nel fallback). Test: il body di default non contiene `require_parameters`.
3. **Fallback model-agnostic** in `OpenRouterClient::complete`/`translate.rs::complete_and_parse`: classificare 404 "No endpoints"/"not supported" e 400 unsupported-param come "degradabile"; un retry rimuovendo in ordine `provider` → `response_format` → `temperature`, affidandosi al prompt "solo JSON" + parser a livelli. Test MockClient: 1ª chiamata (body pieno) → 404 "No endpoints"; retry degradato → successo; asserire recupero e che non si ritenti all'infinito.
4. **Default modelli** in `settings.rs` (`DEFAULT_MODEL`) e nel dropdown frontend (`src/lib/providerConfig.ts` `COMMON_MODELS`): valori attuali luglio-2026 (default `anthropic/claude-sonnet-4.6`; lista: opus-4.8, sonnet-4.6, haiku-4.5, gemini-3.5-flash, gemini-3.1-pro-preview, gpt-4.1). Aggiornare i test che asseriscono il default.
5. Verificare tutta la suite + build.

## Testing Plan

- **Rust unit** (MockClient + JSON campione): deserializzazione con `content:null`; body default senza `require_parameters`; fallback 404→retry degradato→successo; classificazione errori invariata per 401/429/5xx.
- **Regola**: mantenere verdi gli 85 Rust + 46 frontend; aggiornare i test sul default model.
- **QA live (human-only, serve key utente)**: tradurre una pagina reale col nuovo default e con un modello reasoning senza temperature (deve funzionare); verificare che una risposta con content null non rompa più.

## Out of Scope

- Streaming delle risposte.
- Selezione automatica del modello in base ai `supported_parameters` (nice-to-have futuro).

## Completion Note (2026-07-14)

**Stato**: implementato end-to-end, TDD, tutte le verifiche verdi. QA live autenticata contro OpenRouter resta **human-only** (serve la key dell'utente).

**Cosa è cambiato**:

- **Bug #1 (routing 404)** — `src-tauri/src/llm.rs`:
  - `build_request` NON invia più `provider:{require_parameters:true}` (`provider: None`); `ChatRequest.temperature` è ora `Option<f32>` con `skip_serializing_if` (default `Some(0.2)` per qualità, omettibile).
  - Fallback model-agnostic: nuova `LlmError::UnsupportedParams` (non transient, degradabile); `is_unsupported_params_error(status,body)` classifica 404 "No endpoints found" / 400 unsupported-param; `ChatRequest::degrade()` strippa in ordine `provider` → `response_format` → `temperature`; `complete_with_fallback()` ritenta in modo **bounded** (max 4 tentativi, mai loop infinito). Cablato in `translate.rs::complete_and_parse` (sia chiamata iniziale sia correction retry).
- **Bug #2 (deserializzazione)** — `src-tauri/src/llm.rs`: nuovo envelope di risposta `ResponseMessage{ role:Option<String>, content:Option<String> }` (entrambi `#[serde(default)]`) usato da `Choice`; `ChatMessage{content:String}` resta solo per la richiesta. `OpenRouterClient::complete` ora fa `resp.text()` + `serde_json::from_str` (errori diagnosticabili). `ChatResponse::content()` ritorna errore chiaro su content null/vuoto/assente invece di un crash di decode.
- **Default modelli**: `settings.rs::DEFAULT_MODEL` → `anthropic/claude-sonnet-4.6`; `src/lib/providerConfig.ts` `DEFAULT_MODEL` idem e `COMMON_MODELS` rinfrescati (opus-4.8, sonnet-4.6, haiku-4.5, gemini-3.5-flash, gemini-3.1-pro-preview, gpt-4.1).

**Test aggiunti (regression)**: body default senza `require_parameters`; classificazione errori unsupported-param; `degrade()` ordine + bounded; `complete_with_fallback` recupero/bounded/pass-through; risposta con `content:null`/assente/blank gestita; recupero 404→retry degradato attraverso `translate_page`; 404 non-degradabile surfacato senza retry; nuovo `DEFAULT_MODEL` (Rust + frontend).

**Verifica**: `cargo test` 97 passed (85 + 12 nuovi); `cargo build` ok, 0 warning; `cargo clippy` 1 warning pre-esistente a `lib.rs:353` (non toccato); `vitest` 47 passed (46 + 1 nuovo); `npm run check` 0 errori; `npm run build` ok. (`tauri dev` NON eseguito.)

**QA live pendente (human-only)**: tradurre una pagina reale col nuovo default `claude-sonnet-4.6`; tradurre con un modello reasoning senza `temperature` (deve funzionare via fallback); verificare che una risposta con `content:null` non rompa più.

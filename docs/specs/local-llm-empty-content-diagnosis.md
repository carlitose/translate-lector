# Diagnosi — "content null/vuoto" con LLM locale reasoning (context 4096)

## Type

Diagnostic spec

## Status

Proposed (confidenza media-alta; un passo di conferma finale è il Ticket 01)

## Symptom

Traducendo una pagina di un PDF lungo con il **provider locale** (Unsloth Studio → `gemma-4-E2B-it-qat-GGUF`
via llama-server, `n_ctx = 4096`), l'app mostra:
`Errore di rete/servizio LLM: risposta senza contenuto testuale (content null/vuoto)`.
Con pagine cortissime (harness `prototypes/local-llm/validate-perceptor-contract.mjs`) invece funziona.

## Root Cause (consenso 3/3 angoli — triangulate-diagnosis)

Il modello locale è un **modello reasoning**: emette `reasoning_content` consumando molti token di
completamento **prima** di produrre `content`. Entro la finestra da **4096 token**, `prompt +
reasoning` esauriscono il budget: il server risponde `finish_reason: "length"` con `content` **vuoto/null**.
L'app converte questo in un **errore fatale** `LlmError::Http` (`src-tauri/src/llm.rs:159-170`,
`ChatResponse::content()`), non recuperabile dalla ladder/fallback.

Aggrava: la richiesta hardcoda **`max_tokens: 4096`** (`src-tauri/src/llm.rs:87, 829`) = **l'intera
context window**, quindi con qualunque prompt non vuoto non resta spazio per l'output.

L'ipotesi iniziale "il prompt supera 4096" è direzionalmente corretta ma imprecisa: non è solo la
dimensione del prompt, è **prompt + token di reasoning** contro la finestra, con `max_tokens` che non
riserva margine.

## Evidence

- `ChatRequest.max_tokens = 4096` hardcoded — `src-tauri/src/llm.rs:87`, valorizzato in `build_request`
  `src-tauri/src/llm.rs:829`.
- `ChatResponse::content()` ritorna `LlmError::Http("risposta senza contenuto testuale (content
  null/vuoto)")` quando `content` è null/blank — `src-tauri/src/llm.rs:159-170`. `Http` NON è transient
  (`is_transient`, `llm.rs:236`) né param-unsupported → nessun retry, nessun fallback: errore fatale.
- Il codice sa già che i modelli reasoning possono avere `content` null (commento `llm.rs:130-132`; test
  "bug #2" `llm.rs:1210-1214`): la **deserializzazione** non rompe, ma `content()` **fallisce comunque**.
- Repro parziali (subagenti, prima del cutoff per limite di sessione):
  - *Repro-first*: pagina corta OK ma `reasoning_content` 4648 char, `completion_tokens = 2207` → reasoning
    brucia il budget.
  - *Data-flow*: `max_tokens = 512` → **riprodotto** `finish_reason = length`, `content_len = 0`.
  - *Env*: modello caricato riporta `context_length: 4096`.

## Decision / Solution (raccomandata)

Combinazione robusta che funziona entro 4096 **senza** cambiare modello:

1. **Non chiedere `max_tokens` = intera finestra.** Renderlo sensato e/o configurabile per-provider,
   riservando margine (es. default ~1024, o `n_ctx − stima_prompt`). Un `max_tokens` = `n_ctx` è sempre
   sbagliato.
2. **Limitare/disabilitare il reasoning per la traduzione** dove il server lo consente (param stile
   `reasoning_effort`/`think:false`/`/no_think`, o istruzione nel template): la traduzione non richiede
   catena di pensiero, e il reasoning è ciò che esaurisce il budget. In alternativa consigliare un modello
   **non-reasoning** per la traduzione.
3. **Gestire `finish_reason == "length"` con content vuoto** in modo dedicato e **azionabile**: non il
   generico "risposta senza contenuto", ma un messaggio tipo *"Il modello locale ha esaurito il budget di
   token (probabile reasoning + finestra 4096). Usa un modello non-reasoning, riduci il testo, o aumenta il
   context nel server."* Valutare un **retry automatico** con reasoning disattivato / `max_tokens` ridotto
   prima di arrendersi.

Complementari (post): chunking del testo pagina per finestre piccole (EC04); nota di onboarding che il
provider locale rende meglio con `n_ctx` più ampio.

## Options Considered (scartate come causa primaria)

- **Solo dimensione prompt > 4096**: insufficiente da solo — pagine grandi con margine di output hanno
  funzionato in test; il fattore decisivo è il reasoning che consuma il budget di generazione.
- **json_schema/grammar rompe il modello piccolo**: plausibile concausa sulla *qualità* (vedi
  [ticket 03 done](../tickets/local-llm-provider/done/03-prototype-perceptor-contract-local.md)) ma il
  sintomo empty-content si riproduce per esaurimento token (`finish_reason=length`), non per rifiuto schema.
- **Bug di deserializzazione**: escluso — la deserializzazione gestisce già `content:null` (test bug #2).

## Open Questions

- Conferma finale con la richiesta **esatta** dell'app (`max_tokens:4096` + `response_format` json_schema +
  temperature + pagina reale) → Ticket 01. (I subagenti sono stati troncati dal limite di sessione prima di
  questo run decisivo.)
- Il server/modello espone un modo pulito per disattivare il reasoning via API? (Ticket 02.)

## Follow-up

Ticket sotto `docs/tickets/local-llm-empty-content/`.

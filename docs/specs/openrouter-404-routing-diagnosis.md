# Diagnosi — OpenRouter 404 "No endpoints found that can handle the requested parameters"

## Type

Diagnostic spec

## Status

Confirmed (consenso 3/3, confidenza alta — triangulate-diagnosis 2026-07-14)

## Symptom

Traducendo una pagina nell'app in esecuzione, la traduzione fallisce e la UI mostra:

```
Errore di rete/OpenRouter: 404 Not Found: {"error":{"message":"No endpoints found that can handle the requested parameters. To learn more about provider routing, visit: https://openrouter.ai/docs/guides/routing/provider-selection","code":404}}
```

## Root Cause (consenso)

`src-tauri/src/llm.rs` → `build_request` (~524-534) serializza **sempre** `temperature: 0.2` **e** `provider: { "require_parameters": true }`.
Il modello di default `DEFAULT_MODEL = "anthropic/claude-sonnet-5"` (`src-tauri/src/settings.rs:13`) è un modello *reasoning* che **non** espone `temperature` tra i `supported_parameters`.
Con `require_parameters: true`, OpenRouter instrada solo verso endpoint che supportano **ogni** parametro presente nel body; poiché nessun endpoint di Sonnet-5 accetta `temperature`, il router non trova endpoint idonei e restituisce esattamente il 404 osservato.

Concausa (safety net mancante): in `OpenRouterClient::complete` (`llm.rs:341-344`) un 404 diventa `LlmError::Http` **permanente** (`is_transient()` = false → nessun retry); `complete_and_parse` (`translate.rs:135-159`) ritenta **solo** su errore di parsing (richiede un body 2xx). Il fallback model-agnostic prescritto da [research-openrouter-contract.md](./research-openrouter-contract.md) §2 ("su parametro non supportato → retry senza il parametro") **non è implementato**.

## Evidence

- Catalogo pubblico `GET https://openrouter.ai/api/v1/models` (344 modelli): `anthropic/claude-sonnet-5` **esiste**; `supported_parameters` = `[include_reasoning, max_completion_tokens, max_tokens, reasoning, reasoning_effort, response_format, stop, structured_outputs, tool_choice, tools, verbosity]` → **`temperature` assente**, `response_format`/`structured_outputs` presenti.
- Confronto: `anthropic/claude-sonnet-4.5` e `openai/gpt-4o` includono `temperature`. I test unitari usano `gpt-4o` + mock → passano (85) senza mai colpire il difetto.
- Mappatura errore: `LlmError::Http("404 Not Found: {body}")` → `user_message()` = `"Errore di rete/OpenRouter: 404 Not Found: {body}"` → combacia carattere per carattere col sintomo.
- Nessun percorso di recupero: nessun branch per 404 "No endpoints"/parametri non supportati in `src-tauri/src`.

## Decision / Solution

Due modifiche coordinate in `src-tauri/src/llm.rs` (+ `translate.rs`):

1. **Non sovra-vincolare il routing (fix primario):** NON inviare `provider: { require_parameters: true }` di default. Senza `require_parameters`, OpenRouter ignora silenziosamente i parametri non supportati dal modello (es. `temperature`) e la chiamata riesce. Rendere inoltre `temperature` opzionale (`Option` + `skip_serializing_if`) così può essere omesso quando serve.
2. **Fallback model-agnostic (robustezza, come da §2 ricerca):** classificare il 404 con body contenente "No endpoints found"/"not supported" (e 400 su parametro non supportato) come condizione **retryabile una volta**, ri-inviando il body con i parametri opzionali offensivi rimossi (`provider`, poi `response_format`, poi `temperature`), affidandosi al prompt "solo JSON" + parser a livelli esistente.

Il default `anthropic/claude-sonnet-5` resta valido: con il fix funziona senza forzare `temperature`.

## Options Considered

- **Cambiare `DEFAULT_MODEL` a un modello con `temperature`** (es. `claude-sonnet-4.5`): maschera il problema ma non risolve il caso "l'utente sceglie qualsiasi modello". Scartato come fix unico.
- **Solo "retry senza `response_format`"** (come da lettera della ricerca): insufficiente — qui il parametro incriminato è `temperature`, non `response_format`.
- **Trattare il 404 come transient (backoff)**: errato — è permanente; il gap è il *param-relaxation*, non il backoff.

## Testing Decisions

- Regression con MockClient: primo tentativo con body "pieno" → errore 404 "No endpoints"; retry degradato (senza `provider`/`response_format`/`temperature`) → successo; asserire il recupero.
- Test che il body di default NON contenga `require_parameters: true`.
- Mantenere verdi gli 85 test Rust + 46 frontend.
- QA live (human-only, serve la key dell'utente): tradurre una pagina reale col default e con un modello non-structured-output.

## Related bug #2 — risposta non deserializzabile (confermato live 2026-07-14)

Dopo aver aggirato il 404 (cambiando modello), l'utente ha osservato un **secondo** errore distinto:
`Errore di rete/OpenRouter: risposta non deserializzabile: error decoding response body`.

**Root cause:** in `src-tauri/src/llm.rs` la struct `ChatMessage` ha `content: String` (riga ~56, non-opzionale) ed è **riusata sia per la richiesta sia per la risposta** (`Choice.message`, righe 97-100). Vari modelli — in particolare quelli *reasoning* — restituiscono `choices[0].message.content: null` (con il testo altrove, es. `reasoning`), oppure omettono il campo. `resp.json::<ChatResponse>()` (llm.rs:346) è quindi strict e fallisce con l'errore reqwest "error decoding response body", mappato a `LlmError::Http("risposta non deserializzabile: …")` (llm.rs:347).

**Fix:** separare l'envelope di **risposta** dalla struct di **richiesta**; il messaggio di risposta deve avere `content: Option<String>` (con `#[serde(default)]`), gestendo `null`/mancante in modo pulito (contenuto assente → errore chiaro o innesca il fallback, non un crash di deserializzazione). Preferire `resp.text()` + `serde_json::from_str` per messaggi d'errore diagnosticabili.

## Default model update (richiesta utente 2026-07-14)

Il default `anthropic/claude-sonnet-5` è un modello reasoning senza `temperature`. Aggiornare i default ai modelli **attuali (luglio 2026)** dal catalogo OpenRouter live. Proposta: default `anthropic/claude-sonnet-4.6` (qualità/costo, supporta `temperature`+`structured_outputs`); dropdown curato: `claude-opus-4.8`, `claude-sonnet-4.6`, `claude-haiku-4.5`, `google/gemini-3.5-flash`, `google/gemini-3.1-pro-preview`, `openai/gpt-4.1`. (Con il fix del bug #1, funzionano anche i modelli senza `temperature`.)

## Open Questions

- Nessuna bloccante. (Verifica live autenticata da rifare col key dell'utente dopo il fix.)

## Follow-up

Ticket: [14-fix-openrouter-routing-404.md](../tickets/translate-lector/14-fix-openrouter-routing-404.md) — copre bug #1 (routing), bug #2 (deserializzazione), e l'aggiornamento dei modelli di default.

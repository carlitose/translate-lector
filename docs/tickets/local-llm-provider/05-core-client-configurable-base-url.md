## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)
(design di dettaglio: [design-provider-abstraction.md](../../specs/design-provider-abstraction.md) §1, §6, §7-core)

## What to Build

Rendere il client LLM del core capace di parlare con **qualsiasi** endpoint OpenAI-compatible, non solo
con l'URL OpenRouter hardcoded. È il primo tracer-bullet: dopo questo ticket il client accetta una
`base_url`, ma l'app continua a comportarsi **esattamente come prima** (usa ancora OpenRouter con gli
stessi settaggi e la stessa chiave). Nessun selettore, nessuna persistenza nuova — solo il client reso
provider-agnostico.

Per **decisione D5** (l'utente inserisce sempre una chiave, anche fittizia per i server locali senza auth),
**non** si introduce il ramo "key opzionale / `requires_key`" previsto nella bozza di design: la chiave
resta **obbligatoria per tutti i provider** e la guardia EC03 resta universale. Questo semplifica il client.

## Acceptance Criteria

- [ ] Il client (oggi `OpenRouterClient`) è rinominato `ChatCompletionsClient` e accetta una `base_url` a
      costruzione; `complete()` fa `POST` su `self.base_url` invece della costante `OPENROUTER_URL`.
- [ ] La costante `OPENROUTER_URL` (e `HTTP_REFERER`/`X_TITLE`) resta come **default del preset openrouter**.
- [ ] Gli header specifici OpenRouter (`HTTP-Referer`, `X-Title`) vengono inviati **solo** per il provider
      openrouter (flag es. `send_openrouter_headers`), non per gli altri endpoint.
- [ ] La chiave resta obbligatoria: guardia EC03 (`MissingApiKey`) invariata per tutti i provider.
- [ ] I messaggi d'errore utente (`LlmError::user_message`) diventano **provider-neutrali** (niente
      "OpenRouter" hardcoded) mantenendo i prefissi dei codici EC (EC03, ecc.) usati dal frontend.
- [ ] La ladder di degradazione (`provider`→`response_format`→`temperature`) e l'estrazione JSON di
      fallback restano **invariate**.
- [ ] `lib.rs::translate_page` costruisce un `ChatCompletionsClient` con `base_url = OPENROUTER_URL` e la
      chiave attuale → **comportamento identico** a oggi. Nessun altro cambiamento osservabile.
- [ ] `cargo test` verde; `cargo build` ok.

## Blocked By

- None - can start immediately.

## Frontier

Ready now. È l'edge fondante: tutti gli altri ticket (presets, keychain, UI, health-check) presuppongono un
client che accetti una base-URL. Non dipende da nessun server locale.

## Step-by-Step Implementation Plan

1. **Rinomina il client** `OpenRouterClient` → `ChatCompletionsClient` in `src-tauri/src/llm.rs` (struct +
   `new` + tutti i riferimenti in `lib.rs`). Perché prima: il resto del lavoro si aggancia al nuovo nome.
   Verifica: `cargo build` compila dopo la rinomina meccanica.
2. **Aggiungi il campo `base_url: String`** (e `send_openrouter_headers: bool`) alla struct e a `new(...)`.
   In `complete()` sostituisci l'uso di `OPENROUTER_URL` con `self.base_url`. Verifica: il test che
   costruisce il client e fallisce prima della rete continua a passare.
3. **Condiziona gli header di attribuzione**: invia `HTTP-Referer`/`X-Title` solo se
   `send_openrouter_headers`. Verifica: nessun altro header cambia per openrouter.
4. **Rendi provider-neutrale `LlmError::user_message`**: sostituisci le stringhe "OpenRouter" con copy
   generica ("servizio LLM", "API key mancante…"), mantenendo i prefissi EC. Verifica: il test
   `missing_api_key_maps_to_ec03_message` (aggiornato se serve) resta verde e conserva "EC03"/"⚙️".
5. **Cabla `translate_page`** (`lib.rs`) per costruire `ChatCompletionsClient::new(OPENROUTER_URL.into(),
   api_key, /*openrouter*/ true)`. Perché ora: mantiene il comportamento identico finché i ticket 06/07
   non introducono la selezione. Verifica: avvio app + traduzione di una pagina con OpenRouter funziona
   come prima.
6. **Aggiorna i test del client**: `openrouter_client_with_empty_key_errors_before_network` →
   `ChatCompletionsClient` con chiave vuota → ancora `MissingApiKey`. (Niente variante `requires_key=false`:
   per D5 la chiave è sempre richiesta.) Verifica: `cargo test` verde.

Pitfall: non toccare la logica della ladder o del parsing JSON — è già corretta e testata; qui si cambia
solo *dove* si fa la POST e *quali* header/copy. Non introdurre `requires_key` (scartato da D5).

## Testing Plan

- Unit (Rust): client costruito con base-URL arbitraria; chiave vuota → EC03; header di attribuzione
  presenti solo per openrouter. I test esistenti di degrade/fallback/`build_request`/`response_format`
  devono restare verdi **senza modifiche** (non referenziano URL né struct).
- Manuale: `npm run tauri dev`, configurazione OpenRouter esistente, traduci una pagina → identico a prima.

## Out of Scope

- Presets di provider, `active_provider`, persistenza (Ticket 07).
- Keychain provider-scoped (Ticket 06).
- Qualsiasi UI (Ticket 08).
- Chiamata a un server locale reale (Ticket 10).

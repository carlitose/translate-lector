# Design — Provider abstraction (OpenRouter | Local OpenAI-compatible)

**Ticket:** `docs/tickets/local-llm-provider/02-research-provider-abstraction.md`
**Parent spec:** `docs/specs/local-llm-provider-wayfinder.md`
**Prior research:** `docs/specs/research-unsloth-serving.md` (conclusion: the app already speaks
OpenAI chat-completions and degrades gracefully; the only real change is making **base-URL + API key
configurable per provider**, and the key must be **optional** for local servers).
**Date:** 2026-07-14
**Status:** design complete — ready for `to-tickets`.

## TL;DR

The current client hardcodes one endpoint (`OPENROUTER_URL`, `src-tauri/src/llm.rs:13`), one keychain
account (`openrouter-api-key`, `src-tauri/src/secrets.rs:13`), and one always-required key (EC03 guard,
`src-tauri/src/llm.rs:405`). The smallest clean abstraction that supports OpenRouter **and** any local
OpenAI-compatible `/v1` server is:

1. Give the transport client a **`base_url`** field and a **`requires_key`** flag (instead of the const
   URL and the unconditional key guard).
2. Model providers as **built-in presets in code** (`openrouter` + local presets) whose **base-URL and
   model are overridable**, persisted in the **existing `settings` key-value table** (§4.3) — no new
   table. The active provider is one settings row (`active_provider`).
3. Store one API key **per provider** in the keychain under a **provider-scoped account name**
   (`{provider_id}-api-key`). The existing account `openrouter-api-key` is exactly this scheme for
   `openrouter`, so existing users' keys keep working with zero migration.
4. Make `isValidKey` / the save flow / the EC03 guard **conditional on `requires_key`**: OpenRouter still
   requires a key; a local provider with `requires_key=false` is accepted with no key.
5. Keep the degrade ladder and JSON fallback exactly as-is (they already de-risk local `json_schema`
   gaps). Make the OpenRouter-only bits (`HTTP-Referer`/`X-Title` headers, "OpenRouter" error copy)
   provider-neutral or conditional.

---

## 1. Making the hardcoded base-URL configurable

### Today

`src-tauri/src/llm.rs`:

- `pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";` (line 13).
- `OpenRouterClient { api_key, http }` (line 388) — no URL field.
- `complete()` posts to the const: `.post(OPENROUTER_URL)` (line 411), and always sends the two
  attribution headers (lines 414-415).

`src-tauri/src/lib.rs`:

- The `translate_page` command builds the client: `let base = llm::OpenRouterClient::new(api_key);`
  (line 256), with the key from `secrets::get_api_key()` (line 252) and the model from
  `settings::get_model(&conn)` (line 255).

### Change

Add a `base_url` field (and, per §4, `requires_key`) to the client and thread it from settings into the
request. `OPENROUTER_URL` stays as the **default base-URL for the openrouter preset**, not as the wired
constant.

```rust
// llm.rs — client carries its endpoint + whether a key is mandatory.
pub struct ChatCompletionsClient {          // renamed from OpenRouterClient (see Impact)
    base_url: String,
    api_key: Option<String>,                // None/blank allowed when !requires_key
    requires_key: bool,
    send_openrouter_headers: bool,          // gate HTTP-Referer/X-Title (§6)
    http: reqwest::blocking::Client,
}

impl ChatCompletionsClient {
    pub fn new(base_url, api_key: Option<String>, requires_key, send_openrouter_headers) -> Self { … }
}

impl ChatClient for ChatCompletionsClient {
    fn complete(&self, req) -> Result<ChatResponse, LlmError> {
        // EC03 guard is now conditional (§4).
        let key = self.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty());
        if self.requires_key && key.is_none() {
            return Err(LlmError::MissingApiKey);
        }
        let mut rb = self.http.post(&self.base_url).header("Content-Type", "application/json");
        if let Some(k) = key { rb = rb.bearer_auth(k); }         // attach only when present
        if self.send_openrouter_headers {
            rb = rb.header("HTTP-Referer", HTTP_REFERER).header("X-Title", X_TITLE);
        }
        let resp = rb.json(req).send().map_err(…)?;              // unchanged classification below
        …
    }
}
```

URL flow: `settings` (active provider + its base_url) → `lib.rs::translate_page` resolves the active
`ProviderConfig` → `ChatCompletionsClient::new(cfg.base_url, key, cfg.requires_key, cfg.id=="openrouter")`
→ `.post(&self.base_url)`. The `ChatClient` trait (line 348), `RetryingChatClient`,
`complete_with_fallback`, and the whole `translate.rs` service are **untouched** — they only see the
trait object.

**Decision — per-provider field, not a global override:** each provider owns its base-URL (OpenRouter's
default is the const; local's default is `http://localhost:PORT/v1/chat/completions`). A single global
base-URL override cannot express "OpenRouter *and* a local server side by side, switchable" which is the
destination (parent spec Destination).

---

## 2. Provider config data model

```
ProviderConfig {
    id:            string   // stable key: "openrouter" | "lmstudio" | "ollama" | "llamaserver" | "unsloth"
    label:         string   // UI label, e.g. "OpenRouter (cloud)"
    base_url:      string   // full /v1/chat/completions URL (overridable)
    requires_key:  bool     // OpenRouter=true; local presets=false (Unsloth=true)
    model:         string   // per-provider model id (overridable)
}
```

`id`, `label`, `requires_key` are **built-in constants** (not user-editable for built-ins). `base_url`
and `model` have **built-in defaults** but are **user-overridable** (stored only when overridden). This
mirrors the existing "default in code + optional stored override" pattern already used for `model`
(`settings.rs:16`, `providerConfig.ts:8`) and language (`settings.ts:29`).

### Built-in providers (from research §5 / §Q3)

| id | label | default base_url | requires_key | default model |
|----|-------|------------------|:---:|---------------|
| `openrouter` | OpenRouter (cloud) | `https://openrouter.ai/api/v1/chat/completions` (= `OPENROUTER_URL`) | **true** | `anthropic/claude-sonnet-4.6` (= `DEFAULT_MODEL`) |
| `lmstudio` | LM Studio (locale) | `http://localhost:1234/v1/chat/completions` | false | `local-model` |
| `ollama` | Ollama (locale) | `http://localhost:11434/v1/chat/completions` | false | `llama3.1` |
| `llamaserver` | llama.cpp server (locale) | `http://127.0.0.1:8080/v1/chat/completions` | false | `local-model` |
| `unsloth` | Unsloth Studio (locale) | `http://localhost:8000/v1/chat/completions` | **true** | `local-model` |

Notes grounded in research: local default models are placeholders the user must set to a loaded tag
(research §6 uses `local-model` / `llama3.1`). Unsloth's port is **not fixed** (research §Q2 caveat) →
its base-URL is a *starting* default the user is expected to correct. Unsloth `requires_key=true` because
Studio auth is mandatory (`sk-unsloth-…`, research §Q2).

### Choosing the active provider

One settings row: `active_provider` (default `"openrouter"`). `lib.rs::translate_page` reads it, loads
that provider's resolved config (defaults + overrides + key), and builds the client. No change to the
translation service signature.

---

## 3. Persistence — reuse the `settings` table (recommended)

**Recommendation: reuse the existing `settings` key/value table (§4.3, `db.rs:64`). Do NOT add a new
table.**

Why: the whole config is a handful of scalar values (active id + per-provider base_url/model overrides).
The `settings` table plus `get_setting`/`set_setting` (`settings.rs:42-59`) already handle exactly this.
A dedicated `providers` table would add a schema migration, a repository, and new Tauri commands for
zero functional gain — over-engineering for 5 built-in presets whose identity lives in code. Built-in
providers are **code constants**; the DB stores only *the active choice* and *overrides*.

### Keys / rows

| Key | Value | Notes |
|-----|-------|-------|
| `active_provider` | `openrouter` \| `lmstudio` \| … | default `openrouter` when unset/blank |
| `provider.<id>.base_url` | full URL | absent → built-in default for `<id>` |
| `provider.<id>.model` | model id | absent → built-in default for `<id>` |
| `model` *(legacy)* | model id | **existing** key; treated as `provider.openrouter.model` for back-compat (§8) |

No SQL/DDL change is required — these are just rows in the existing table. New typed accessors in
`settings.rs` (Rust) layer defaults on top, exactly like `get_model` (`settings.rs:63`):

```rust
pub const ACTIVE_PROVIDER_KEY: &str = "active_provider";
pub const DEFAULT_PROVIDER_ID: &str = "openrouter";

pub fn get_active_provider(conn) -> Result<String>            // default DEFAULT_PROVIDER_ID
pub fn provider_base_url_key(id) -> String                    // "provider.{id}.base_url"
pub fn provider_model_key(id)    -> String                    // "provider.{id}.model"
pub fn get_provider_config(conn, id) -> Result<ProviderConfig> // preset ∘ overrides; openrouter model
                                                               // falls back to legacy `model` key
```

### Where API keys live — keychain, provider-scoped (see §detail below)

Keys stay in the OS keychain (NFR07), **never** in `settings`. Multiple providers ⇒ multiple keychain
entries under a provider-scoped account name (§keychain scheme).

---

## 3b. Keychain account-naming scheme (`secrets.rs`)

### Today

`src-tauri/src/secrets.rs` uses one fixed service + one fixed account:

- `const SERVICE = "translate-lector";` (line 11)
- `const ACCOUNT = "openrouter-api-key";` (line 13)
- Public `set/get/delete/has_api_key()` (lines 21-41) hardcode `ACCOUNT`, delegating to the already
  **parametrised internals** `*_api_key_for(service, account, …)` (lines 45-67).

### Change — provider-scoped account name

Derive the account from the provider id:

```rust
/// Keychain account for a provider's key. openrouter → "openrouter-api-key"
/// (unchanged → existing users' keys are found as-is, §8).
fn account_for(provider_id: &str) -> String { format!("{provider_id}-api-key") }

pub fn set_api_key(provider_id: &str, key: &str) -> Result<(), Error>
    { set_api_key_for(SERVICE, &account_for(provider_id), key) }
pub fn get_api_key(provider_id: &str) -> Result<Option<String>, Error>
    { get_api_key_for(SERVICE, &account_for(provider_id)) }
pub fn delete_api_key(provider_id: &str) -> Result<(), Error>
    { delete_api_key_for(SERVICE, &account_for(provider_id)) }
pub fn has_api_key(provider_id: &str) -> Result<bool, Error>
    { has_api_key_for(SERVICE, &account_for(provider_id)) }
```

The parametrised internals (lines 45-67) and their real-store tests (lines 83-134) are **unchanged** —
they already take an arbitrary account. The public API just gains a `provider_id` argument. Because
`account_for("openrouter") == "openrouter-api-key"`, the existing single entry is transparently the
openrouter key (back-compat, §8).

The four Tauri commands (`store_api_key`/`load_api_key`/`clear_api_key`/`has_api_key`, `lib.rs:18-39`)
gain a `provider_id: String` parameter passed straight through.

---

## 4. Optional API key

### Frontend — `providerConfig.ts`

`isValidKey` (line 35) currently returns `false` for an empty key. Keep it as the *"is this a savable
non-empty key"* predicate, but make **acceptance to save depend on `requires_key`**:

```ts
// A key is required only for providers that mandate auth.
export function keyAcceptable(key, requiresKey: boolean): boolean {
    return requiresKey ? isValidKey(key) : true;   // local w/o key is fine
}
```

`ProviderConfig.svelte::save()` (lines 116-119) already stores the key **only when the user typed one**
(`if (isValidKey(keyInput))`). That logic stays: for a local provider the user simply leaves the field
empty and nothing is written to the keychain. Gate the "no key" *warning/blocking* on
`activeProvider.requires_key` so OpenRouter still nudges the user (EC03) while local does not.

### Core — EC03 guard

The guard now fires only when the provider mandates a key (§1 code):
`if self.requires_key && key.is_none() { return Err(LlmError::MissingApiKey); }`. For a local provider
with `requires_key=false` and no key: **no `Authorization` header is sent** (works for
llama-server/Ollama/LM Studio which accept no auth, research §Q3). If the user *does* type a dummy key
(e.g. `lm-studio`), it is sent as `bearer_auth` — also fine. The 401→`MissingApiKey` mapping
(`llm.rs:432`) stays for the case where a server unexpectedly demands auth.

**OpenRouter keeps requiring a key** (`requires_key=true`), so its behaviour is byte-identical to today.

---

## 5. UI — extend the settings panel (§3.5)

`src/lib/ProviderConfig.svelte` gains a **provider selector** at the top of the panel; the existing
"API key" and "Modello" fields become **provider-scoped**.

- **New: Provider `<select>`** bound to `activeProviderId` (options from a `PROVIDERS` preset array in
  `providerConfig.ts`, mirroring `COMMON_MODELS`). On change: load that provider's base_url/model/key
  state.
- **New: Base-URL field** (free text), shown for the active provider, prefilled with its resolved
  base_url (default or stored override). Sensible default per provider (table §2), e.g.
  `http://localhost:1234/v1/chat/completions` for LM Studio.
- **API key field** (existing, lines 231-241): label becomes provider-aware ("API key {label}"). When
  `!requires_key`, show it as optional (placeholder "opzionale per server locale") and drop the
  "nessuna key" warning styling.
- **Modello field** (existing, lines 243-255): unchanged widget; its value now saves to
  `provider.<id>.model`. Keep `COMMON_MODELS` for OpenRouter; local providers rely on the free-text
  field (the loaded model tag).

`load()` (lines 63-108) reads `active_provider`, then that provider's `base_url`/`model`/`has_api_key`.
`save()` (lines 110-141) writes `active_provider`, `provider.<id>.base_url`, `provider.<id>.model`, and
the key (only if typed) via the provider-scoped commands. The reading-preferences fields (language,
prefetch, summary limit, data dir, clear cache) are **provider-independent and stay exactly as they
are**.

Spec touch-up (deferred to the epic's Next Review, parent spec §Next Review): §3.5 table row "API key"
and "Modello" become "per provider (OpenRouter | locale)"; add a "Provider" + "Base URL" row; §4.4 title
"(OpenRouter)" → "(OpenAI-compatible: OpenRouter | locale)".

---

## 6. Degrade ladder + JSON fallback stay valid; OpenRouter-specific bits

**The ladder and fallback are unchanged and remain useful for local endpoints** — confirmed:

- `ChatRequest::degrade()` (`llm.rs:100`) strips `provider` → `response_format` → `temperature`. For
  local providers `provider` is already `None` (`build_request` sets `provider: None`, `llm.rs:717`), so
  the ladder effectively becomes `response_format` → `temperature` — exactly what a small local model
  needs when it rejects `json_schema` (research §Q3: Ollama ignores `response_format` on `/v1`,
  llama-server has rough edges).
- The layered `parse_content` / `extract_first_json_block` (`llm.rs:732`, `751`) is the ultimate
  safety net for models that emit prose around the JSON. Unchanged.
- `build_request` still sends `response_format` by default (`llm.rs:717`); the ladder drops it if the
  local server 400s on it. This is precisely the "low-risk" path the research relies on (research §Q2).

**OpenRouter-specific bits to make provider-neutral / conditional:**

| Item | Location | Action |
|------|----------|--------|
| `HTTP-Referer` / `X-Title` headers | `llm.rs:414-415` | Send **only when** `send_openrouter_headers` (i.e. `id=="openrouter"`). Harmless to a local server (research §5) but cleaner gated. |
| `is_unsupported_params_error` 404 signature `"no endpoints found"` | `llm.rs:494` | OpenRouter-specific routing phrase; a local server won't emit it. Leave as-is — it simply won't match for local, and the **400 param-cue path** (lines 498-512) still catches local `response_format` rejections. No change needed; note it. |
| Error copy hardcodes "OpenRouter" | `llm.rs:246-269` (`user_message`) | Make provider-neutral: EC03 → "API key {provider} mancante…"; `Http`/`Timeout`/`ServerError` → "Errore di rete/servizio LLM…". Cosmetic; keep the EC-code prefixes so the frontend hints still work. |
| Doc comments / module header naming "OpenRouter" | `llm.rs:1-8`, struct name | Rename `OpenRouterClient` → `ChatCompletionsClient`; keep `OPENROUTER_URL` const as the openrouter preset default. |

---

## 7. Impact list (change points) + test impact

### Core (Rust)

- `src-tauri/src/llm.rs`
  - Rename `OpenRouterClient` → `ChatCompletionsClient`; add fields `base_url`, `requires_key`,
    `send_openrouter_headers`; `api_key: Option<String>` (struct line 388, `new` line 393).
  - `complete()` (line 402): post to `self.base_url` (was `OPENROUTER_URL` line 411); conditional EC03
    guard (line 405); conditional `bearer_auth` and attribution headers (lines 413-415).
  - `LlmError::user_message()` (lines 243-271): provider-neutral copy.
  - Keep `OPENROUTER_URL`/`HTTP_REFERER`/`X_TITLE` consts as openrouter-preset defaults.
- `src-tauri/src/secrets.rs`: public `set/get/delete/has_api_key` gain `provider_id`; add
  `account_for()` (§3b). Internals + tests unchanged.
- `src-tauri/src/settings.rs`: add `ACTIVE_PROVIDER_KEY`, `DEFAULT_PROVIDER_ID`, a `ProviderConfig`
  struct + built-in preset table, `get_active_provider`, `provider_base_url_key`, `provider_model_key`,
  `get_provider_config` (with openrouter→legacy `model` fallback). Existing `get_model` kept (used by
  the legacy fallback).
- `src-tauri/src/lib.rs`
  - `translate_page` command (lines 241-271): resolve active `ProviderConfig`; build
    `ChatCompletionsClient::new(cfg.base_url, key_for(cfg), cfg.requires_key, cfg.id=="openrouter")`
    (was `OpenRouterClient::new(api_key)` line 256); fetch key via `secrets::get_api_key(&cfg.id)`.
  - `store/load/clear/has_api_key` commands (lines 18-39): add `provider_id` param.
  - New commands: `get_active_provider`, `set_active_provider` (or reuse `set_setting`),
    `get_provider_config` / `list_providers` for the UI; register in `generate_handler!` (line 358).
  - `db.rs`: **no change** (settings table reused).

### Frontend (TS/Svelte)

- `src/lib/providerConfig.ts`: add `PROVIDERS` preset array (id/label/base_url/requires_key/default
  model), `keyAcceptable(key, requiresKey)`, a base-URL resolver. Keep `DEFAULT_MODEL`/`COMMON_MODELS`/
  `resolveModel`/`isValidKey`/`isCommonModel`.
- `src/lib/ProviderConfig.svelte`: provider `<select>` + base-URL field; provider-scoped key/model
  load/save; provider-aware key labelling; pass `provider_id` to the key commands (§5).

### Tests impacted

Grounded in the current test suites:

- **`llm.rs` tests** — `openrouter_client_with_empty_key_errors_before_network` (line 1247) constructs
  `OpenRouterClient::new("   ")`: update to `ChatCompletionsClient` with `requires_key=true` (still
  expects `MissingApiKey`); **add** a twin with `requires_key=false` asserting an empty key does **not**
  error. `missing_api_key_maps_to_ec03_message` (line 1131) — update if copy changes (keep "EC03"/"⚙️").
  The degrade/fallback tests (`degrade_strips_…` line 1021, `complete_with_fallback_*` lines 1040-1093,
  `is_unsupported_params_error` tests lines 965-1018) **do not reference the URL or the client struct** →
  unaffected. `build_request_*` / `response_format` tests (lines 936-960) unaffected.
- **`translate.rs` tests** — all use the in-memory `MockClient` and `model: "openai/gpt-4o"`
  (e.g. lines 434-443), never the real client or URL → **unaffected**. `params.model` stays a `&str`.
- **`secrets.rs` tests** (lines 83-134) — already use the parametrised `*_for` internals with throwaway
  accounts → **unaffected**; optionally add an `account_for("openrouter") == "openrouter-api-key"`
  assertion to lock back-compat.
- **`settings.rs` tests** (lines 125-252) — existing `get_model`/setting round-trips unaffected; **add**
  tests for `get_active_provider` default, `get_provider_config` default+override, and the
  openrouter→legacy-`model` fallback.
- **Frontend** — `providerConfig` unit tests (if present) gain `keyAcceptable` + preset-resolution
  cases.

No search hit references `OPENROUTER_URL` from tests directly (it is only read inside `complete()`), so
renaming the const usage is contained to `llm.rs`.

---

## 8. Migration / back-compat

Existing users have exactly one keychain entry at service `translate-lector`, account
`openrouter-api-key` (`secrets.rs:11-13`) and possibly a `model` row in `settings`.

- **Keychain:** `account_for("openrouter") == "openrouter-api-key"` → the existing key is found
  unchanged. No migration code.
- **Active provider:** `active_provider` unset → defaults to `openrouter` (`get_active_provider`
  fallback). Existing users keep talking to OpenRouter with no action.
- **Model:** `get_provider_config("openrouter")` reads `provider.openrouter.model` and, when absent,
  **falls back to the legacy `model` key** (`settings.rs` `get_model`, line 63). So the existing chosen
  model is honoured. Optional one-time migration (copy `model` → `provider.openrouter.model`) is
  **not required** — the fallback makes it a no-op; recommend skipping it to keep the diff minimal.
- **Base-URL:** unset → openrouter default = `OPENROUTER_URL` = today's endpoint. Byte-identical
  requests for existing users.

Net: an untouched existing install behaves identically; the abstraction only *adds* the ability to
select a local provider.

---

## Ready for `to-tickets` — vertical build slices this design enables

Each slice is independently grabbable and end-to-end (tracer-bullet):

1. **Core client takes a base-URL + optional key.** Rename `OpenRouterClient` → `ChatCompletionsClient`
   with `base_url`/`requires_key`/optional key; conditional EC03 guard, auth header, attribution
   headers; provider-neutral error copy. Wire `lib.rs::translate_page` to still build an openrouter
   client from the same settings/keychain (no behaviour change). Tests: empty-key both ways.
2. **Provider presets + active-provider persistence (settings).** `ProviderConfig` + built-in table in
   `settings.rs`; `active_provider` + `provider.<id>.{base_url,model}` accessors with openrouter→legacy
   fallback; `translate_page` resolves the active config. Tests: defaults/overrides/fallback.
3. **Provider-scoped keychain.** `account_for()` + `provider_id` on the four secret commands and
   `secrets.rs` public fns; back-compat assertion for openrouter. 
4. **Settings UI: provider selector + base-URL + per-provider model/key.** Extend
   `providerConfig.ts` (presets, `keyAcceptable`) and `ProviderConfig.svelte`; provider-scoped
   load/save.
5. **(Depends on Ticket 03 outcome) Local-provider end-to-end validation & defaults tuning** — confirm
   a local `/v1` server translates a page through the degrade ladder; adjust preset defaults/model
   placeholders from real runs. (Gated by human decisions in Ticket 04: default vs opt-in, target
   model/quant.)

Slices 1-4 are pure abstraction/plumbing and can land before any local server exists; slice 5 needs a
running local endpoint (the AFK-deferred part, research §7).

## Open questions for the grilling ticket (04)

- **Local as default or opt-in?** Design defaults `active_provider=openrouter` (safe back-compat); the
  human decision (parent spec §Not Yet Specified) may flip the shipped default.
- **Dummy key vs no-auth for local.** Design sends *no* `Authorization` header when `requires_key=false`
  and no key typed. If any target local server rejects missing auth, the user must type a dummy key — is
  that acceptable UX, or should local presets ship a dummy key by default?
- **Unsloth base-URL default** is a guess (port not fixed, research §Q2). Ship `:8000` as a hint, or
  omit a default and force the user to paste the URL Studio prints?
- **Model dropdown for local.** Keep free-text only (loaded tag varies), or query the server's
  `/v1/models` to populate a dropdown (post-MVP)?
- **`json_schema` per local server** (Ticket 03 territory): if the local model neither honours
  `json_schema` nor survives the fallback reliably, does that gate shipping the local provider at all?

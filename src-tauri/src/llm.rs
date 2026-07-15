//! OpenAI-compatible chat-completions transport, percettore output types,
//! prompt builder and the layered JSON parsing fallback (SPECIFICATION §4.4,
//! ticket 08, research-openrouter-contract.md).
//!
//! The transport is abstracted behind [`ChatClient`] so the translation service
//! can be unit-tested with a mock, no network required. The real
//! [`ChatCompletionsClient`] uses a blocking `reqwest` client, targets any
//! configurable OpenAI-compatible endpoint (OpenRouter or a local `/v1` server)
//! and is exercised only by human QA against a live endpoint.

use serde::{Deserialize, Serialize};

/// OpenRouter chat-completions endpoint (§4.4).
pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
/// Attribution headers (optional; used only for OpenRouter leaderboards).
const HTTP_REFERER: &str = "https://github.com/translate-lector/translate-lector";
const X_TITLE: &str = "translate-lector";
/// JSON-schema name sent in `response_format` (§4.4). Parte del **contratto
/// completo**: dal flusso live usa il contratto snello (STC-10), ma il completo
/// resta per i test e per eventuale riuso — da qui `allow(dead_code)`.
#[allow(dead_code)]
const SCHEMA_NAME: &str = "percettore_output";

/// Default characters-per-token ratio for the `chars/4` heuristic (research §3).
/// Calibratable at runtime from `usage.prompt_tokens` (see
/// [`calibrate_chars_per_token`]).
pub const DEFAULT_CHARS_PER_TOKEN: f64 = 4.0;
/// Fraction of the summary limit that trips auto-compression (EC05, ~80%).
pub const COMPRESSION_THRESHOLD: f64 = 0.8;

// --- Percettore output (§4.4) ------------------------------------------------

/// A single glossary term proposed by the model (§4.4). `term_type` maps to the
/// JSON key `type` (a Rust keyword); its allowed values are
/// `nome proprio` | `tecnico` | `comune`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlossaryTerm {
    pub source_term: String,
    pub translation: String,
    #[serde(rename = "type")]
    pub term_type: String,
    pub note: String,
}

/// The full structured output returned (as a JSON string) in
/// `choices[0].message.content` (§4.4). **Contratto completo**: il flusso live
/// usa [`PerceptorUpdateOutput`] (snello, STC-10); questo resta per i test e per
/// eventuale riuso.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerceptoreOutput {
    pub translated_text: String,
    pub updated_summary: String,
    pub new_glossary_terms: Vec<GlossaryTerm>,
}

// --- Chat-completions envelope ----------------------------------------------

/// One chat message (`role` + `content`) sent in a **request**. The response
/// envelope uses the separate [`ResponseMessage`] (whose `content` is optional)
/// because some models return `choices[0].message.content: null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
}

/// Request body for `POST /chat/completions` (relevant MVP fields, §4.4).
///
/// `temperature`, `response_format` and `provider` are all optional and omitted
/// from the wire when `None` (`skip_serializing_if`). This lets the
/// model-agnostic fallback (§2, [`ChatRequest::degrade`]) strip the parameters a
/// given model may not advertise, avoiding the OpenRouter routing 404
/// ("No endpoints found that can handle the requested parameters").
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub max_tokens: u32,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<serde_json::Value>,
}

impl ChatRequest {
    /// Strip the next offending optional parameter for the model-agnostic
    /// fallback (research §2), in order: `provider` → `response_format` →
    /// `temperature`. Returns `true` when a field was removed (a retry is worth
    /// trying), `false` once nothing optional is left to relax. Bounds the
    /// fallback loop to at most three downgraded retries.
    pub fn degrade(&mut self) -> bool {
        if self.provider.is_some() {
            self.provider = None;
            true
        } else if self.response_format.is_some() {
            self.response_format = None;
            true
        } else if self.temperature.is_some() {
            self.temperature = None;
            true
        } else {
            false
        }
    }
}

/// Token accounting from the provider (`usage.total_tokens` persisted for
/// cost control, NFR04).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: i64,
    #[serde(default)]
    pub completion_tokens: i64,
    #[serde(default)]
    pub total_tokens: i64,
}

/// The assistant message inside a **response** choice. Distinct from the
/// request-side [`ChatMessage`]: `content` is optional because reasoning (and
/// some other) models return `choices[0].message.content: null` or omit it
/// entirely (the text may live under `reasoning`); it defaults so a
/// missing/`null` value never fails deserialization (bug #2). `role` is ignored
/// (serde drops unknown fields) — we only ever read the content.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ResponseMessage {
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    /// Why generation stopped (`stop` | `length` | …), when the provider reports
    /// it. Optional (defaults to `None` if missing). `length` with empty/null
    /// `content` means the model exhausted its completion budget — typically a
    /// reasoning model in a small context window — and is surfaced as the
    /// dedicated [`LlmError::OutputBudgetExhausted`] instead of the generic
    /// empty-content error (local-llm-empty-content, ticket 03).
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// Response body (relevant fields, §4.4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

impl ChatResponse {
    /// The assistant text of the first choice. Returns a clear error (never a
    /// decode panic) when there is no choice, or when `content` is `null`,
    /// missing or blank — the caller can then surface it or trigger the
    /// model-agnostic fallback (bug #2).
    pub fn content(&self) -> Result<&str, LlmError> {
        let choice = self
            .choices
            .first()
            .ok_or_else(|| LlmError::Http("risposta senza choices".into()))?;
        match choice.message.content.as_deref() {
            Some(c) if !c.trim().is_empty() => Ok(c),
            // Empty/null content: distinguish the "ran out of output budget"
            // case (finish_reason == "length", typical of a reasoning model in a
            // small context window) from a plain empty response. The former gets
            // a dedicated, actionable error (ticket 03); the latter keeps the
            // generic message (bug #2 reasoning-null path is unaffected: no
            // finish_reason == "length" there).
            _ if matches!(
                choice.finish_reason.as_deref(),
                Some(r) if r.eq_ignore_ascii_case("length")
            ) =>
            {
                Err(LlmError::OutputBudgetExhausted(
                    choice.finish_reason.clone().unwrap_or_default(),
                ))
            }
            _ => Err(LlmError::Http(
                "risposta senza contenuto testuale (content null/vuoto)".into(),
            )),
        }
    }

    /// Like [`content`], but additionally refuses a **truncated** completion: a
    /// non-empty content with `finish_reason == "length"` means the model ran out
    /// of budget mid-answer, so the text is cut off (e.g. "…con milioni o"). On
    /// the translate path we must NOT accept such a partial — it would silently
    /// drop half a page (unit-truncation-diagnosis, ticket 11) — so it is
    /// surfaced as [`LlmError::OutputTruncated`], letting the caller retry with a
    /// larger budget. Everything else defers to [`content`]: the empty+length
    /// case still maps to [`LlmError::OutputBudgetExhausted`] (EC08) exactly as
    /// before, and an empty/`null`/blank content stays the generic error.
    ///
    /// Deliberately a **separate** accessor. It is used by the whole translate
    /// path — including the single-unit / cloud-degrade case — so a truncation
    /// triggers a bounded retry there too (a truncated page is worse than a
    /// retried one). The **perceptor-update** path keeps calling [`content`] and
    /// retains its old accept-partial behaviour (it never emits page text).
    ///
    /// [`content`]: ChatResponse::content
    pub fn content_complete(&self) -> Result<&str, LlmError> {
        // Reuse content() for the no-choice / empty / empty+length (EC08) cases;
        // an Ok here means there IS a first choice with non-empty text.
        let text = self.content()?;
        let truncated = self
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_deref())
            .is_some_and(|r| r.eq_ignore_ascii_case("length"));
        if truncated {
            let reason = self
                .choices
                .first()
                .and_then(|c| c.finish_reason.clone())
                .unwrap_or_default();
            return Err(LlmError::OutputTruncated(reason));
        }
        Ok(text)
    }
}

// --- Errors ------------------------------------------------------------------

/// Failure modes of a translation call, each mapped to a user-facing message.
///
/// The transport-level variants are split so the retry layer can tell
/// **transient** failures (worth retrying with backoff: [`Timeout`],
/// [`ServerError`], [`RateLimited`], [`Offline`]) apart from **permanent** ones
/// (no point retrying: [`MissingApiKey`], generic [`Http`], [`ParseFailed`],
/// [`Storage`]). See [`LlmError::is_transient`].
///
/// [`Timeout`]: LlmError::Timeout
/// [`ServerError`]: LlmError::ServerError
/// [`RateLimited`]: LlmError::RateLimited
/// [`Offline`]: LlmError::Offline
/// [`MissingApiKey`]: LlmError::MissingApiKey
/// [`Http`]: LlmError::Http
/// [`ParseFailed`]: LlmError::ParseFailed
/// [`Storage`]: LlmError::Storage
#[derive(Debug, Clone, PartialEq)]
pub enum LlmError {
    /// No usable API key (EC03) — detected before any network call, or on 401.
    /// Permanent.
    MissingApiKey,
    /// Generic transport/HTTP failure that is not worth retrying (e.g. 400).
    Http(String),
    /// OpenRouter rejected the request because the chosen model/provider does
    /// not support one of the optional parameters we sent — a 404
    /// "No endpoints found that can handle the requested parameters" or a 400
    /// unsupported-parameter (research §2). Not transient, but **degradable**:
    /// the fallback strips the offending optional param and retries once
    /// ([`complete_with_fallback`], [`ChatRequest::degrade`]).
    UnsupportedParams(String),
    /// The request timed out. Transient (NFR06).
    Timeout(String),
    /// The provider returned a 5xx server error. Transient (NFR06).
    ServerError(String),
    /// The provider returned 429 (rate limit, EC07). Transient — retried with
    /// backoff and given a dedicated message.
    RateLimited(String),
    /// No network connection reached the provider (EC02). Transient: retried a
    /// bounded number of times, then surfaced so cached pages stay readable.
    Offline(String),
    /// A **local** provider endpoint refused the connection / could not be
    /// reached (connection refused, host down): the local server is simply not
    /// running or the address/port is wrong (D3/D7, ticket 09). Distinct from
    /// [`Offline`] (a remote endpoint unreachable = EC02): this carries the
    /// `base_url` and a dedicated, actionable message. **Permanent** (fail-fast):
    /// it is NOT retried, because spinning on a server that is down only delays
    /// the clear message. There is deliberately **no** automatic fallback to the
    /// cloud (decision D4).
    ///
    /// [`Offline`]: LlmError::Offline
    Unreachable(String),
    /// The response came back with empty/`null` `content` **and**
    /// `finish_reason == "length"`: the model exhausted its completion-token
    /// budget before emitting any text — typically a **reasoning** model whose
    /// chain of thought fills a small context window (local-llm-empty-content
    /// diagnosis, ticket 03). Distinct from the generic empty-content [`Http`]
    /// error: it carries a dedicated, actionable message (EC08 — change model /
    /// reduce text / raise `n_ctx`). **Permanent**: not transient (a retry with
    /// the same budget would likely burn it again) and not param-degradable
    /// (relaxing optional params does not add budget), so it surfaces
    /// immediately for the user to act. The carried string is the reported
    /// `finish_reason` (for diagnostics/logs).
    ///
    /// [`Http`]: LlmError::Http
    OutputBudgetExhausted(String),
    /// The response came back with **non-empty** content but
    /// `finish_reason == "length"`: the model emitted a **partial** answer and hit
    /// the completion-token budget mid-output — the text is truncated (e.g. a
    /// paragraph cut at "…con milioni o"). Distinct from
    /// [`OutputBudgetExhausted`] (empty content + length = EC08): here there IS
    /// text, but it is incomplete, so accepting it would silently drop half a page
    /// (unit-truncation-diagnosis, ticket 11). Surfaced only by the dedicated
    /// [`ChatResponse::content_complete`] used on the whole translate path
    /// (single-unit / cloud-degrade included); the perceptor-update path keeps
    /// [`ChatResponse::content`] and its old accept-partial behaviour. On the
    /// translate path this triggers a retry with a larger `max_tokens`
    /// ([`crate::translate`]); if the retry still truncates at the maximum
    /// headroom the partial is refused and re-surfaced as EC08
    /// ([`OutputBudgetExhausted`]). **Not transient** (a retry with the *same*
    /// budget would truncate again — the retry must GROW the budget) and not
    /// param-degradable. The carried string is the reported `finish_reason`.
    ///
    /// [`OutputBudgetExhausted`]: LlmError::OutputBudgetExhausted
    OutputTruncated(String),
    /// The response could not be parsed even after the correction retry.
    ParseFailed(String),
    /// A local storage error while reading/writing the cache.
    Storage(String),
    /// The job was interrupted at a unit boundary because it is no longer
    /// relevant: either the page it targets is no longer the current one (the
    /// user navigated away) or it is a prefetch that yielded the local-provider
    /// slot to a higher-priority on-demand request (ticket 06, L3/L4). This is
    /// **not** a real failure: it is checked only between units (never mid
    /// in-flight HTTP call), and the units already translated in this same run
    /// stay cached. **Permanent** (not transient — retrying a cancellation
    /// would just introduce an implicit retry the decision brief forbids) and
    /// not param-degradable. Its message is deliberately low-key: the frontend
    /// already discards stale results on its own, so this text is not expected
    /// to reach the user in practice.
    Cancelled,
}

impl LlmError {
    /// Whether this failure is worth retrying with backoff (NFR06/EC07). Only
    /// transport-transient errors qualify; a bad key, malformed JSON, a local
    /// storage error or a plain 4xx are permanent and returned immediately.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            LlmError::Timeout(_)
                | LlmError::ServerError(_)
                | LlmError::RateLimited(_)
                | LlmError::Offline(_)
        )
    }

    /// Whether this failure is an unsupported-parameter rejection that the
    /// model-agnostic fallback can recover from by relaxing the request body
    /// (research §2). See [`complete_with_fallback`].
    pub fn is_param_unsupported(&self) -> bool {
        matches!(self, LlmError::UnsupportedParams(_))
    }

    /// A clear, Italian, user-facing message. EC03 points the user at ⚙️;
    /// EC02/EC07 carry their code so the frontend can show a dedicated hint.
    pub fn user_message(&self) -> String {
        match self {
            LlmError::MissingApiKey => {
                "EC03: API key mancante o non valida. \
                 Configurala in ⚙️ (Impostazioni provider)."
                    .into()
            }
            LlmError::Http(m) => format!("Errore di rete/servizio LLM: {m}"),
            LlmError::UnsupportedParams(m) => format!(
                "Il modello selezionato non supporta i parametri richiesti \
                 (nessun endpoint compatibile). {m}"
            ),
            // The message is built in full (including any generic prefix) at
            // construction time in `classify_send_error` — same precedent as
            // `Unreachable` below — so it's owned as-is here. This avoids
            // double-prefixing the already-actionable local-timeout copy
            // (ticket 13 review) while the remote/generic copy still carries
            // its "Errore di rete/servizio LLM (timeout): " prefix, just
            // added at the source instead of here.
            LlmError::Timeout(m) => m.clone(),
            LlmError::ServerError(m) => format!("Errore del servizio LLM: {m}"),
            LlmError::RateLimited(m) => {
                format!("EC07: limite di richieste raggiunto (rate limit). Riprova tra poco. {m}")
            }
            LlmError::Offline(m) => {
                format!(
                    "EC02: nessuna connessione. Le pagine già tradotte restano \
                     leggibili dalla cache. {m}"
                )
            }
            LlmError::Unreachable(base_url) => {
                format!(
                    "Server locale non raggiungibile a {base_url}. \
                     Avvia il server (es. Unsloth Studio) o verifica l'indirizzo in ⚙️."
                )
            }
            LlmError::OutputBudgetExhausted(_) => {
                "EC08: il modello locale ha esaurito il budget di token \
                 (probabile reasoning entro una finestra piccola). Usa un modello \
                 non-reasoning, riduci il testo, o aumenta il context (n_ctx) del server."
                    .into()
            }
            LlmError::OutputTruncated(_) => {
                // Same actionable EC08 framing: the per-unit retry converts a
                // persistent truncation into OutputBudgetExhausted, but keep a
                // coherent message here in case this variant ever surfaces.
                "EC08: la traduzione è stata troncata (budget di token esaurito a \
                 metà risposta). Riduci il testo, usa un modello meno verboso, o \
                 aumenta il context (n_ctx) del server."
                    .into()
            }
            LlmError::ParseFailed(m) => {
                format!("Risposta del modello non valida (JSON non conforme): {m}")
            }
            LlmError::Storage(m) => format!("Errore della cache locale: {m}"),
            LlmError::Cancelled => {
                "Traduzione annullata: la pagina non è più quella corrente.".into()
            }
        }
    }
}

// --- Retry with exponential backoff (NFR06, EC07) ----------------------------

/// Bounded retry policy for transient transport failures. Backoff is
/// exponential: `base_delay * 2^attempt`. Tests use [`RetryPolicy::no_delay`]
/// so no wall-clock time is spent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts (including the first). Clamped to at least 1.
    pub max_attempts: u32,
    /// Delay before the first retry; doubles each subsequent retry.
    pub base_delay: std::time::Duration,
    /// Whether a [`LlmError::Timeout`] is retried with backoff like other
    /// transient errors (default `true`, preserving today's behaviour for
    /// OpenRouter/cloud). Ticket 13 / decision L4: for a systematically slow
    /// local server a timeout signals a real problem — retrying just triples
    /// the wait without helping — so local providers set this to `false`.
    /// Other transient errors (`ServerError`, `RateLimited`, `Offline`) are
    /// unaffected by this flag and keep retrying up to `max_attempts`.
    pub retry_on_timeout: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: std::time::Duration::from_millis(500),
            retry_on_timeout: true,
        }
    }
}

impl RetryPolicy {
    /// A policy with no backoff delay, for fast unit tests.
    #[cfg(test)]
    pub fn no_delay(max_attempts: u32) -> Self {
        Self { max_attempts, base_delay: std::time::Duration::ZERO, retry_on_timeout: true }
    }

    /// The retry policy to use for a given provider `base_url` (ticket 13,
    /// decision L4): a **local** endpoint does not retry on timeout — a
    /// systematically slow local server signals a real problem, and retrying
    /// just triples the wait without helping; a **remote**/cloud endpoint
    /// keeps the default (retries everything transient, including timeouts).
    ///
    /// Kept here next to [`is_local_url`] rather than inline at each call
    /// site so a future second caller (e.g. ticket 06 prefetch) reuses the
    /// same rule instead of re-deriving (and potentially dropping) it.
    pub fn for_base_url(base_url: &str) -> Self {
        if is_local_url(base_url) {
            Self { retry_on_timeout: false, ..Self::default() }
        } else {
            Self::default()
        }
    }

    /// Backoff delay before the retry that follows the given zero-based failed
    /// attempt index (attempt 0 → `base_delay`, attempt 1 → `2 * base_delay`…).
    pub fn backoff(&self, attempt: u32) -> std::time::Duration {
        self.base_delay * 2u32.saturating_pow(attempt)
    }
}

/// A [`ChatClient`] decorator that retries the inner client on transient errors
/// with exponential backoff (NFR06). Permanent errors (e.g. [`MissingApiKey`]
/// → EC03) are returned immediately without retrying.
///
/// [`MissingApiKey`]: LlmError::MissingApiKey
pub struct RetryingChatClient<'a> {
    inner: &'a dyn ChatClient,
    policy: RetryPolicy,
}

impl<'a> RetryingChatClient<'a> {
    pub fn new(inner: &'a dyn ChatClient, policy: RetryPolicy) -> Self {
        Self { inner, policy }
    }
}

impl ChatClient for RetryingChatClient<'_> {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let max = self.policy.max_attempts.max(1);
        let mut attempt: u32 = 0;
        loop {
            match self.inner.complete(req) {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    attempt += 1;
                    // Give up on the last attempt, on a permanent error, or
                    // (ticket 13 / decision L4) on a Timeout when the policy
                    // has retry_on_timeout disabled (local providers) — other
                    // transient errors are untouched by that flag.
                    let give_up_on_timeout =
                        matches!(e, LlmError::Timeout(_)) && !self.policy.retry_on_timeout;
                    if attempt >= max || !e.is_transient() || give_up_on_timeout {
                        return Err(e);
                    }
                    std::thread::sleep(self.policy.backoff(attempt - 1));
                }
            }
        }
    }
}

// --- Transport abstraction ---------------------------------------------------

/// A chat-completions transport. The real impl hits OpenRouter; tests inject a
/// mock. Blocking so the whole translation service stays synchronous (it also
/// does synchronous SQLite work) and trivially unit-testable.
pub trait ChatClient {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError>;
}

/// Call `client.complete`, and on an unsupported-parameter rejection (research
/// §2) retry with the offending optional params progressively stripped
/// (`provider` → `response_format` → `temperature`, via
/// [`ChatRequest::degrade`]), relying on the prompt's "JSON only" rules plus the
/// layered [`parse_content`]. Bounded: [`ChatRequest::degrade`] returns `false`
/// once nothing is left to relax, so this makes at most four attempts (full
/// body + three downgraded retries) — never an infinite loop. Any other error
/// (including a genuine, non-degradable 404) is returned immediately.
///
/// Returns the [`ChatResponse`] **and the request body that actually worked**
/// (possibly degraded). Callers reuse that working body — for the JSON
/// correction retry and for later chunks of the same page — instead of
/// re-probing the already-rejected params, saving one failed round-trip per
/// reuse. On the happy path (no degradation) the returned request equals the
/// input.
pub fn complete_with_fallback(
    client: &dyn ChatClient,
    req: &ChatRequest,
) -> Result<(ChatResponse, ChatRequest), LlmError> {
    let mut current = req.clone();
    loop {
        match client.complete(&current) {
            Ok(resp) => return Ok((resp, current)),
            Err(e) if e.is_param_unsupported() => {
                // Relax one more optional param; give up (surface the error) if
                // there is nothing left to strip.
                if !current.degrade() {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// Real chat-completions client backed by a blocking `reqwest` client, talking
/// to any OpenAI-compatible endpoint. `base_url` is the full
/// `/v1/chat/completions` URL (OpenRouter's is [`OPENROUTER_URL`], the openrouter
/// preset default); `send_openrouter_headers` gates the OpenRouter-only
/// attribution headers so other endpoints don't receive them.
pub struct ChatCompletionsClient {
    base_url: String,
    api_key: String,
    send_openrouter_headers: bool,
    http: reqwest::blocking::Client,
}

impl ChatCompletionsClient {
    /// `timeout_secs` bounds the whole request (connect + send + receive body),
    /// per-provider (ticket 13): explicit rather than relying on reqwest's
    /// undocumented-in-app 30s default. Falls back to an untimed client (never
    /// panics) if the builder were to fail, mirroring [`probe_reachable`]'s style.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        send_openrouter_headers: bool,
        timeout_secs: u32,
    ) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs.into()))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { base_url: base_url.into(), api_key: api_key.into(), send_openrouter_headers, http }
    }
}

impl ChatClient for ChatCompletionsClient {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        // EC03 guard: never touch the network without a key. Per decision D5 the
        // key is mandatory for every provider (local servers get a dummy key).
        if self.api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }

        let mut rb = self
            .http
            .post(&self.base_url)
            .bearer_auth(self.api_key.trim())
            .header("Content-Type", "application/json");
        // Attribution headers are OpenRouter-specific: send them only for the
        // openrouter preset, not for local/other OpenAI-compatible endpoints.
        if self.send_openrouter_headers {
            rb = rb.header("HTTP-Referer", HTTP_REFERER).header("X-Title", X_TITLE);
        }

        let resp = rb
            .json(req)
            .send()
            .map_err(|e| {
                // Classify transport failures so the retry layer can react
                // (timeout = transient; connection refused to a local server =
                // fail-fast Unreachable; to a remote endpoint = offline EC02).
                classify_send_error(e.is_timeout(), e.is_connect(), &self.base_url, e.to_string())
            })?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            // 401 -> invalid/missing key (EC03).
            return Err(LlmError::MissingApiKey);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // 429 -> rate limit (EC07): transient, retried with backoff.
            let body = resp.text().unwrap_or_default();
            return Err(LlmError::RateLimited(body));
        }
        if status.is_server_error() {
            // 5xx: transient, retried with backoff.
            let body = resp.text().unwrap_or_default();
            return Err(LlmError::ServerError(format!("{status}: {body}")));
        }
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            // Model-agnostic fallback (research §2): a 404 "No endpoints found"
            // or a 400 unsupported-parameter is not terminal — classify it so
            // the fallback can strip the offending optional param and retry.
            if is_unsupported_params_error(status.as_u16(), &body) {
                return Err(LlmError::UnsupportedParams(format!("{status}: {body}")));
            }
            return Err(LlmError::Http(format!("{status}: {body}")));
        }

        // Read the raw body then deserialize, so a strict/`null` `content`
        // yields a diagnosable error instead of an opaque reqwest decode error
        // (bug #2). The layered `content()`/`parse_content` handle the rest.
        let text = resp
            .text()
            .map_err(|e| LlmError::Http(format!("lettura risposta fallita: {e}")))?;
        serde_json::from_str::<ChatResponse>(&text).map_err(|e| {
            let snippet: String = text.chars().take(500).collect();
            LlmError::Http(format!("risposta non deserializzabile: {e}: {snippet}"))
        })
    }
}

/// Whether an unsuccessful OpenRouter response is an unsupported-*parameter*
/// rejection the fallback can recover from (research §2). Precise classification
/// so we neither waste downgrade retries on genuinely terminal 4xx (content
/// policy, invalid model, media type…) nor miss real parameter rejections:
///
/// - **404** → degradable ONLY when the message contains "no endpoints found",
///   the OpenRouter routing signature for an unsatisfiable parameter set.
/// - **400** → degradable ONLY when the message points at a *parameter* (a param
///   cue such as `parameter`/`temperature`/`response_format`/`provider`/
///   `require_parameters`/`structured_outputs`) AND signals rejection
///   (`unsupported`, `not supported`/`not a supported`, `does not support`,
///   `isn't supported`, `not accept`…).
/// - anything else (any other status, a bare 400/404, content policy, invalid
///   model) → NOT degradable (surfaced as a plain [`LlmError::Http`]).
///
/// Prefers the structured `error.message` when the body is JSON, falling back to
/// the raw body otherwise. Pure and unit-testable without the network.
pub fn is_unsupported_params_error(status: u16, body: &str) -> bool {
    if status != 404 && status != 400 {
        return false;
    }
    let message = error_message(body).unwrap_or_else(|| body.to_string());
    let m = message.to_lowercase();

    if status == 404 {
        return m.contains("no endpoints found");
    }

    // status == 400: only an unsupported/unaccepted *parameter* is degradable.
    let param_cue = m.contains("parameter")
        || m.contains("temperature")
        || m.contains("response_format")
        || m.contains("provider")
        || m.contains("require_parameters")
        || m.contains("structured_outputs");
    // Rejection: "unsupported" is explicit; the negated-"support"/"accept" check
    // catches "not supported"/"not a supported"/"does not support"/"isn't
    // supported"/"doesn't accept"/"does not accept" without enumerating every
    // phrasing.
    let negated = m.contains("not ") || m.contains("n't");
    let rejection_cue =
        m.contains("unsupported") || ((m.contains("support") || m.contains("accept")) && negated);
    param_cue && rejection_cue
}

/// Classify a transport-level `send()` failure into a typed [`LlmError`], given
/// the `reqwest` error flags and the target `base_url`. Split out as a pure,
/// unit-testable function because constructing a real `reqwest::Error` in a test
/// is awkward (ticket 09):
///
/// - **timeout** → [`Timeout`] (transient, retried with backoff, NFR06).
/// - **connect** (connection refused / cannot connect to host): for a **local**
///   endpoint → [`Unreachable`] carrying the `base_url` (the local server is
///   down — fail-fast, no retry, no cloud fallback, D4); for a **remote**
///   endpoint → [`Offline`] (EC02, no connection).
/// - anything else → [`Http`] (permanent).
///
/// [`Timeout`]: LlmError::Timeout
/// [`Unreachable`]: LlmError::Unreachable
/// [`Offline`]: LlmError::Offline
/// [`Http`]: LlmError::Http
pub fn classify_send_error(
    is_timeout: bool,
    is_connect: bool,
    base_url: &str,
    msg: String,
) -> LlmError {
    if is_timeout {
        if is_local_url(base_url) {
            // Ticket 13: a timeout against a LOCAL endpoint is actionable —
            // point the user at the configurable per-provider timeout, a
            // faster model, or a smaller n_ctx, instead of the generic copy.
            // The message is complete as-is (no generic prefix added later by
            // `user_message`, to avoid double-prefixing — ticket 13 review).
            LlmError::Timeout(
                "Il server locale è troppo lento o ha chiuso la connessione. \
                 Aumenta il timeout in ⚙️ (Impostazioni provider), usa un modello \
                 più veloce, o riduci n_ctx del server."
                    .to_string(),
            )
        } else {
            // Remote/generic case: build the full user-facing message here
            // (including the prefix) so `user_message` can own every variant
            // uniformly, same as `Unreachable`.
            LlmError::Timeout(format!("Errore di rete/servizio LLM (timeout): {msg}"))
        }
    } else if is_connect {
        if is_local_url(base_url) {
            LlmError::Unreachable(base_url.to_string())
        } else {
            LlmError::Offline(msg)
        }
    } else {
        LlmError::Http(msg)
    }
}

/// The `host[:port]` authority slice of `url`, i.e. everything after the scheme
/// and before the path, with any `user:pass@` userinfo stripped. Shared by
/// [`is_local_url`] and [`port_from_base_url`] so the scheme-strip /
/// authority-split / userinfo-strip steps live in one place. Pure string parsing
/// (no DNS). Note: bracketed IPv6 literals keep their brackets here (`[::1]:8080`);
/// the callers unwrap them as needed.
fn authority_host_port(url: &str) -> &str {
    // Drop the scheme, then keep only the authority (up to the first '/').
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let authority = after_scheme.split('/').next().unwrap_or("");
    // Strip any userinfo ("user:pass@host").
    authority.rsplit('@').next().unwrap_or(authority)
}

/// Whether `url`'s host is a loopback/local address (`localhost`, `127.x.x.x`,
/// `::1`, `0.0.0.0`). Used to tell a **local server down** ([`LlmError::Unreachable`])
/// apart from a **remote endpoint unreachable** ([`LlmError::Offline`], EC02) on
/// a connection failure. Pure string parsing (no DNS), unit-testable.
pub fn is_local_url(url: &str) -> bool {
    let host_port = authority_host_port(url);
    // Strip the port, honouring bracketed IPv6 literals ("[::1]:8080").
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    let h = host.to_ascii_lowercase();
    h == "localhost" || h == "::1" || h == "0.0.0.0" || h.starts_with("127.")
}

/// Extract the explicit TCP port from a chat-completions `base_url`, honouring
/// bracketed IPv6 literals (`http://[::1]:8080/v1` → `8080`). Returns `None`
/// when the authority carries no explicit `:port` — the scheme default is
/// deliberately **not** inferred: the llama-server spawner (ticket 04) needs a
/// concrete `--port`, and falling back to a hardcoded default is the caller's
/// choice, not this parser's. Pure string parsing (no DNS), unit-testable.
pub fn port_from_base_url(url: &str) -> Option<u16> {
    let host_port = authority_host_port(url);
    let port_str = if let Some(rest) = host_port.strip_prefix('[') {
        // "[::1]:8080" → after the closing bracket, drop the leading ':'.
        rest.split(']').nth(1)?.strip_prefix(':')?
    } else {
        // "host:port" → the part after the single ':'. No ':' → no port.
        let mut parts = host_port.splitn(2, ':');
        let _host = parts.next();
        parts.next()?
    };
    port_str.parse::<u16>().ok()
}

/// Derive a cheap `/v1/models` reachability-probe URL from a chat-completions
/// `base_url` (ticket 09). `…/v1/chat/completions` → `…/v1/models`; any other
/// URL is probed as-is (a successful TCP connection is what proves reachability,
/// regardless of the path). Pure and unit-testable.
pub fn models_probe_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    match trimmed.strip_suffix("/chat/completions") {
        Some(prefix) => format!("{prefix}/models"),
        None => trimmed.to_string(),
    }
}

/// Cheap, dependency-free reachability probe for the onboarding hint (ticket 09):
/// a short-timeout blocking `GET` to the provider's [`models_probe_url`]. Returns
/// `true` when the server answers **at all** (any HTTP status, even 401/404 — the
/// endpoint is up), `false` on connection refused / timeout / DNS error. Never
/// surfaces an error: "down" is simply `false`. It performs no cloud fallback and
/// never translates — it only checks whether the endpoint is listening.
pub fn probe_reachable(base_url: &str) -> bool {
    let url = models_probe_url(base_url);
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client.get(&url).send().is_ok()
}

/// Extract a human-readable error string from an OpenRouter error body. Handles
/// `{"error":{"message":"…"}}` (the common shape), `{"error":"…"}` and a
/// top-level `{"message":"…"}`. Returns `None` when the body is not JSON or has
/// no such field, so the caller can fall back to matching the raw body.
fn error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    match v.get("error") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(obj) => obj.get("message").and_then(|m| m.as_str()).map(str::to_string),
        None => v.get("message").and_then(|m| m.as_str()).map(str::to_string),
    }
}

// --- response_format schema (§4.4) ------------------------------------------

/// The JSON schema passed in `response_format.json_schema.schema`, matching
/// §4.4 field-for-field (`strict: true` requires `additionalProperties: false`
/// and every property in `required`). **Contratto completo** (con
/// `translated_text`): il flusso live usa [`perceptor_update_response_format`]
/// (snello, STC-10); questo resta per i test e per eventuale riuso.
#[allow(dead_code)]
pub fn response_format() -> serde_json::Value {
    serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": SCHEMA_NAME,
            "strict": true,
            "schema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["translated_text", "updated_summary", "new_glossary_terms"],
                "properties": {
                    "translated_text": { "type": "string" },
                    "updated_summary": { "type": "string" },
                    "new_glossary_terms": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["source_term", "translation", "type", "note"],
                            "properties": {
                                "source_term": { "type": "string" },
                                "translation": { "type": "string" },
                                "type": { "type": "string", "enum": ["nome proprio", "tecnico", "comune"] },
                                "note": { "type": "string" }
                            }
                        }
                    }
                }
            }
        }
    })
}

// --- Token heuristic (research §3, EC05) -------------------------------------

/// Estimate the token count of `text` with the `chars/ratio` heuristic
/// (research §3). Deterministic and model-independent; `ratio` defaults to
/// [`DEFAULT_CHARS_PER_TOKEN`] but can be calibrated per model from real
/// `usage.prompt_tokens` via [`calibrate_chars_per_token`].
pub fn est_tokens(text: &str, ratio: f64) -> u32 {
    let chars = text.chars().count() as f64;
    let ratio = if ratio > 0.0 { ratio } else { DEFAULT_CHARS_PER_TOKEN };
    (chars / ratio).ceil() as u32
}

/// Whether the rolling summary should be recompressed before the next page:
/// `true` once its estimated tokens reach [`COMPRESSION_THRESHOLD`] (~80%) of
/// `limit` (EC05, §3.3). Uses the default heuristic ratio for a stable,
/// unit-testable threshold.
pub fn needs_compression(summary: &str, limit: u32) -> bool {
    let est = est_tokens(summary, DEFAULT_CHARS_PER_TOKEN) as f64;
    est >= limit as f64 * COMPRESSION_THRESHOLD
}

/// Observed characters-per-token ratio from a real call: `prompt_chars /
/// usage.prompt_tokens`. `None` when either input is non-positive (nothing to
/// calibrate from). Persisted by the service to refine future estimates.
pub fn calibrate_chars_per_token(prompt_chars: usize, prompt_tokens: i64) -> Option<f64> {
    if prompt_chars == 0 || prompt_tokens <= 0 {
        return None;
    }
    Some(prompt_chars as f64 / prompt_tokens as f64)
}

// --- Prompt builder ----------------------------------------------------------

/// Extra user-message instruction appended when the rolling summary is over the
/// compression threshold (EC05): the model must recompress it this page.
pub const COMPRESSION_INSTRUCTION: &str =
    "ATTENZIONE: il RIASSUNTO PROGRESSIVO ha raggiunto il limite. Nel campo \
\"updated_summary\" RICOMPRIMILO in modo piu conciso, mantenendo solo trama, entita, \
terminologia e fatti utili alla coerenza futura, così da tornare ben sotto il limite di token.";

/// System message: fixes the percettore role, the output schema and the "JSON
/// only" rules (research §4). The summary budget is injected from settings
/// (§3.5, decision D5) so the prompt reflects the configured limit. **Contratto
/// completo**: il flusso live usa [`build_perceptor_update_system_prompt`]
/// (snello, STC-10); questo resta per i test e per eventuale riuso.
#[allow(dead_code)]
pub fn build_system_prompt(summary_token_limit: u32) -> String {
    format!(
        "Sei il motore di traduzione di translate-lector. Traduci il testo di UNA pagina di un \
documento verso la lingua di destinazione indicata, mantenendo la coerenza con il resto del documento.\n\n\
Devi:\n\
1. Tradurre l'intero testo della pagina in modo fedele, naturale e coerente col tono del documento. \
Non riassumere, non omettere: traduci tutto il contenuto.\n\
2. Rispettare in modo ASSOLUTO le traduzioni dei termini marcati come BLOCCATI nel glossario: usa \
sempre e solo la traduzione indicata, senza eccezioni.\n\
3. Usare le traduzioni del glossario non bloccato quando appropriato, per coerenza.\n\
4. Aggiornare il riassunto progressivo (summary) integrando i punti chiave di questa pagina. Se il \
summary risultante supererebbe circa {summary_token_limit} token, COMPRIMILO mantenendo solo trama, \
entita, terminologia e fatti utili alla coerenza futura.\n\
5. Proporre nuovi termini di glossario rilevanti apparsi in questa pagina (nomi propri, termini \
tecnici, espressioni ricorrenti) che non siano gia nel glossario.\n\n\
REGOLE DI OUTPUT (tassative):\n\
- Rispondi con UN SOLO oggetto JSON valido, senza testo prima o dopo, senza markdown, senza code fence.\n\
- Lo schema e ESATTAMENTE:\n\
  {{\n\
    \"translated_text\": string,\n\
    \"updated_summary\": string,\n\
    \"new_glossary_terms\": [ {{ \"source_term\": string, \"translation\": string, \"type\": \"nome proprio\" | \"tecnico\" | \"comune\", \"note\": string }} ]\n\
  }}\n\
- Non aggiungere altre chiavi. Non tradurre le chiavi JSON. \"note\" vuota = \"\"."
    )
}

/// User message with full context slots (research §4). `rolling_summary`,
/// `locked_terms` and `unlocked_terms` carry the percettore context (ticket 09;
/// ticket 08 passed them empty). When `compress` is `true` an explicit
/// recompression instruction is appended (EC05). **Contratto completo** (chiede
/// `translated_text`): il flusso live usa [`build_perceptor_update_user_prompt`]
/// (snello, STC-10); questo resta per i test e per eventuale riuso.
#[allow(dead_code)]
pub fn build_user_prompt(
    target_language: &str,
    page_text: &str,
    rolling_summary: &str,
    locked_terms: &str,
    unlocked_terms: &str,
    compress: bool,
) -> String {
    let summary = if rolling_summary.trim().is_empty() {
        "(nessuno: e la prima pagina)"
    } else {
        rolling_summary
    };
    let locked = if locked_terms.trim().is_empty() { "(nessuno)" } else { locked_terms };
    let unlocked = if unlocked_terms.trim().is_empty() { "(nessuno)" } else { unlocked_terms };
    let compress_note = if compress {
        format!("\n\n{COMPRESSION_INSTRUCTION}")
    } else {
        String::new()
    };

    format!(
        "LINGUA DI DESTINAZIONE: {target_language}\n\n\
RIASSUNTO PROGRESSIVO FINORA (contesto delle pagine precedenti):\n{summary}\n\n\
GLOSSARIO ATTUALE:\n\
Termini BLOCCATI (vincolo assoluto - usa esattamente questa traduzione):\n{locked}\n\
Termini suggeriti (coerenza consigliata, non vincolante):\n{unlocked}\n\n\
TESTO DELLA PAGINA DA TRADURRE:\n\"\"\"\n{page_text}\n\"\"\"\n\n\
Produci ora il JSON come da schema.{compress_note}"
    )
}

/// Assemble the system+user message pair for one page (or chunk), with the full
/// percettore context. `summary_token_limit` shapes the system prompt; the
/// `rolling_summary`/`locked_terms`/`unlocked_terms` slots and the `compress`
/// flag shape the user message. **Contratto completo**: il flusso live usa
/// [`build_perceptor_update_messages`] (snello, STC-10); questo resta per i test
/// e per eventuale riuso.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn build_messages(
    target_language: &str,
    page_text: &str,
    rolling_summary: &str,
    locked_terms: &str,
    unlocked_terms: &str,
    compress: bool,
    summary_token_limit: u32,
) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(build_system_prompt(summary_token_limit)),
        ChatMessage::user(build_user_prompt(
            target_language,
            page_text,
            rolling_summary,
            locked_terms,
            unlocked_terms,
            compress,
        )),
    ]
}

/// Assemble a `ChatRequest` for the given model and messages, with the §4.4
/// `response_format` and a low `temperature` for quality.
///
/// `max_tokens` is supplied by the caller (from the active provider config,
/// ticket 02) instead of being hardcoded to the whole context window: a small
/// local model (`n_ctx ~4096`) needs headroom for `prompt + reasoning + output`,
/// otherwise the server returns `finish_reason: length` with empty `content`
/// (see local-llm-empty-content diagnosis). Cloud providers pass a generous
/// value so long page translations are not truncated.
///
/// Deliberately does **not** send `provider.require_parameters` (bug #1): with
/// it, OpenRouter routes only to endpoints supporting *every* parameter in the
/// body, so a model that does not advertise `temperature` (e.g. a reasoning
/// model) gets a 404 "No endpoints found". Without it the router silently
/// ignores parameters the model does not support. `temperature` is kept (0.2)
/// for deterministic translations but is optional, so the model-agnostic
/// fallback ([`complete_with_fallback`]) can drop it if a model still rejects it.
///
/// **Contratto completo** (invia [`response_format`] con `translated_text`): il
/// flusso live usa [`build_perceptor_update_request`] (snello, STC-10); questo
/// resta per i test e per eventuale riuso.
#[allow(dead_code)]
pub fn build_request(model: &str, messages: Vec<ChatMessage>, max_tokens: u32) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages,
        temperature: Some(0.2),
        max_tokens,
        stream: false,
        response_format: Some(response_format()),
        provider: None,
    }
}

/// The correction message appended before the single retry (layer c, §4.4).
pub const CORRECTION_PROMPT: &str =
    "La tua risposta non era JSON valido conforme allo schema. Rispondi di nuovo con SOLO \
l'oggetto JSON, senza testo aggiuntivo, senza markdown, senza code fence.";

// --- Contratto translate-only per unità (STC-08, decisione D5) ---------------
//
// Sul percorso a budget la pagina è divisa in unità piccole (STC-02) tradotte una
// per una con un prompt MINIMALE: niente schema JSON ricco del percettore, niente
// riassunto/glossario da PRODURRE — solo la traduzione. Il grosso del contesto
// (glossario intero, contratto ricco) resta fuori da queste chiamate, che così
// stanno larghe dentro una finestra piccola e riducono il rischio EC08. Il
// riassunto e i nuovi termini vengono ricavati una sola volta per pagina dalla
// chiamata percettore separata (D6), che continua a usare `build_messages` +
// `build_request` + `response_format` come prima.

/// System prompt per una chiamata **translate-only** di una singola unità
/// (STC-08, D5). Minimale di proposito: traduci fedelmente, rispetta in modo
/// assoluto i termini bloccati, rispondi con il SOLO testo tradotto (nessun JSON,
/// nessun riassunto, nessun glossario da generare).
///
/// Vieta esplicitamente la **chain-of-thought** nel contenuto ("Thinking
/// Process"/"Reasoning"): sui modelli locali verbosi quel ragionamento consuma il
/// budget di output e tronca la traduzione a metà (unit-truncation-diagnosis,
/// ticket 11). La difesa a valle è [`parse_translation`], che strippa comunque un
/// eventuale blocco CoT iniziale.
pub fn build_translate_only_system_prompt() -> String {
    "Sei il motore di traduzione di translate-lector. Traduci fedelmente il testo fornito \
verso la lingua di destinazione indicata, mantenendo tono, significato e formattazione.\n\n\
Regole:\n\
- Rispetta in modo ASSOLUTO le traduzioni dei termini marcati come BLOCCATI: usa sempre e \
solo la traduzione indicata, senza eccezioni.\n\
- Usa i termini suggeriti quando appropriato, per coerenza.\n\
- Non riassumere, non omettere, non aggiungere commenti o spiegazioni.\n\
- VIETATO mostrare il ragionamento: NON produrre \"Thinking Process\", \"Reasoning\", \
catene di pensiero o passaggi intermedi. Vai DIRETTO alla traduzione.\n\
- Rispondi con IL SOLO testo tradotto, senza virgolette, senza markdown, senza JSON."
        .to_string()
}

/// User message per una chiamata **translate-only**: riassunto **read-only** come
/// contesto, glossario **già selezionato** per l'unità (locked-first, STC-03) e
/// il testo dell'unità da tradurre. Nessuna richiesta di aggiornare summary o
/// glossario: quello è compito della chiamata percettore per-pagina (D6).
pub fn build_translate_only_user_prompt(
    target_language: &str,
    unit_text: &str,
    rolling_summary: &str,
    locked_terms: &str,
    unlocked_terms: &str,
) -> String {
    let summary = if rolling_summary.trim().is_empty() {
        "(nessuno: e la prima pagina)"
    } else {
        rolling_summary
    };
    let locked = if locked_terms.trim().is_empty() { "(nessuno)" } else { locked_terms };
    let unlocked = if unlocked_terms.trim().is_empty() { "(nessuno)" } else { unlocked_terms };

    format!(
        "LINGUA DI DESTINAZIONE: {target_language}\n\n\
CONTESTO (riassunto delle pagine precedenti, solo lettura):\n{summary}\n\n\
GLOSSARIO RILEVANTE PER QUESTO TESTO:\n\
Termini BLOCCATI (vincolo assoluto - usa esattamente questa traduzione):\n{locked}\n\
Termini suggeriti (coerenza consigliata, non vincolante):\n{unlocked}\n\n\
TESTO DA TRADURRE:\n\"\"\"\n{unit_text}\n\"\"\"\n\n\
Rispondi con il solo testo tradotto."
    )
}

/// Coppia system+user per una chiamata translate-only di una singola unità.
pub fn build_translate_only_messages(
    target_language: &str,
    unit_text: &str,
    rolling_summary: &str,
    locked_terms: &str,
    unlocked_terms: &str,
) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(build_translate_only_system_prompt()),
        ChatMessage::user(build_translate_only_user_prompt(
            target_language,
            unit_text,
            rolling_summary,
            locked_terms,
            unlocked_terms,
        )),
    ]
}

/// [`ChatRequest`] per una chiamata translate-only: come [`build_request`] ma
/// **senza** il `response_format` ricco del percettore (D5). Il contratto è
/// "solo testo", con fallback JSON minimo in [`parse_translation`]. `temperature`
/// resta (0.2) ma è opzionale, così il fallback model-agnostico può rimuoverla se
/// un modello la rifiuta. `max_tokens` è piccolo (out_unit) sul percorso a budget
/// stretto — vedi `translate::translate_page`.
pub fn build_translate_only_request(
    model: &str,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages,
        temperature: Some(0.2),
        max_tokens,
        stream: false,
        // Contratto minimo: niente schema JSON ricco su queste chiamate (D5).
        response_format: None,
        provider: None,
    }
}

/// Contratto minimo di una risposta translate-only: solo il testo tradotto.
/// I campi extra sono ignorati (nessun `deny_unknown_fields`), così anche una
/// risposta che includa per errore l'intero JSON percettore viene estratta.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct TranslateUnitOutput {
    translated_text: String,
}

/// Estrae la traduzione dalla risposta di una chiamata **translate-only** (D5).
/// Contratto robusto e minimale: prova prima il JSON `{ "translated_text": ... }`
/// (diretto o dentro il primo blocco bilanciato), poi ripiega sul **testo puro**
/// (l'intero contenuto senza spazi di bordo). Riesce sempre finché il contenuto
/// non è vuoto — l'assenza di contenuto (incl. EC08 `finish_reason == "length"`)
/// è già intercettata a monte da [`ChatResponse::content`].
///
/// Sul ramo testo-puro rimuove un eventuale **blocco di ragionamento (CoT)**
/// iniziale che un modello verboso può emettere prima della traduzione
/// ([`strip_leading_cot`], unit-truncation-diagnosis ticket 11): difesa a valle
/// del divieto già presente nel system prompt translate-only.
pub fn parse_translation(content: &str) -> String {
    let trimmed = content.trim();
    // (a) JSON minimo diretto.
    if let Ok(v) = serde_json::from_str::<TranslateUnitOutput>(trimmed) {
        return v.translated_text;
    }
    // (b) primo blocco JSON bilanciato (strip di ```json / prosa attorno).
    if let Some(block) = extract_first_json_block(content) {
        if let Ok(v) = serde_json::from_str::<TranslateUnitOutput>(block) {
            return v.translated_text;
        }
    }
    // (c) testo puro: il contenuto è già la traduzione, meno un eventuale
    // blocco chain-of-thought iniziale.
    strip_leading_cot(trimmed)
}

/// Rimuove un blocco di **chain-of-thought (CoT)** in testa a una risposta
/// translate-only, restituendo la sola traduzione (ticket 11). È volutamente
/// **conservativo**: non deve MAI poter rimuovere testo di traduzione legittimo
/// (era il bug "mezza pagina mancante" che il ticket 11 corregge). Gestisce due
/// forme, entrambe non ambigue:
///
/// 1. Un blocco XML-ish `<think>…</think>`: è CoT di macchina inequivocabile,
///    quindi si strippa sempre in sicurezza.
/// 2. Una sezione etichettata: si strippa SOLO quando sono presenti **entrambi**
///    (a) un'intestazione CoT distintiva a inizio testo (frasi multi-parola come
///    "Thinking process:"/"Chain of thought:" — mai parole comuni come
///    "Reasoning"/"Ragionamento" che possono aprire legittimamente un paragrafo
///    di prosa) E (b) un marcatore di traduzione esplicito ("Traduzione:" /
///    "Translation:") a **inizio di una riga successiva**. In quel caso si taglia
///    fino al marcatore. Se il marcatore manca NON si strippa nulla (si ritorna
///    l'input invariato): niente fallback "prima riga vuota", perché è la via
///    non sicura che mangia il paragrafo iniziale.
fn strip_leading_cot(text: &str) -> String {
    let trimmed = text.trim();

    // (1) blocco `<think>…</think>` (chiusura case-insensitive): CoT di macchina
    // inequivocabile, sempre sicuro da rimuovere.
    if starts_with_ci(trimmed, "<think>") {
        if let Some(pos) = find_ascii_ci(trimmed, "</think>") {
            return trimmed[pos + "</think>".len()..].trim().to_string();
        }
    }

    // (2) sezione etichettata: strip SOLO se la prima riga è un'intestazione CoT
    // distintiva E c'è un marcatore di traduzione a inizio di una riga
    // successiva. Senza marcatore non si tocca nulla (mai il fallback "riga
    // vuota", che potrebbe scartare vera traduzione).
    let first_line = trimmed.lines().next().unwrap_or("");
    if is_cot_header_line(first_line) {
        for marker in ["traduzione:", "translation:"] {
            // Il marcatore deve iniziare una riga OLTRE la prima, così una
            // menzione della parola nel corpo del ragionamento non può essere
            // scelta come punto di taglio.
            if let Some(idx) = find_line_start_marker_after_first_line(trimmed, marker) {
                return trimmed[idx + marker.len()..].trim().to_string();
            }
        }
    }

    trimmed.to_string()
}

/// Whether `first_line` is a distinctive machine chain-of-thought header. Only
/// multi-word phrases a human paragraph would not open with are recognized
/// (never bare words like "reasoning"/"ragionamento"), and each must be
/// colon-terminated ("Thinking process: …") or be the entire line ("Thinking
/// process") — so a paragraph that merely *contains* such words is never taken
/// for CoT.
fn is_cot_header_line(first_line: &str) -> bool {
    const HEADERS: [&str; 6] = [
        "thinking process",
        "thought process",
        "reasoning process",
        "chain of thought",
        "processo di ragionamento",
        "catena di pensiero",
    ];
    let line = first_line.trim();
    HEADERS.iter().any(|h| {
        if !starts_with_ci(line, h) {
            return false;
        }
        // Header colon-terminated ("Thinking process:") or the whole line
        // ("Thinking process"); "Thinking processing" must NOT match.
        let rest = line[h.len()..].trim_start();
        rest.is_empty() || rest.starts_with(':')
    })
}

/// Byte offset of a translation `marker` that STARTS a line other than the
/// first. Searching only at line starts after the header line prevents a mention
/// of the word inside the reasoning body from being chosen as the split point.
fn find_line_start_marker_after_first_line(text: &str, marker: &str) -> Option<usize> {
    text.match_indices('\n')
        .map(|(i, _)| i + 1)
        .find(|&start| starts_with_ci(&text[start..], marker))
}

/// Whether `s` begins with the ASCII `prefix`, case-insensitively. Byte-wise
/// ASCII comparison so no allocation and no multi-byte offset surprises.
fn starts_with_ci(s: &str, prefix: &str) -> bool {
    let (sb, pb) = (s.as_bytes(), prefix.as_bytes());
    sb.len() >= pb.len() && sb[..pb.len()].iter().zip(pb).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Byte offset in `haystack` of the first case-insensitive occurrence of the
/// ASCII `needle`. ASCII-only comparison keeps the returned offset valid on the
/// original string (it lands on an ASCII byte = a char boundary), unlike a
/// `to_lowercase()` copy whose offsets can shift on multi-byte input.
fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() {
        return None;
    }
    (0..=hb.len() - nb.len())
        .find(|&i| hb[i..i + nb.len()].iter().zip(nb).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

// --- Contratto perceptor-update snello (STC-10, decisione D5) -----------------
//
// A fine pagina il percettore aggiorna SOLO il riassunto progressivo e il
// glossario, SENZA ri-tradurre la pagina: la traduzione è già stata prodotta
// dalle chiamate translate-only per unità. Il vecchio contratto completo
// (`build_messages`/`build_request`/`response_format` con `translated_text`)
// costringeva invece il modello a ri-tradurre l'intera pagina → maxi-output che
// sfonda una finestra piccola (EC08); e poiché quella chiamata falliva con `?`,
// l'app scartava la traduzione già fatta. Questo contratto snello toglie
// `translated_text` da schema e prompt: output piccolo, budget-safe. Il contratto
// completo resta disponibile (altri percorsi/test lo usano); qui si affianca.

/// Nome dello schema JSON inviato in `response_format` per il perceptor-update.
const PERCEPTOR_UPDATE_SCHEMA_NAME: &str = "perceptor_update";

/// Output **snello** del perceptor-update (STC-10): SOLO riassunto aggiornato e
/// nuovi termini di glossario, NESSUN `translated_text`. Nessun
/// `deny_unknown_fields`, così una risposta che includa per errore l'intero JSON
/// percettore (con `translated_text`) viene comunque estratta correttamente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerceptorUpdateOutput {
    pub updated_summary: String,
    pub new_glossary_terms: Vec<GlossaryTerm>,
}

/// Lo schema `response_format.json_schema.schema` per il perceptor-update snello:
/// SOLO `updated_summary` + `new_glossary_terms` (nessun `translated_text`), così
/// il modello non ri-traduce la pagina (STC-10). `strict: true` richiede
/// `additionalProperties: false` e ogni proprietà in `required`.
pub fn perceptor_update_response_format() -> serde_json::Value {
    serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": PERCEPTOR_UPDATE_SCHEMA_NAME,
            "strict": true,
            "schema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["updated_summary", "new_glossary_terms"],
                "properties": {
                    "updated_summary": { "type": "string" },
                    "new_glossary_terms": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["source_term", "translation", "type", "note"],
                            "properties": {
                                "source_term": { "type": "string" },
                                "translation": { "type": "string" },
                                "type": { "type": "string", "enum": ["nome proprio", "tecnico", "comune"] },
                                "note": { "type": "string" }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// System prompt del perceptor-update snello (STC-10): il modello aggiorna SOLO
/// summary + glossario e NON traduce la pagina (è già tradotta altrove). Mantiene
/// le stesse regole "JSON only" del percettore completo ma con lo schema ridotto.
/// Il limite del summary è iniettato dalle settings (EC05, §3.5).
pub fn build_perceptor_update_system_prompt(summary_token_limit: u32) -> String {
    format!(
        "Sei il percettore di translate-lector. Il testo di UNA pagina di un documento è GIA \
stato tradotto altrove: il tuo compito NON e tradurre, ma SOLO aggiornare il contesto del \
documento a partire dal testo della pagina.\n\n\
Devi:\n\
1. Aggiornare il riassunto progressivo (summary) integrando i punti chiave di questa pagina, \
mantenendo la coerenza col resto del documento. Se il summary risultante supererebbe circa \
{summary_token_limit} token, COMPRIMILO mantenendo solo trama, entita, terminologia e fatti utili \
alla coerenza futura.\n\
2. Proporre nuovi termini di glossario rilevanti apparsi in questa pagina (nomi propri, termini \
tecnici, espressioni ricorrenti) che non siano gia nel glossario.\n\n\
NON tradurre la pagina e NON restituire alcun testo tradotto: quello e gia stato prodotto.\n\n\
REGOLE DI OUTPUT (tassative):\n\
- Rispondi con UN SOLO oggetto JSON valido, senza testo prima o dopo, senza markdown, senza code fence.\n\
- Lo schema e ESATTAMENTE:\n\
  {{\n\
    \"updated_summary\": string,\n\
    \"new_glossary_terms\": [ {{ \"source_term\": string, \"translation\": string, \"type\": \"nome proprio\" | \"tecnico\" | \"comune\", \"note\": string }} ]\n\
  }}\n\
- Non aggiungere altre chiavi (in particolare NIENTE \"translated_text\"). Non tradurre le chiavi JSON. \"note\" vuota = \"\"."
    )
}

/// User message del perceptor-update snello: riassunto corrente, glossario
/// **gia noto** (per non riproporlo) e testo pagina come input per aggiornare il
/// contesto — con la nota esplicita che la pagina è già tradotta. Quando
/// `compress` è `true` appende l'istruzione di ricompressione (EC05).
pub fn build_perceptor_update_user_prompt(
    target_language: &str,
    page_text: &str,
    rolling_summary: &str,
    locked_terms: &str,
    unlocked_terms: &str,
    compress: bool,
) -> String {
    let summary = if rolling_summary.trim().is_empty() {
        "(nessuno: e la prima pagina)"
    } else {
        rolling_summary
    };
    let locked = if locked_terms.trim().is_empty() { "(nessuno)" } else { locked_terms };
    let unlocked = if unlocked_terms.trim().is_empty() { "(nessuno)" } else { unlocked_terms };
    let compress_note = if compress {
        format!("\n\n{COMPRESSION_INSTRUCTION}")
    } else {
        String::new()
    };

    format!(
        "LINGUA DI DESTINAZIONE: {target_language}\n\n\
RIASSUNTO PROGRESSIVO FINORA (contesto delle pagine precedenti):\n{summary}\n\n\
GLOSSARIO ATTUALE (termini gia noti, per non riproporli):\n\
Termini BLOCCATI (vincolo assoluto):\n{locked}\n\
Termini suggeriti:\n{unlocked}\n\n\
TESTO DELLA PAGINA (gia tradotto altrove, qui solo per aggiornare il contesto):\n\"\"\"\n{page_text}\n\"\"\"\n\n\
Produci ora il JSON con SOLO updated_summary e new_glossary_terms (NIENTE traduzione).{compress_note}"
    )
}

/// Coppia system+user per la chiamata perceptor-update snella (STC-10).
#[allow(clippy::too_many_arguments)]
pub fn build_perceptor_update_messages(
    target_language: &str,
    page_text: &str,
    rolling_summary: &str,
    locked_terms: &str,
    unlocked_terms: &str,
    compress: bool,
    summary_token_limit: u32,
) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(build_perceptor_update_system_prompt(summary_token_limit)),
        ChatMessage::user(build_perceptor_update_user_prompt(
            target_language,
            page_text,
            rolling_summary,
            locked_terms,
            unlocked_terms,
            compress,
        )),
    ]
}

/// [`ChatRequest`] per la chiamata perceptor-update snella: come [`build_request`]
/// ma con lo schema ridotto [`perceptor_update_response_format`] (niente
/// `translated_text`). `temperature` resta (0.2) ma opzionale, così il fallback
/// model-agnostico può rimuoverla; `provider` assente per non forzare il routing
/// (bug #1). `max_tokens` è quello di pagina: l'output ora è solo summary +
/// glossario, molto più piccolo di una ri-traduzione dell'intera pagina.
pub fn build_perceptor_update_request(
    model: &str,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages,
        temperature: Some(0.2),
        max_tokens,
        stream: false,
        response_format: Some(perceptor_update_response_format()),
        provider: None,
    }
}

/// Estrae [`PerceptorUpdateOutput`] dal `content` della risposta perceptor-update
/// con lo stesso fallback robusto del contratto completo: (a) deserializzazione
/// diretta, (b) primo blocco `{...}` bilanciato (strip di ```json / prosa). La
/// correzione (layer c) vive nel servizio di traduzione, che può richiamare il
/// client. Un `translated_text` eventualmente presente viene ignorato.
pub fn parse_perceptor_update(content: &str) -> Result<PerceptorUpdateOutput, String> {
    // (a) diretto.
    if let Ok(out) = serde_json::from_str::<PerceptorUpdateOutput>(content.trim()) {
        return Ok(out);
    }
    // (b) primo blocco bilanciato.
    if let Some(block) = extract_first_json_block(content) {
        if let Ok(out) = serde_json::from_str::<PerceptorUpdateOutput>(block) {
            return Ok(out);
        }
    }
    Err(format!(
        "impossibile estrarre JSON conforme (updated_summary + new_glossary_terms) \
         dal contenuto ({} char)",
        content.chars().count()
    ))
}

// --- Layered parsing (§4.4) --------------------------------------------------

/// Parse the model `content` into [`PerceptoreOutput`] using layers (a) direct
/// deserialize and (b) first balanced `{...}` block extraction. The correction
/// retry (layer c) lives in the translation service, which can call the client
/// again; this function is the pure, string-only part. **Contratto completo**: il
/// flusso live usa [`parse_perceptor_update`] (snello, STC-10); questo resta per
/// i test e per eventuale riuso.
#[allow(dead_code)]
pub fn parse_content(content: &str) -> Result<PerceptoreOutput, String> {
    // (a) direct.
    if let Ok(out) = serde_json::from_str::<PerceptoreOutput>(content.trim()) {
        return Ok(out);
    }
    // (b) extract the first balanced object (strips ```json fences, prose).
    if let Some(block) = extract_first_json_block(content) {
        if let Ok(out) = serde_json::from_str::<PerceptoreOutput>(block) {
            return Ok(out);
        }
    }
    Err(format!(
        "impossibile estrarre JSON conforme dallo schema dal contenuto ({} char)",
        content.chars().count()
    ))
}

/// Return the first balanced `{...}` slice of `s`, honouring braces that appear
/// inside JSON string literals (and their escapes). `None` if unbalanced/absent.
pub fn extract_first_json_block(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for i in start..bytes.len() {
        let c = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_JSON: &str = r#"{
        "translated_text": "Ciao mondo",
        "updated_summary": "Documento di prova.",
        "new_glossary_terms": [
            { "source_term": "hello", "translation": "ciao", "type": "comune", "note": "" }
        ]
    }"#;

    // --- Layer (a): direct deserialize --------------------------------------

    #[test]
    fn parse_content_deserializes_clean_json_directly() {
        let out = parse_content(VALID_JSON).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo");
        assert_eq!(out.new_glossary_terms.len(), 1);
        assert_eq!(out.new_glossary_terms[0].term_type, "comune");
    }

    #[test]
    fn perceptore_type_field_round_trips_as_json_key_type() {
        let out = parse_content(VALID_JSON).unwrap();
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["new_glossary_terms"][0]["type"], "comune");
        assert!(json["new_glossary_terms"][0].get("term_type").is_none());
    }

    // --- Layer (b): extract first balanced block ----------------------------

    #[test]
    fn parse_content_extracts_json_from_code_fence_and_prose() {
        let wrapped = format!(
            "Ecco la traduzione richiesta:\n```json\n{VALID_JSON}\n```\nSpero sia utile!"
        );
        let out = parse_content(&wrapped).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo");
    }

    #[test]
    fn extract_first_json_block_handles_nested_and_string_braces() {
        let s = r#"prefix {"a": {"b": "has } brace and \" quote"}} suffix {ignored}"#;
        let block = extract_first_json_block(s).unwrap();
        assert_eq!(block, r#"{"a": {"b": "has } brace and \" quote"}}"#);
    }

    #[test]
    fn extract_first_json_block_returns_none_when_unbalanced() {
        assert_eq!(extract_first_json_block("no json here"), None);
        assert_eq!(extract_first_json_block("{ unbalanced "), None);
    }

    // --- Malformed -> error --------------------------------------------------

    #[test]
    fn parse_content_errors_on_malformed_json() {
        assert!(parse_content("not json at all").is_err());
        // Valid JSON object but missing required fields fails the schema too.
        assert!(parse_content(r#"{"foo": "bar"}"#).is_err());
    }

    // --- Prompt builder ------------------------------------------------------

    #[test]
    fn build_user_prompt_minimal_has_language_text_and_empty_sections() {
        let p = build_user_prompt("italiano", "Hello world", "", "", "", false);
        assert!(p.contains("LINGUA DI DESTINAZIONE: italiano"));
        assert!(p.contains("Hello world"));
        assert!(p.contains("(nessuno: e la prima pagina)"));
        assert!(p.contains("(nessuno)"));
        assert!(!p.contains("RICOMPRIMILO"), "no compression note when compress=false");
    }

    #[test]
    fn build_user_prompt_renders_summary_and_locked_terms_as_absolute() {
        let p = build_user_prompt(
            "italiano",
            "The board met.",
            "Riassunto delle pagine precedenti.",
            "board => consiglio  [tecnico]",
            "CEO => amministratrice delegata  [tecnico]",
            false,
        );
        // The current summary is passed as context.
        assert!(p.contains("Riassunto delle pagine precedenti."));
        // Locked term appears under an absolute-constraint heading.
        assert!(p.contains("board => consiglio"));
        assert!(p.contains("Termini BLOCCATI (vincolo assoluto"));
        // Unlocked suggestions are present but under the non-binding heading.
        assert!(p.contains("CEO => amministratrice delegata"));
        assert!(p.contains("non vincolante"));
    }

    #[test]
    fn build_user_prompt_appends_compression_instruction_when_flagged() {
        let p = build_user_prompt("it", "x", "long summary", "", "", true);
        assert!(p.contains("RICOMPRIMILO"));
        assert!(p.contains(COMPRESSION_INSTRUCTION));
    }

    #[test]
    fn build_system_prompt_embeds_configured_summary_limit() {
        let p = build_system_prompt(850);
        assert!(p.contains("850 token"));
    }

    #[test]
    fn build_messages_pairs_system_and_user_with_context() {
        let msgs = build_messages("italiano", "Testo pagina", "sommario", "board => consiglio", "", false, 1000);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("Testo pagina"));
        assert!(msgs[1].content.contains("sommario"));
        assert!(msgs[1].content.contains("board => consiglio"));
    }

    // --- Token heuristic (research §3, EC05) --------------------------------

    #[test]
    fn est_tokens_uses_chars_over_ratio_rounding_up() {
        // 8 chars / 4.0 = 2 tokens exactly.
        assert_eq!(est_tokens("abcdefgh", 4.0), 2);
        // 9 chars / 4.0 = 2.25 -> ceil 3.
        assert_eq!(est_tokens("abcdefghi", 4.0), 3);
        // Non-positive ratio falls back to the default.
        assert_eq!(est_tokens("abcdefgh", 0.0), 2);
    }

    #[test]
    fn needs_compression_below_at_and_above_threshold() {
        // limit=100 -> threshold at 80 tokens -> 320 chars at ratio 4.
        let below = "x".repeat(316); // 79 tokens
        assert!(!needs_compression(&below, 100), "79 tokens is below 80% of 100");
        let at = "x".repeat(320); // 80 tokens
        assert!(needs_compression(&at, 100), "80 tokens hits the 80% threshold");
        let above = "x".repeat(400); // 100 tokens
        assert!(needs_compression(&above, 100), "100 tokens is above threshold");
    }

    #[test]
    fn calibrate_chars_per_token_from_usage_or_none() {
        assert_eq!(calibrate_chars_per_token(4000, 1000), Some(4.0));
        assert_eq!(calibrate_chars_per_token(0, 1000), None);
        assert_eq!(calibrate_chars_per_token(4000, 0), None);
    }

    #[test]
    fn build_request_sets_response_format_and_low_temperature() {
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096);
        assert_eq!(req.model, "openai/gpt-4o");
        assert!(!req.stream);
        let rf = req.response_format.clone().unwrap();
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(req.temperature, Some(0.2), "low temperature kept for quality");
    }

    // --- Ticket 02: max_tokens comes from the caller, not a hardcoded window --

    #[test]
    fn build_request_uses_the_provided_max_tokens_not_a_hardcoded_4096() {
        // The whole point of ticket 02: `max_tokens` is no longer pinned to the
        // (small local) context window — it flows in from the provider config so
        // a local model keeps room to actually emit `content`.
        let local = build_request("m", build_messages("it", "x", "", "", "", false, 1000), 2048);
        assert_eq!(local.max_tokens, 2048, "max_tokens is threaded from the caller");
        let cloud = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096);
        assert_eq!(cloud.max_tokens, 4096, "a generous cloud value passes through unchanged");
    }

    // --- STC-08: translate-only contract (minimal prompt + parse) -----------

    #[test]
    fn translate_only_system_prompt_is_minimal_text_only_no_json_schema() {
        let s = build_translate_only_system_prompt();
        // Deve chiedere il SOLO testo tradotto e vietare il JSON (contratto minimo).
        assert!(s.to_lowercase().contains("solo testo tradotto"));
        assert!(s.contains("senza JSON"));
        // Deve ribadire il vincolo assoluto sui termini bloccati (D5).
        assert!(s.contains("BLOCCATI"));
    }

    #[test]
    fn translate_only_user_prompt_carries_summary_selected_glossary_and_unit() {
        let p = build_translate_only_user_prompt(
            "italiano",
            "The board met.",
            "Riassunto delle pagine precedenti.",
            "board => consiglio  [tecnico]",
            "CEO => amministratrice delegata  [tecnico]",
        );
        assert!(p.contains("LINGUA DI DESTINAZIONE: italiano"));
        assert!(p.contains("Riassunto delle pagine precedenti."), "summary read-only in prompt");
        assert!(p.contains("Termini BLOCCATI (vincolo assoluto"));
        assert!(p.contains("board => consiglio"));
        assert!(p.contains("The board met."), "unit text present");
        // Nessuna richiesta di produrre summary/glossario o JSON.
        assert!(!p.contains("updated_summary"));
        assert!(!p.contains("new_glossary_terms"));
    }

    #[test]
    fn translate_only_request_omits_the_rich_response_format() {
        // Contratto minimo (D5): nessuno schema JSON ricco sulle chiamate di unità.
        let req = build_translate_only_request(
            "m",
            build_translate_only_messages("it", "x", "", "", ""),
            768,
        );
        assert!(req.response_format.is_none(), "translate-only sends no rich JSON schema");
        assert_eq!(req.max_tokens, 768, "small per-unit output cap threaded");
        assert_eq!(req.temperature, Some(0.2), "temperature kept but optional");
        // Non deve serializzare alcun response_format sul wire.
        let wire = serde_json::to_value(&req).unwrap();
        assert!(wire.get("response_format").is_none());
    }

    #[test]
    fn parse_translation_reads_plain_text() {
        assert_eq!(parse_translation("Ciao mondo"), "Ciao mondo");
        assert_eq!(parse_translation("  Ciao mondo  "), "Ciao mondo", "trims edge whitespace");
    }

    #[test]
    fn parse_translation_reads_tiny_json_and_ignores_extra_fields() {
        // JSON minimo diretto.
        assert_eq!(parse_translation(r#"{"translated_text":"Ciao mondo"}"#), "Ciao mondo");
        // JSON dentro un code fence + prosa attorno.
        assert_eq!(
            parse_translation("Ecco:\n```json\n{\"translated_text\":\"Ciao\"}\n```\nfine"),
            "Ciao"
        );
        // Un intero JSON percettore su una chiamata di unità: si estrae comunque
        // il solo translated_text (campi extra ignorati).
        let full = r#"{"translated_text":"Ciao","updated_summary":"s","new_glossary_terms":[]}"#;
        assert_eq!(parse_translation(full), "Ciao");
    }

    // --- Ticket 11: chain-of-thought suppression + stripping -----------------

    #[test]
    fn translate_only_system_prompt_forbids_chain_of_thought() {
        let s = build_translate_only_system_prompt();
        // Vieta esplicitamente la CoT nel contenuto (assorbe budget → troncamento).
        assert!(s.contains("Thinking Process"));
        assert!(s.contains("Reasoning"));
        assert!(s.to_lowercase().contains("solo testo tradotto"));
    }

    #[test]
    fn parse_translation_strips_labeled_cot_only_with_explicit_marker() {
        // Intestazione CoT distintiva ("Thinking process:") + marcatore di
        // traduzione a inizio di una riga successiva: esce SOLO la traduzione.
        let content = "Thinking process:\nThe user wants a faithful, natural rendering. \
I'll keep the tone.\n\nTraduzione: Ciao, mondo.";
        assert_eq!(parse_translation(content), "Ciao, mondo.");
    }

    #[test]
    fn parse_translation_keeps_cot_header_without_marker_unchanged() {
        // CONSERVATIVO: intestazione CoT ma NESSUN marcatore di traduzione
        // esplicito ⇒ non si può sapere dove finisce il ragionamento, quindi non
        // si strippa nulla (niente fallback "prima riga vuota" che mangiava il
        // paragrafo iniziale — il bug "mezza pagina mancante" del ticket 11).
        let content = "Thinking process:\nsome reasoning here.\n\nCiao, mondo.";
        assert_eq!(parse_translation(content), content);
    }

    #[test]
    fn parse_translation_leaves_paragraph_opening_with_reasoning_word_untouched() {
        // Una traduzione legittima il cui PRIMO paragrafo apre con una parola
        // comune di ragionamento (prosa di psicologia/filosofia) NON deve essere
        // scambiata per CoT: senza marcatore e senza <think> resta invariata,
        // anche se c'è una riga vuota più sotto.
        let content = "Ragionamento: il filosofo procede per gradi verso la sintesi.\n\n\
E così, senza fretta, l'argomentazione si chiude.";
        assert_eq!(parse_translation(content), content);
    }

    #[test]
    fn parse_translation_strips_think_xml_block() {
        let content = "<think>ragiono sul testo e sui termini bloccati</think>\nCiao mondo";
        assert_eq!(parse_translation(content), "Ciao mondo");
    }

    #[test]
    fn parse_translation_leaves_plain_text_untouched_even_mentioning_reasoning() {
        // "ragionamento" a metà frase non è un'intestazione CoT: nessuno strip.
        let plain = "Il ragionamento del filosofo era complesso ma affascinante.";
        assert_eq!(parse_translation(plain), plain);
    }

    // --- Ticket 11: truncation accessor (content_complete) -------------------

    #[test]
    fn content_complete_refuses_nonempty_length_as_truncated() {
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":"…con milioni o"},"finish_reason":"length"}]}"#,
        )
        .unwrap();
        // Il path pagina/percettore (content) accetta ancora il parziale (invariato).
        assert_eq!(resp.content().unwrap(), "…con milioni o");
        // Il path unità (content_complete) lo rifiuta come troncato.
        assert!(
            matches!(resp.content_complete().unwrap_err(), LlmError::OutputTruncated(_)),
            "non-empty + length must be OutputTruncated on the unit path"
        );
    }

    #[test]
    fn content_complete_accepts_nonempty_stop_and_missing_reason() {
        let stop: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":"Ciao mondo"},"finish_reason":"stop"}]}"#,
        )
        .unwrap();
        assert_eq!(stop.content_complete().unwrap(), "Ciao mondo");
        let no_reason: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":"Ciao"}}]}"#,
        )
        .unwrap();
        assert_eq!(no_reason.content_complete().unwrap(), "Ciao");
    }

    #[test]
    fn content_complete_keeps_empty_plus_length_as_ec08() {
        // Il caso vuoto+length resta EC08 esattamente come content() (non toccato).
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":null},"finish_reason":"length"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            resp.content_complete().unwrap_err(),
            LlmError::OutputBudgetExhausted(_)
        ));
    }

    #[test]
    fn output_truncated_is_permanent_not_degradable_with_ec08_message() {
        let e = LlmError::OutputTruncated("length".into());
        assert!(!e.is_transient(), "same budget would truncate again: not a backoff retry");
        assert!(!e.is_param_unsupported(), "not a param relaxation case");
        assert!(e.user_message().contains("EC08"), "actionable EC08 framing");
    }

    // --- STC-10: lean perceptor-update contract -----------------------------

    #[test]
    fn perceptor_update_response_format_requires_summary_and_glossary_not_translated_text() {
        let rf = perceptor_update_response_format();
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["strict"], true);
        let required = &rf["json_schema"]["schema"]["required"];
        // Only the two lean fields are required — never translated_text.
        assert_eq!(required, &serde_json::json!(["updated_summary", "new_glossary_terms"]));
        let props = &rf["json_schema"]["schema"]["properties"];
        assert!(props.get("translated_text").is_none(), "lean schema must not expose translated_text");
        assert!(props.get("updated_summary").is_some());
        assert!(props.get("new_glossary_terms").is_some());
    }

    #[test]
    fn perceptor_update_system_prompt_does_not_ask_to_translate() {
        let s = build_perceptor_update_system_prompt(700);
        // Explicitly forbids re-translation and the translated_text field.
        assert!(s.contains("NON e tradurre") || s.contains("NON tradurre"));
        assert!(s.contains("NIENTE \"translated_text\""));
        // Still owns the summary + glossary duties and the configured limit.
        assert!(s.contains("700 token"));
        assert!(s.contains("updated_summary"));
        assert!(s.contains("new_glossary_terms"));
        assert!(!s.contains("\"translated_text\": string"), "no translated_text in the schema block");
    }

    #[test]
    fn perceptor_update_user_prompt_appends_compression_only_when_flagged() {
        let plain = build_perceptor_update_user_prompt("it", "Testo pagina", "sommario", "", "", false);
        assert!(plain.contains("Testo pagina"));
        assert!(plain.contains("sommario"));
        assert!(plain.contains("NIENTE traduzione"));
        assert!(!plain.contains("RICOMPRIMILO"), "no compression note when compress=false");

        let compressed = build_perceptor_update_user_prompt("it", "x", "long", "", "", true);
        assert!(compressed.contains(COMPRESSION_INSTRUCTION), "EC05 compression reused when flagged");
    }

    #[test]
    fn build_perceptor_update_request_carries_the_lean_schema() {
        let req = build_perceptor_update_request(
            "m",
            build_perceptor_update_messages("it", "x", "", "", "", false, 1000),
            4096,
        );
        let rf = req.response_format.clone().expect("perceptor-update sends a response_format");
        // It is the LEAN schema, not the full one (no translated_text).
        assert_eq!(rf["json_schema"]["name"], "perceptor_update");
        assert!(rf["json_schema"]["schema"]["properties"].get("translated_text").is_none());
        assert_eq!(req.temperature, Some(0.2));
        assert!(req.provider.is_none(), "no provider routing by default (bug #1)");
    }

    #[test]
    fn parse_perceptor_update_reads_lean_json_and_ignores_translated_text() {
        // Pure lean JSON.
        let lean = r#"{"updated_summary":"riassunto","new_glossary_terms":[]}"#;
        let out = parse_perceptor_update(lean).unwrap();
        assert_eq!(out.updated_summary, "riassunto");
        assert!(out.new_glossary_terms.is_empty());

        // A full PerceptoreOutput JSON (with translated_text) still parses: the
        // extra field is ignored, so old fixtures keep working.
        let full = r#"{"translated_text":"ignored","updated_summary":"s","new_glossary_terms":[
            {"source_term":"hello","translation":"ciao","type":"comune","note":""}
        ]}"#;
        let out = parse_perceptor_update(full).unwrap();
        assert_eq!(out.updated_summary, "s");
        assert_eq!(out.new_glossary_terms.len(), 1);
        assert_eq!(out.new_glossary_terms[0].source_term, "hello");

        // Inside a code fence + prose.
        let fenced = format!("Ecco:\n```json\n{lean}\n```\nfine");
        assert_eq!(parse_perceptor_update(&fenced).unwrap().updated_summary, "riassunto");
    }

    #[test]
    fn parse_perceptor_update_errors_on_malformed_or_incomplete() {
        assert!(parse_perceptor_update("not json at all").is_err());
        // Missing the required new_glossary_terms field.
        assert!(parse_perceptor_update(r#"{"updated_summary":"s"}"#).is_err());
    }

    // --- Bug #1: default body must not force provider routing --------------

    #[test]
    fn build_request_default_body_has_no_require_parameters() {
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096);
        // The default body must not send provider.require_parameters, which
        // would 404 on models that don't advertise `temperature` (bug #1).
        assert!(req.provider.is_none(), "no provider block by default");
        let wire = serde_json::to_value(&req).unwrap();
        assert!(wire.get("provider").is_none(), "provider omitted from the wire");
        assert!(
            wire.to_string().find("require_parameters").is_none(),
            "serialized body must not contain require_parameters"
        );
    }

    // --- Bug #1: unsupported-param classification + request degradation -----

    #[test]
    fn unsupported_params_error_matches_no_endpoints_404_and_bad_param_400() {
        assert!(is_unsupported_params_error(
            404,
            r#"{"error":{"message":"No endpoints found that can handle the requested parameters."}}"#
        ));
        assert!(is_unsupported_params_error(400, "Parameter temperature is not supported"));
        assert!(is_unsupported_params_error(400, "unsupported parameter: response_format"));
        // Unrelated statuses / bodies are not degradable.
        assert!(!is_unsupported_params_error(404, "model not found"));
        assert!(!is_unsupported_params_error(500, "no endpoints found"));
        assert!(!is_unsupported_params_error(429, "rate limited"));
    }

    #[test]
    fn unsupported_params_error_rejects_non_parameter_400s() {
        // Content-policy 400 mentions "unsupported" but has no parameter cue: a
        // downgrade retry would be wasted, and the message would mislead. NOT
        // degradable.
        assert!(!is_unsupported_params_error(
            400,
            r#"{"error":{"message":"Content policy violation: this prompt contains unsupported content."}}"#
        ));
        // "unsupported media type" — no parameter cue.
        assert!(!is_unsupported_params_error(400, "unsupported media type"));
        // Invalid model is terminal, not a parameter relaxation.
        assert!(!is_unsupported_params_error(
            400,
            r#"{"error":{"message":"not a valid model"}}"#
        ));
        // A 404 that is not the routing signature is terminal.
        assert!(!is_unsupported_params_error(
            404,
            r#"{"error":{"message":"model not found"}}"#
        ));
    }

    #[test]
    fn unsupported_params_error_catches_previously_missed_parameter_phrasing() {
        // Previously a false negative: real phrasing "is not a supported
        // parameter" must now classify as a degradable parameter rejection.
        assert!(is_unsupported_params_error(
            400,
            r#"{"error":{"message":"temperature is not a supported parameter for this model"}}"#
        ));
        // Other genuine phrasings the fallback should recover from.
        assert!(is_unsupported_params_error(
            400,
            r#"{"error":{"message":"provider does not support structured_outputs"}}"#
        ));
        assert!(is_unsupported_params_error(
            400,
            r#"{"error":{"message":"this model doesn't accept the response_format parameter"}}"#
        ));
    }

    #[test]
    fn degrade_strips_provider_then_response_format_then_temperature_then_stops() {
        let mut req = build_request("m", build_messages("it", "x", "", "", "", false, 1000), 4096);
        req.provider = Some(serde_json::json!({ "require_parameters": true }));
        // provider first
        assert!(req.degrade());
        assert!(req.provider.is_none());
        assert!(req.response_format.is_some());
        // then response_format
        assert!(req.degrade());
        assert!(req.response_format.is_none());
        assert!(req.temperature.is_some());
        // then temperature
        assert!(req.degrade());
        assert!(req.temperature.is_none());
        // nothing left to relax -> bounded
        assert!(!req.degrade());
    }

    #[test]
    fn complete_with_fallback_recovers_after_a_downgraded_retry() {
        let inner = SeqClient::new(vec![
            Err(LlmError::UnsupportedParams(
                "404 Not Found: No endpoints found that can handle the requested parameters".into(),
            )),
            Ok(ok_resp()),
        ]);
        let (resp, working) = complete_with_fallback(&inner, &a_request()).unwrap();
        assert_eq!(resp.content().unwrap(), "ok");
        assert_eq!(inner.calls.get(), 2, "one full attempt + one downgraded retry");
        // The returned working body is the degraded one (response_format was the
        // first strippable param, provider being None by default).
        assert!(
            working.response_format.is_none(),
            "working body reflects the degradation that succeeded"
        );
    }

    #[test]
    fn complete_with_fallback_returns_the_unchanged_request_on_the_happy_path() {
        let inner = SeqClient::new(vec![Ok(ok_resp())]);
        let req = a_request();
        let (_resp, working) = complete_with_fallback(&inner, &req).unwrap();
        assert_eq!(inner.calls.get(), 1, "no degradation on the happy path");
        // Identical body: same optional params as the input.
        assert_eq!(working.response_format.is_some(), req.response_format.is_some());
        assert_eq!(working.temperature, req.temperature);
        assert!(working.provider.is_none());
    }

    #[test]
    fn complete_with_fallback_is_bounded_and_does_not_loop_forever() {
        // Always degradable: it must stop after the full body + 3 strips.
        let inner = SeqClient::new(vec![
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
        ]);
        // A body carrying all three optional params exercises every strip.
        let mut req = a_request();
        req.provider = Some(serde_json::json!({ "require_parameters": true }));
        let err = complete_with_fallback(&inner, &req).unwrap_err();
        assert!(err.is_param_unsupported());
        assert_eq!(inner.calls.get(), 4, "full body + 3 degraded retries, then stop");
    }

    #[test]
    fn complete_with_fallback_passes_through_non_degradable_errors() {
        let inner = SeqClient::new(vec![Err(LlmError::MissingApiKey)]);
        let err = complete_with_fallback(&inner, &a_request()).unwrap_err();
        assert_eq!(err, LlmError::MissingApiKey);
        assert_eq!(inner.calls.get(), 1, "no retry on a non-degradable error");
    }

    // --- Bug #2: reasoning-style response with null/absent content ----------

    #[test]
    fn response_with_null_content_deserializes_and_is_handled_gracefully() {
        // Reasoning-style: content is null. Must NOT fail deserialization.
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":null}}],"usage":{"total_tokens":10}}"#,
        )
        .expect("null content must deserialize");
        // content() returns a clear error, not a panic/decode failure.
        let err = resp.content().unwrap_err();
        assert!(matches!(err, LlmError::Http(_)));
        assert_eq!(resp.usage.unwrap().total_tokens, 10);
    }

    #[test]
    fn response_omitting_content_deserializes_and_is_handled_gracefully() {
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant"}}]}"#,
        )
        .expect("absent content must deserialize");
        assert!(resp.content().is_err(), "missing content is a clear error");
    }

    #[test]
    fn response_blank_content_is_a_clear_error() {
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":"   "}}]}"#,
        )
        .unwrap();
        assert!(resp.content().is_err(), "blank content is treated as empty");
    }

    // --- Ticket 03: empty content + finish_reason == "length" ----------------

    #[test]
    fn empty_content_with_finish_reason_length_is_output_budget_exhausted() {
        // A reasoning model burned the whole completion budget: the server
        // returns finish_reason "length" with empty/null content. This is NOT
        // the generic empty-content error — it gets a dedicated, actionable
        // variant + message (change model / reduce text / raise n_ctx).
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":null},"finish_reason":"length"}],"usage":{"completion_tokens":2048,"total_tokens":4096}}"#,
        )
        .expect("length+null content must deserialize");
        let err = resp.content().unwrap_err();
        assert!(
            matches!(err, LlmError::OutputBudgetExhausted(_)),
            "length + empty content -> dedicated variant, got {err:?}"
        );
        let msg = err.user_message();
        assert!(msg.contains("EC08"), "carries the EC08 marker for the frontend");
        assert!(msg.contains("budget"), "actionable: mentions the token budget");
        assert!(msg.contains("n_ctx"), "actionable: suggests raising the context");
    }

    #[test]
    fn empty_string_content_with_finish_reason_length_is_output_budget_exhausted() {
        // Same case but with an empty string (not null) content.
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":""},"finish_reason":"length"}]}"#,
        )
        .unwrap();
        assert!(matches!(resp.content().unwrap_err(), LlmError::OutputBudgetExhausted(_)));
    }

    #[test]
    fn empty_content_with_finish_reason_stop_keeps_the_generic_error() {
        // finish_reason "stop" (or any non-length reason) with empty content is
        // NOT a budget-exhaustion case: keep the existing generic Http error so
        // we don't mislead the user with a budget message.
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":null},"finish_reason":"stop"}]}"#,
        )
        .unwrap();
        assert!(
            matches!(resp.content().unwrap_err(), LlmError::Http(_)),
            "stop + empty content stays a generic Http error"
        );
    }

    #[test]
    fn output_budget_exhausted_is_permanent_and_not_degradable() {
        // A retry with the same budget would likely burn it again, and relaxing
        // optional params does not add budget: surface immediately (no backoff
        // retry, no param-relaxation fallback).
        let e = LlmError::OutputBudgetExhausted("length".into());
        assert!(!e.is_transient(), "no backoff retry: the budget won't grow on retry");
        assert!(!e.is_param_unsupported(), "not a param relaxation case");
    }

    // --- Errors --------------------------------------------------------------

    #[test]
    fn missing_api_key_maps_to_ec03_message() {
        let msg = LlmError::MissingApiKey.user_message();
        assert!(msg.contains("EC03"));
        assert!(msg.contains("⚙️"));
    }

    // --- Error classification (NFR06/EC07/EC02) ------------------------------

    #[test]
    fn transient_errors_are_classified_permanent_ones_are_not() {
        assert!(LlmError::Timeout("t".into()).is_transient());
        assert!(LlmError::ServerError("500".into()).is_transient());
        assert!(LlmError::RateLimited("429".into()).is_transient());
        assert!(LlmError::Offline("down".into()).is_transient());

        assert!(!LlmError::MissingApiKey.is_transient(), "EC03 is permanent");
        assert!(!LlmError::Http("400".into()).is_transient());
        assert!(!LlmError::ParseFailed("x".into()).is_transient());
        assert!(!LlmError::Storage("x".into()).is_transient());
        // Degradable, but not transient: no backoff retry, a param-relaxation
        // retry instead.
        assert!(!LlmError::UnsupportedParams("404".into()).is_transient());
        assert!(LlmError::UnsupportedParams("404".into()).is_param_unsupported());
        assert!(!LlmError::Http("404".into()).is_param_unsupported());
    }

    #[test]
    fn rate_limit_and_offline_carry_their_edge_case_codes() {
        assert!(LlmError::RateLimited("".into()).user_message().contains("EC07"));
        assert!(LlmError::Offline("".into()).user_message().contains("EC02"));
    }

    #[test]
    fn cancelled_is_neither_transient_nor_param_degradable_and_has_a_low_key_message() {
        // Ticket 06: a stale/superseded job stopping at a unit boundary is not a
        // real failure — it must never be retried (no implicit retry from the
        // cancellation path) and its message must stay low-key, not alarming.
        let e = LlmError::Cancelled;
        assert!(!e.is_transient(), "cancellation is not worth retrying");
        assert!(!e.is_param_unsupported(), "not a param-relaxation case");
        let msg = e.user_message();
        assert!(!msg.contains("Errore"), "low-key message, not framed as a real error");
        assert!(!msg.to_uppercase().contains("EC0"), "no alarming error code for a routine cancellation");
    }

    // --- Local server unreachable (ticket 09, D3/D4/D7, EC02 local case) ------

    #[test]
    fn connection_error_to_a_local_server_maps_to_unreachable_with_base_url() {
        // A connection refused (is_connect) to a loopback endpoint means the
        // local server is simply down: a dedicated, fail-fast Unreachable that
        // names the base_url — not a generic Http nor a cloud-y Offline.
        let base = "http://localhost:8888/v1/chat/completions";
        let err = classify_send_error(
            /* is_timeout = */ false,
            /* is_connect = */ true,
            base,
            "connection refused".into(),
        );
        assert_eq!(err, LlmError::Unreachable(base.to_string()));
        let msg = err.user_message();
        assert!(msg.contains(base), "message names the configured base_url");
        assert!(msg.contains("Server locale non raggiungibile"), "clear local-down copy");
        assert!(msg.contains("⚙️"), "actionable: points at settings");
    }

    #[test]
    fn connection_error_to_a_remote_endpoint_stays_offline_ec02() {
        // The same connection failure to a REMOTE endpoint is the EC02 offline
        // case (no internet), not the local-server-down case.
        let err = classify_send_error(
            false,
            true,
            OPENROUTER_URL,
            "dns error".into(),
        );
        assert_eq!(err, LlmError::Offline("dns error".into()));
        assert!(err.user_message().contains("EC02"));
    }

    #[test]
    fn timeout_and_generic_send_errors_are_classified_independently_of_host() {
        // A timeout is always classified as Timeout regardless of host (the
        // *message* is what differs by host — ticket 13, see the dedicated
        // tests below); a non-timeout/non-connect failure stays Http either way.
        assert!(matches!(
            classify_send_error(true, false, "http://localhost:8888/v1/chat/completions", "t".into()),
            LlmError::Timeout(_)
        ));
        // The generic/remote branch now owns its final message (including the
        // prefix) at construction time too, same as the local branch — so
        // `user_message` never has to guess whether a prefix is still needed
        // (ticket 13 review: avoids double-prefixing the local message).
        assert_eq!(
            classify_send_error(true, false, OPENROUTER_URL, "t".into()),
            LlmError::Timeout("Errore di rete/servizio LLM (timeout): t".into())
        );
        assert_eq!(
            classify_send_error(false, false, "http://localhost:8888/v1/chat/completions", "x".into()),
            LlmError::Http("x".into())
        );
    }

    #[test]
    fn timeout_on_a_local_url_gets_an_actionable_local_message() {
        // Ticket 13: a timeout against a local server means the local inference
        // is too slow (or the server dropped the connection) — the message must
        // point the user at raising the timeout / a faster model / smaller
        // n_ctx, not the generic cloud-y copy.
        let err = classify_send_error(true, false, "http://localhost:8888/v1/chat/completions", "t".into());
        let msg = err.user_message();
        assert!(
            msg.contains("server locale") || msg.contains("Server locale"),
            "names the local server: {msg}"
        );
        assert!(msg.contains("⚙️"), "points at settings to raise the timeout: {msg}");
        assert!(
            !msg.contains("Errore di rete/servizio LLM (timeout): Il server locale"),
            "must not double-prefix the already-actionable local message: {msg}"
        );
    }

    #[test]
    fn timeout_on_a_remote_url_keeps_the_generic_message() {
        let err = classify_send_error(true, false, OPENROUTER_URL, "t".into());
        let msg = err.user_message();
        assert_eq!(msg, "Errore di rete/servizio LLM (timeout): t");
    }

    #[test]
    fn unreachable_is_fail_fast_not_transient_and_not_degradable() {
        let e = LlmError::Unreachable("http://localhost:8888/v1/chat/completions".into());
        // Must NOT be retried with backoff (would spin on a down server), and is
        // not a param-relaxation case either — it surfaces immediately.
        assert!(!e.is_transient(), "a down local server fails fast, no backoff retry");
        assert!(!e.is_param_unsupported());
    }

    #[test]
    fn retry_layer_returns_unreachable_immediately_without_spinning() {
        // A down local server (Unreachable) must fail fast: the RetryingChatClient
        // returns it on the first attempt, never looping/hanging (ticket 09).
        let inner = SeqClient::new(vec![Err(LlmError::Unreachable(
            "http://localhost:8888/v1/chat/completions".into(),
        ))]);
        let client = RetryingChatClient::new(&inner, RetryPolicy::no_delay(5));
        let err = client.complete(&a_request()).unwrap_err();
        assert!(matches!(err, LlmError::Unreachable(_)));
        assert_eq!(inner.calls.get(), 1, "no retry on a fail-fast Unreachable");
    }

    #[test]
    fn complete_with_fallback_passes_unreachable_through_without_cloud_fallback() {
        // D4: an unreachable local server must NOT trigger any fallback — the
        // error surfaces unchanged (no second, cloud-bound attempt).
        let inner = SeqClient::new(vec![Err(LlmError::Unreachable("http://127.0.0.1:8080".into()))]);
        let err = complete_with_fallback(&inner, &a_request()).unwrap_err();
        assert!(matches!(err, LlmError::Unreachable(_)));
        assert_eq!(inner.calls.get(), 1, "no degrade/fallback attempt on Unreachable");
    }

    #[test]
    fn authority_host_port_isolates_the_authority_slice() {
        // Shared by is_local_url + port_from_base_url: scheme-strip, path-split,
        // userinfo-strip. IPv6 brackets are preserved for the callers to unwrap.
        assert_eq!(authority_host_port("http://127.0.0.1:8080/v1/chat/completions"), "127.0.0.1:8080");
        assert_eq!(authority_host_port("http://localhost/v1"), "localhost");
        assert_eq!(authority_host_port("http://[::1]:1234/v1"), "[::1]:1234");
        // Userinfo is dropped, so the host (not the credentials) is what remains.
        assert_eq!(authority_host_port("http://user:pass@host:9000/v1"), "host:9000");
        // No scheme: the whole string is treated as the authority+path.
        assert_eq!(authority_host_port("localhost:8080/v1"), "localhost:8080");
    }

    #[test]
    fn is_local_url_recognises_loopback_hosts_only() {
        assert!(is_local_url("http://localhost:8888/v1/chat/completions"));
        assert!(is_local_url("http://127.0.0.1:8080/v1/chat/completions"));
        assert!(is_local_url("http://127.5.6.7:1234/v1"));
        assert!(is_local_url("http://0.0.0.0:11434/v1/chat/completions"));
        assert!(is_local_url("http://[::1]:1234/v1/chat/completions"));
        // Remote hosts are not local.
        assert!(!is_local_url(OPENROUTER_URL));
        assert!(!is_local_url("https://api.example.com/v1/chat/completions"));
        assert!(!is_local_url("http://192.168.1.10:8888/v1")); // LAN IP, not loopback
    }

    #[test]
    fn port_from_base_url_reads_the_explicit_port() {
        assert_eq!(port_from_base_url("http://127.0.0.1:8080/v1/chat/completions"), Some(8080));
        assert_eq!(port_from_base_url("http://localhost:8888/v1"), Some(8888));
        assert_eq!(port_from_base_url("http://[::1]:1234/v1/chat/completions"), Some(1234));
        assert_eq!(port_from_base_url("https://host:443/"), Some(443));
    }

    #[test]
    fn port_from_base_url_is_none_without_an_explicit_port() {
        // No scheme default is inferred: the spawner must decide the fallback.
        assert_eq!(port_from_base_url("http://localhost/v1/chat/completions"), None);
        assert_eq!(port_from_base_url("https://openrouter.ai/api/v1/chat/completions"), None);
        // Out-of-range / non-numeric ports do not parse.
        assert_eq!(port_from_base_url("http://localhost:99999/v1"), None);
        assert_eq!(port_from_base_url("http://localhost:abc/v1"), None);
    }

    #[test]
    fn retry_policy_for_base_url_disables_timeout_retry_only_for_local() {
        // Ticket 13 review: the L4 rule (no retry-on-timeout for a local
        // provider) now lives next to RetryPolicy/is_local_url so a future
        // second call site (e.g. ticket 06 prefetch) can reuse it instead of
        // re-deriving the if/else and risking dropping the rule.
        let local = RetryPolicy::for_base_url("http://localhost:8888/v1/chat/completions");
        assert!(!local.retry_on_timeout, "local providers must not retry on timeout");
        assert_eq!(local.max_attempts, RetryPolicy::default().max_attempts);
        assert_eq!(local.base_delay, RetryPolicy::default().base_delay);

        let remote = RetryPolicy::for_base_url(OPENROUTER_URL);
        assert_eq!(remote, RetryPolicy::default());
    }

    #[test]
    fn models_probe_url_maps_chat_completions_to_models() {
        assert_eq!(
            models_probe_url("http://localhost:8888/v1/chat/completions"),
            "http://localhost:8888/v1/models"
        );
        assert_eq!(
            models_probe_url("http://127.0.0.1:8080/v1/chat/completions/"),
            "http://127.0.0.1:8080/v1/models"
        );
        // A non-standard URL is probed as-is (connecting is what matters).
        assert_eq!(models_probe_url("http://localhost:1234/health"), "http://localhost:1234/health");
    }

    #[test]
    fn probe_reachable_is_false_when_nothing_is_listening() {
        // Bind then drop to obtain an almost-certainly-free loopback port; a probe
        // there gets connection refused fast → false (AC: "false con server spento").
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let base = format!("http://127.0.0.1:{port}/v1/chat/completions");
        assert!(!probe_reachable(&base), "a closed port is not reachable");
    }

    #[test]
    fn probe_reachable_is_true_when_the_endpoint_answers() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                // Any HTTP status proves the endpoint is up (even 404).
                let body = r#"{"data":[]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        let base = format!("http://{addr}/v1/chat/completions");
        assert!(probe_reachable(&base), "a listening endpoint is reachable");
        handle.join().unwrap();
    }

    #[test]
    fn backoff_is_exponential_from_the_base_delay() {
        let p = RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(100),
            retry_on_timeout: true,
        };
        assert_eq!(p.backoff(0), Duration::from_millis(100));
        assert_eq!(p.backoff(1), Duration::from_millis(200));
        assert_eq!(p.backoff(2), Duration::from_millis(400));
    }

    // --- Retry layer (NFR06) -------------------------------------------------

    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::time::Duration;

    /// A `ChatClient` that pops canned results and counts calls. No network.
    struct SeqClient {
        responses: RefCell<VecDeque<Result<ChatResponse, LlmError>>>,
        calls: Cell<usize>,
    }
    impl SeqClient {
        fn new(responses: Vec<Result<ChatResponse, LlmError>>) -> Self {
            Self { responses: RefCell::new(responses.into_iter().collect()), calls: Cell::new(0) }
        }
    }
    impl ChatClient for SeqClient {
        fn complete(&self, _req: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.calls.set(self.calls.get() + 1);
            self.responses.borrow_mut().pop_front().expect("no more canned responses")
        }
    }

    fn ok_resp() -> ChatResponse {
        serde_json::from_value(serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": "ok" } }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 1 }
        }))
        .unwrap()
    }

    fn a_request() -> ChatRequest {
        build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096)
    }

    #[test]
    fn retry_succeeds_after_transient_failures() {
        let inner = SeqClient::new(vec![
            Err(LlmError::ServerError("500".into())),
            Err(LlmError::RateLimited("429".into())),
            Ok(ok_resp()),
        ]);
        let client = RetryingChatClient::new(&inner, RetryPolicy::no_delay(3));

        let resp = client.complete(&a_request()).unwrap();
        assert_eq!(resp.content().unwrap(), "ok");
        assert_eq!(inner.calls.get(), 3, "retried twice then succeeded");
    }

    #[test]
    fn retry_gives_up_after_the_cap_with_a_typed_error() {
        let inner = SeqClient::new(vec![
            Err(LlmError::Timeout("t1".into())),
            Err(LlmError::Timeout("t2".into())),
            Err(LlmError::Timeout("t3".into())),
        ]);
        let client = RetryingChatClient::new(&inner, RetryPolicy::no_delay(3));

        let err = client.complete(&a_request()).unwrap_err();
        assert!(matches!(err, LlmError::Timeout(_)), "typed error surfaced");
        assert_eq!(inner.calls.get(), 3, "capped at max_attempts");
    }

    #[test]
    fn retry_on_timeout_false_gives_up_immediately_on_timeout_but_not_on_other_transients() {
        // Ticket 13 / decision L4: a local provider disables retry-on-timeout.
        // A Timeout must NOT be retried (1 call only)...
        let inner = SeqClient::new(vec![Err(LlmError::Timeout("t1".into()))]);
        let policy = RetryPolicy { retry_on_timeout: false, ..RetryPolicy::no_delay(3) };
        let client = RetryingChatClient::new(&inner, policy);
        let err = client.complete(&a_request()).unwrap_err();
        assert!(matches!(err, LlmError::Timeout(_)));
        assert_eq!(inner.calls.get(), 1, "timeout is not retried when retry_on_timeout is false");

        // ...but ServerError/RateLimited/Offline still retry up to max_attempts
        // with the very same policy.
        let inner = SeqClient::new(vec![
            Err(LlmError::ServerError("500".into())),
            Err(LlmError::RateLimited("429".into())),
            Err(LlmError::Offline("net".into())),
        ]);
        let policy = RetryPolicy { retry_on_timeout: false, ..RetryPolicy::no_delay(3) };
        let client = RetryingChatClient::new(&inner, policy);
        let err = client.complete(&a_request()).unwrap_err();
        assert!(matches!(err, LlmError::Offline(_)));
        assert_eq!(inner.calls.get(), 3, "other transient errors keep retrying to max_attempts");
    }

    #[test]
    fn retry_does_not_retry_permanent_errors_ec03() {
        let inner = SeqClient::new(vec![Err(LlmError::MissingApiKey)]);
        let client = RetryingChatClient::new(&inner, RetryPolicy::no_delay(5));

        let err = client.complete(&a_request()).unwrap_err();
        assert_eq!(err, LlmError::MissingApiKey);
        assert_eq!(inner.calls.get(), 1, "401/EC03 is not retried");
    }

    // --- Real client key guard (no network) ---------------------------------

    #[test]
    fn chat_completions_client_with_empty_key_errors_before_network() {
        // Per D5 the key is always mandatory: a blank key trips EC03 before any
        // network call, regardless of the (arbitrary) base-URL configured.
        let client = ChatCompletionsClient::new(
            "http://127.0.0.1:9/v1/chat/completions",
            "   ",
            /* send_openrouter_headers = */ true,
            30,
        );
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096);
        assert_eq!(client.complete(&req), Err(LlmError::MissingApiKey));
    }

    #[test]
    fn chat_completions_client_honors_the_configured_timeout() {
        // Ticket 13: prove the configured timeout is *actually* applied by the
        // built reqwest client, not just plumbed through unused. A TcpListener
        // that accepts the connection but never writes a response simulates a
        // hung/very-slow local server (same style as probe_reachable's tests).
        // With timeout_secs=1 this must fail fast with LlmError::Timeout, not
        // hang for reqwest's 30s implicit default.
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let _handle = std::thread::spawn(move || {
            // Accept and hold the connection open without ever responding.
            // Binding the stream (not `let _ = ...`) matters: dropping it
            // immediately would close/reset the connection right away instead
            // of leaving the client waiting on a silent, still-open socket.
            let (_stream, _addr) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(5));
        });

        let base_url = format!("http://{addr}/v1/chat/completions");
        let client = ChatCompletionsClient::new(base_url, "test-key", false, 1);
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096);

        let start = std::time::Instant::now();
        let err = client.complete(&req).unwrap_err();
        let elapsed = start.elapsed();

        assert!(matches!(err, LlmError::Timeout(_)), "expected a Timeout, got {err:?}");
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "timed out at ~1s, not reqwest's 30s implicit default (elapsed={elapsed:?})"
        );
        // Don't join the spawned thread: it sleeps 5s regardless of the test's
        // own ~1s timeout, and joining would make this test as slow as it.
    }

    // --- Base-URL routing + attribution-header gating (loopback, no internet) -

    /// Spin up a one-shot loopback HTTP server, point the client at it, fire one
    /// `complete()` and return the raw request bytes the client sent. Lets us
    /// assert *where* the POST goes and *which* headers are attached without any
    /// real provider or internet access.
    #[cfg(test)]
    fn capture_request(send_openrouter_headers: bool) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Headers precede the body, so the first read carries them in full.
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body =
                r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"total_tokens":1}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = tx.send(req);
        });

        let base_url = format!("http://{addr}/v1/chat/completions");
        let client = ChatCompletionsClient::new(base_url, "test-key", send_openrouter_headers, 30);
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000), 4096);
        let _ = client.complete(&req);
        let captured = rx.recv().unwrap();
        handle.join().unwrap();
        captured
    }

    #[test]
    fn complete_posts_to_the_configured_base_url() {
        // The request reaches the arbitrary base-URL (not the OpenRouter const),
        // hitting the /v1/chat/completions path we configured.
        let captured = capture_request(true);
        assert!(captured.starts_with("POST /v1/chat/completions"));
    }

    #[test]
    fn attribution_headers_sent_only_when_openrouter_flag_is_set() {
        // openrouter preset -> attribution headers present.
        let with = capture_request(true).to_lowercase();
        assert!(with.contains("http-referer"));
        assert!(with.contains("x-title"));

        // Any other endpoint -> no OpenRouter-specific attribution headers.
        let without = capture_request(false).to_lowercase();
        assert!(!without.contains("http-referer"));
        assert!(!without.contains("x-title"));
    }

    // --- Response helper -----------------------------------------------------

    #[test]
    fn chat_response_content_reads_first_choice() {
        let resp: ChatResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"role":"assistant","content":"hi"}}],"usage":{"total_tokens":42}}"#,
        )
        .unwrap();
        assert_eq!(resp.content().unwrap(), "hi");
        assert_eq!(resp.usage.unwrap().total_tokens, 42);
    }
}

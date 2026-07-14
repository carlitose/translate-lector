//! OpenRouter chat-completions transport, percettore output types, prompt
//! builder and the layered JSON parsing fallback (SPECIFICATION §4.4, ticket
//! 08, research-openrouter-contract.md).
//!
//! The transport is abstracted behind [`ChatClient`] so the translation service
//! can be unit-tested with a mock, no network required. The real
//! [`OpenRouterClient`] uses a blocking `reqwest` client and is exercised only
//! by human QA against a live key.

use serde::{Deserialize, Serialize};

/// OpenRouter chat-completions endpoint (§4.4).
pub const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
/// Attribution headers (optional; used only for OpenRouter leaderboards).
const HTTP_REFERER: &str = "https://github.com/translate-lector/translate-lector";
const X_TITLE: &str = "translate-lector";
/// JSON-schema name sent in `response_format` (§4.4).
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
/// `choices[0].message.content` (§4.4).
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
            _ => Err(LlmError::Http(
                "risposta senza contenuto testuale (content null/vuoto)".into(),
            )),
        }
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
    /// The response could not be parsed even after the correction retry.
    ParseFailed(String),
    /// A local storage error while reading/writing the cache.
    Storage(String),
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
                "EC03: API key OpenRouter mancante o non valida. \
                 Configurala in ⚙️ (Impostazioni provider)."
                    .into()
            }
            LlmError::Http(m) => format!("Errore di rete/OpenRouter: {m}"),
            LlmError::UnsupportedParams(m) => format!(
                "Il modello selezionato non supporta i parametri richiesti \
                 (nessun endpoint compatibile). {m}"
            ),
            LlmError::Timeout(m) => format!("Errore di rete/OpenRouter (timeout): {m}"),
            LlmError::ServerError(m) => format!("Errore del servizio OpenRouter: {m}"),
            LlmError::RateLimited(m) => {
                format!("EC07: limite di richieste raggiunto (rate limit). Riprova tra poco. {m}")
            }
            LlmError::Offline(m) => {
                format!(
                    "EC02: nessuna connessione. Le pagine già tradotte restano \
                     leggibili dalla cache. {m}"
                )
            }
            LlmError::ParseFailed(m) => {
                format!("Risposta del modello non valida (JSON non conforme): {m}")
            }
            LlmError::Storage(m) => format!("Errore della cache locale: {m}"),
        }
    }
}

// --- Retry with exponential backoff (NFR06, EC07) ----------------------------

/// Bounded retry policy for transient transport failures. Backoff is
/// exponential: `base_delay * 2^attempt`. Tests use [`RetryPolicy::no_delay`]
/// so no wall-clock time is spent.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Total attempts (including the first). Clamped to at least 1.
    pub max_attempts: u32,
    /// Delay before the first retry; doubles each subsequent retry.
    pub base_delay: std::time::Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 3, base_delay: std::time::Duration::from_millis(500) }
    }
}

impl RetryPolicy {
    /// A policy with no backoff delay, for fast unit tests.
    #[cfg(test)]
    pub fn no_delay(max_attempts: u32) -> Self {
        Self { max_attempts, base_delay: std::time::Duration::ZERO }
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
                    // Give up on the last attempt or on a permanent error.
                    if attempt >= max || !e.is_transient() {
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

/// Real OpenRouter client backed by a blocking `reqwest` client.
pub struct OpenRouterClient {
    api_key: String,
    http: reqwest::blocking::Client,
}

impl OpenRouterClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            http: reqwest::blocking::Client::new(),
        }
    }
}

impl ChatClient for OpenRouterClient {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        // EC03 guard: never touch the network without a key.
        if self.api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey);
        }

        let resp = self
            .http
            .post(OPENROUTER_URL)
            .bearer_auth(self.api_key.trim())
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", HTTP_REFERER)
            .header("X-Title", X_TITLE)
            .json(req)
            .send()
            .map_err(|e| {
                // Classify transport failures so the retry layer can react
                // (timeout/offline = transient; anything else = permanent).
                let msg = e.to_string();
                if e.is_timeout() {
                    LlmError::Timeout(msg)
                } else if e.is_connect() {
                    LlmError::Offline(msg)
                } else {
                    LlmError::Http(msg)
                }
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
/// and every property in `required`).
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
/// (§3.5, decision D5) so the prompt reflects the configured limit.
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
/// recompression instruction is appended (EC05).
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
/// flag shape the user message.
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
/// Deliberately does **not** send `provider.require_parameters` (bug #1): with
/// it, OpenRouter routes only to endpoints supporting *every* parameter in the
/// body, so a model that does not advertise `temperature` (e.g. a reasoning
/// model) gets a 404 "No endpoints found". Without it the router silently
/// ignores parameters the model does not support. `temperature` is kept (0.2)
/// for deterministic translations but is optional, so the model-agnostic
/// fallback ([`complete_with_fallback`]) can drop it if a model still rejects it.
pub fn build_request(model: &str, messages: Vec<ChatMessage>) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages,
        temperature: Some(0.2),
        max_tokens: 4096,
        stream: false,
        response_format: Some(response_format()),
        provider: None,
    }
}

/// The correction message appended before the single retry (layer c, §4.4).
pub const CORRECTION_PROMPT: &str =
    "La tua risposta non era JSON valido conforme allo schema. Rispondi di nuovo con SOLO \
l'oggetto JSON, senza testo aggiuntivo, senza markdown, senza code fence.";

// --- Layered parsing (§4.4) --------------------------------------------------

/// Parse the model `content` into [`PerceptoreOutput`] using layers (a) direct
/// deserialize and (b) first balanced `{...}` block extraction. The correction
/// retry (layer c) lives in the translation service, which can call the client
/// again; this function is the pure, string-only part.
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
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000));
        assert_eq!(req.model, "openai/gpt-4o");
        assert!(!req.stream);
        let rf = req.response_format.clone().unwrap();
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(req.temperature, Some(0.2), "low temperature kept for quality");
    }

    // --- Bug #1: default body must not force provider routing --------------

    #[test]
    fn build_request_default_body_has_no_require_parameters() {
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000));
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
        let mut req = build_request("m", build_messages("it", "x", "", "", "", false, 1000));
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
    fn backoff_is_exponential_from_the_base_delay() {
        let p = RetryPolicy { max_attempts: 4, base_delay: Duration::from_millis(100) };
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
        build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000))
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
    fn retry_does_not_retry_permanent_errors_ec03() {
        let inner = SeqClient::new(vec![Err(LlmError::MissingApiKey)]);
        let client = RetryingChatClient::new(&inner, RetryPolicy::no_delay(5));

        let err = client.complete(&a_request()).unwrap_err();
        assert_eq!(err, LlmError::MissingApiKey);
        assert_eq!(inner.calls.get(), 1, "401/EC03 is not retried");
    }

    // --- Real client key guard (no network) ---------------------------------

    #[test]
    fn openrouter_client_with_empty_key_errors_before_network() {
        let client = OpenRouterClient::new("   ");
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000));
        assert_eq!(client.complete(&req), Err(LlmError::MissingApiKey));
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

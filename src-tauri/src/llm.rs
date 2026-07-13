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

/// One chat message (`role` + `content`).
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
#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<serde_json::Value>,
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Choice {
    pub message: ChatMessage,
}

/// Response body (relevant fields, §4.4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

impl ChatResponse {
    /// The assistant text of the first choice, or an error if none is present.
    pub fn content(&self) -> Result<&str, LlmError> {
        self.choices
            .first()
            .map(|c| c.message.content.as_str())
            .ok_or_else(|| LlmError::Http("risposta senza choices".into()))
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
            return Err(LlmError::Http(format!("{status}: {body}")));
        }

        resp.json::<ChatResponse>()
            .map_err(|e| LlmError::Http(format!("risposta non deserializzabile: {e}")))
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
/// `response_format` and `provider.require_parameters` set.
pub fn build_request(model: &str, messages: Vec<ChatMessage>) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages,
        temperature: 0.2,
        max_tokens: 4096,
        stream: false,
        response_format: Some(response_format()),
        provider: Some(serde_json::json!({ "require_parameters": true })),
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
    fn build_request_sets_response_format_and_provider() {
        let req = build_request("openai/gpt-4o", build_messages("it", "x", "", "", "", false, 1000));
        assert_eq!(req.model, "openai/gpt-4o");
        assert!(!req.stream);
        let rf = req.response_format.unwrap();
        assert_eq!(rf["type"], "json_schema");
        assert_eq!(rf["json_schema"]["strict"], true);
        assert_eq!(req.provider.unwrap()["require_parameters"], true);
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

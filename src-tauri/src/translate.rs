//! Per-page translation service (SPECIFICATION §3.2/§3.3/§4.4, UC02, tickets
//! 08 & 09).
//!
//! On arriving at a page: if a translation is already cached
//! (`translations_cache` keyed by document_id + page_number + target_language)
//! it is returned immediately **without** calling the model and **without**
//! re-running the percettore (the cached page keeps its earlier
//! summary/glossary effect). Otherwise the full percettore prompt is built with
//! the current `rolling_summary` and glossary (locked = absolute constraint,
//! unlocked = suggestions); very large pages are chunked (EC04). Each response
//! is parsed with the layered fallback (direct → block extraction → one
//! correction retry → error). Afterwards the recomposed translation is cached,
//! `sessions.rolling_summary` is updated once (recompressed when over the limit,
//! EC05) and the new glossary terms are inserted deduped.
//!
//! The service takes `&Connection` + `&dyn ChatClient`, so tests inject a mock
//! client and an in-memory DB — no network required.

use crate::glossary;
use crate::llm::{
    build_messages, build_request, calibrate_chars_per_token, complete_with_fallback,
    needs_compression, ChatClient, ChatMessage, ChatRequest, GlossaryTerm, LlmError,
    PerceptoreOutput, Usage, CORRECTION_PROMPT,
};
use crate::{documents, settings};
use rusqlite::{params, Connection, OptionalExtension};

/// Above this many characters a page is split into chunks (EC04): each chunk is
/// translated in its own call, keeping every call well inside the model context
/// budget, then the translated pieces are recomposed.
pub const CHUNK_CHAR_THRESHOLD: usize = 8000;

/// Inputs for a single page translation.
pub struct TranslateParams<'a> {
    pub document_id: i64,
    pub page_number: i64,
    pub target_language: &'a str,
    pub page_text: &'a str,
    pub model: &'a str,
    /// Whether this translation should advance the percettore context (ticket
    /// 09): `true` on real navigation — persists the rolling summary and inserts
    /// glossary terms; `false` on **prefetch** (ticket 12) — caches only the
    /// `translated_text` so a page translated out of order never corrupts the
    /// summary/glossary. The current context is still used read-only as prompt
    /// input either way.
    pub update_context: bool,
}

/// Result of a translation, exposed to the frontend.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TranslationResult {
    pub translated_text: String,
    /// True when served from `translations_cache` (no model call was made).
    pub from_cache: bool,
    /// Sum of `usage.total_tokens` across the page's calls; `None` on a cache
    /// hit or when the provider reported no usage.
    pub total_tokens: Option<i64>,
    /// The rolling summary after this page (percettore); `None` on a cache hit
    /// (the percettore is not re-run for cached pages).
    pub updated_summary: Option<String>,
}

/// Split `text` into chunks of at most `max_chars` characters, preferring to
/// break at whitespace so words are not cut mid-way. Concatenating the chunks
/// in order reproduces `text` exactly (no content lost, EC04). Text at or below
/// the limit yields a single chunk.
pub fn split_into_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if max_chars == 0 || chars.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let hard_end = (start + max_chars).min(chars.len());
        let mut end = hard_end;
        if hard_end < chars.len() {
            // Break after the last whitespace in the window, if any past the start.
            if let Some(pos) = chars[start..hard_end].iter().rposition(|c| c.is_whitespace()) {
                if pos > 0 {
                    end = start + pos + 1;
                }
            }
        }
        chunks.push(chars[start..end].iter().collect::<String>());
        start = end;
    }
    chunks
}

/// Read a cached translation, if present. Returns the stored `translated_text`
/// together with its `source_text` so the caller can verify the cached row was
/// produced from the SAME page text (ticket 16 defence): a row whose stored
/// source differs from the current page text is a poisoned entry and must be
/// treated as a miss, not served.
fn cache_lookup(
    conn: &Connection,
    document_id: i64,
    page_number: i64,
    target_language: &str,
) -> Result<Option<(String, String)>, LlmError> {
    conn.query_row(
        "SELECT translated_text, source_text FROM translations_cache
          WHERE document_id = ?1 AND page_number = ?2 AND target_language = ?3",
        params![document_id, page_number, target_language],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(|e| LlmError::Storage(e.to_string()))
}

/// Insert a freshly translated page into the cache. Uses an UPSERT on the UNIQUE
/// key so a corrected translation OVERWRITES a previously poisoned row (ticket
/// 16 self-heal): a stale write that captured the wrong source_text is replaced
/// as soon as the page is re-translated from its real text.
fn cache_insert(
    conn: &Connection,
    p: &TranslateParams,
    translated_text: &str,
) -> Result<(), LlmError> {
    conn.execute(
        "INSERT INTO translations_cache
             (document_id, page_number, target_language, source_text, translated_text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(document_id, page_number, target_language) DO UPDATE SET
             source_text     = excluded.source_text,
             translated_text = excluded.translated_text,
             created_at      = excluded.created_at",
        params![
            p.document_id,
            p.page_number,
            p.target_language,
            p.page_text,
            translated_text
        ],
    )
    .map_err(|e| LlmError::Storage(e.to_string()))?;
    Ok(())
}

/// Call the client and parse the response with the layered fallback, including
/// the single correction retry (layer c). Returns the parsed output, the
/// provider `usage` when reported (used for cost telemetry and ratio
/// calibration), and the **working request body** that actually succeeded
/// (possibly degraded by the param-relaxation fallback) so the caller can reuse
/// its shape for later chunks without re-probing the already-rejected params.
fn complete_and_parse(
    client: &dyn ChatClient,
    req: ChatRequest,
) -> Result<(PerceptoreOutput, Option<Usage>, ChatRequest), LlmError> {
    // `complete_with_fallback` adds the model-agnostic param-relaxation retry
    // (research §2, bug #1) around the transport: a 404 "No endpoints found" or
    // a 400 unsupported-parameter downgrades the body (drop provider →
    // response_format → temperature) instead of failing outright. It returns the
    // body that worked, so we never re-probe the rejected param below.
    let (resp, working) = complete_with_fallback(client, &req)?;
    let content = resp.content()?.to_string();
    let usage = resp.usage.clone();

    match crate::llm::parse_content(&content) {
        Ok(out) => Ok((out, usage, working)),
        Err(_) => {
            // (c) one correction retry: echo the bad answer, demand pure JSON.
            // Build it on the *working* (possibly degraded) body so the retry
            // does not re-send the param the model already rejected.
            let mut retry_req = working;
            retry_req.messages.push(ChatMessage::assistant(content));
            retry_req.messages.push(ChatMessage::user(CORRECTION_PROMPT));

            let (resp2, working2) = complete_with_fallback(client, &retry_req)?;
            let content2 = resp2.content()?.to_string();
            let usage2 = resp2.usage.clone();

            // (d) final error if the retry still isn't conformant.
            let out = crate::llm::parse_content(&content2).map_err(LlmError::ParseFailed)?;
            Ok((out, usage2.or(usage), working2))
        }
    }
}

/// Map a storage error into [`LlmError::Storage`].
fn storage<E: std::fmt::Display>(e: E) -> LlmError {
    LlmError::Storage(e.to_string())
}

/// Translate a page with the full percettore (SPECIFICATION §3.3/§4.4, UC02).
///
/// Flow: a cache hit returns the stored `translated_text` immediately and does
/// **not** re-run the percettore (no summary/glossary rewrite for cached pages).
/// On a miss the current `rolling_summary` and glossary (locked = absolute
/// constraint, unlocked = suggestions) are loaded and fed to the model; the page
/// is chunked when it exceeds [`CHUNK_CHAR_THRESHOLD`] (EC04) and each chunk is
/// translated in sequence carrying the running summary forward. Afterwards the
/// recomposed translation is cached, `sessions.rolling_summary` is updated once
/// and the new glossary terms are inserted deduped (locked terms untouched).
pub fn translate_page(
    conn: &Connection,
    client: &dyn ChatClient,
    p: &TranslateParams,
) -> Result<TranslationResult, LlmError> {
    // Cache hit → return immediately, no model call, no percettore rewrite —
    // but ONLY when the stored source_text matches the page text we were asked
    // to translate (ticket 16). A mismatch means the row was poisoned by a stale
    // write (page N holding page N-1's text): treat it as a miss so we
    // re-translate and overwrite it below (self-heal), never serving it.
    if let Some((cached_text, cached_source)) =
        cache_lookup(conn, p.document_id, p.page_number, p.target_language)?
    {
        if cached_source == p.page_text {
            return Ok(TranslationResult {
                translated_text: cached_text,
                from_cache: true,
                total_tokens: None,
                updated_summary: None,
            });
        }
    }

    // Load the percettore context (EC03 surfaces later, on the first call).
    let summary_limit =
        settings::get_summary_token_limit(conn).map_err(storage)?;
    let mut rolling_summary = documents::get_rolling_summary(conn, p.document_id).map_err(storage)?;
    let entries = glossary::list_glossary(conn, p.document_id).map_err(storage)?;
    let (locked, unlocked) = glossary::render_locked_unlocked(&entries);

    // Chunk the page (EC04); one chunk when it fits.
    let chunks = split_into_chunks(p.page_text, CHUNK_CHAR_THRESHOLD);

    let mut translated_parts: Vec<String> = Vec::with_capacity(chunks.len());
    let mut new_terms: Vec<GlossaryTerm> = Vec::new();
    let mut total_tokens_sum: i64 = 0;
    let mut saw_usage = false;
    let mut prompt_chars_sum: usize = 0;
    let mut prompt_tokens_sum: i64 = 0;
    // The request shape discovered to work for this page: once the fallback
    // strips a param a model rejects, later chunks start already-degraded so
    // they don't each pay the same failed 404 (bug #1 follow-up). `None` until
    // the first chunk returns its working body.
    let mut working_shape: Option<ChatRequest> = None;

    for chunk in &chunks {
        // Recompression is requested when the running summary is over the
        // threshold (EC05); after the model compresses it, later chunks won't.
        let compress = needs_compression(&rolling_summary, summary_limit);
        let messages = build_messages(
            p.target_language,
            chunk,
            &rolling_summary,
            &locked,
            &unlocked,
            compress,
            summary_limit,
        );
        prompt_chars_sum += messages.iter().map(|m| m.content.chars().count()).sum::<usize>();

        let mut req = build_request(p.model, messages);
        // Reuse the optional-param shape discovered on a previous chunk so we
        // don't re-probe a param the model already rejected this page.
        if let Some(shape) = &working_shape {
            req.temperature = shape.temperature;
            req.response_format = shape.response_format.clone();
            req.provider = shape.provider.clone();
        }
        let (output, usage, working) = complete_and_parse(client, req)?;
        working_shape = Some(working);

        translated_parts.push(output.translated_text);
        rolling_summary = output.updated_summary; // carry continuity to next chunk
        new_terms.extend(output.new_glossary_terms);

        if let Some(u) = usage {
            saw_usage = true;
            total_tokens_sum += u.total_tokens;
            prompt_tokens_sum += u.prompt_tokens;
        }
    }

    let translated_text = translated_parts.join("\n\n");

    // Persist. The cache is written either way. The percettore context (summary
    // + glossary) is advanced ONLY on real navigation (`update_context`): a
    // prefetch of a later page must not mutate the running context out of order
    // (ticket 12) — it warms the cache and nothing else.
    cache_insert(conn, p, &translated_text)?;
    if p.update_context {
        documents::set_rolling_summary(conn, p.document_id, &rolling_summary).map_err(storage)?;
        glossary::insert_terms_deduped(conn, p.document_id, &new_terms, p.page_number)
            .map_err(storage)?;

        // Calibrate the chars/token ratio from real usage (research §3) — stored
        // for cost telemetry; `needs_compression` keeps the stable default ratio.
        if let Some(ratio) = calibrate_chars_per_token(prompt_chars_sum, prompt_tokens_sum) {
            let _ =
                settings::set_setting(conn, settings::CHARS_PER_TOKEN_KEY, &format!("{ratio:.4}"));
        }
    }

    let total_tokens = if saw_usage { Some(total_tokens_sum) } else { None };
    if let Some(tokens) = total_tokens {
        // Cost telemetry (NFR04): logged rather than a schema column for the MVP.
        eprintln!(
            "[usage] document_id={} page={} lang={} chunks={} prefetch={} total_tokens={}",
            p.document_id,
            p.page_number,
            p.target_language,
            chunks.len(),
            !p.update_context,
            tokens
        );
    }

    Ok(TranslationResult {
        translated_text,
        from_cache: false,
        total_tokens,
        // Only report the advanced summary when it was actually persisted; a
        // prefetch reports `None` because it did not touch the context.
        updated_summary: if p.update_context { Some(rolling_summary) } else { None },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatResponse, GlossaryTerm};
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    // --- Mock client ---------------------------------------------------------

    /// A `ChatClient` that pops canned results, counts calls and records every
    /// request so tests can assert what the prompt contained. No network.
    struct MockClient {
        responses: RefCell<VecDeque<Result<ChatResponse, LlmError>>>,
        calls: Cell<usize>,
        requests: RefCell<Vec<ChatRequest>>,
    }

    impl MockClient {
        fn new(responses: Vec<Result<ChatResponse, LlmError>>) -> Self {
            Self {
                responses: RefCell::new(responses.into_iter().collect()),
                calls: Cell::new(0),
                requests: RefCell::new(Vec::new()),
            }
        }
        fn calls(&self) -> usize {
            self.calls.get()
        }
        /// The user-message text of the recorded request at `idx`.
        fn user_prompt(&self, idx: usize) -> String {
            let reqs = self.requests.borrow();
            reqs[idx]
                .messages
                .iter()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone())
                .unwrap_or_default()
        }
    }

    impl ChatClient for MockClient {
        fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.calls.set(self.calls.get() + 1);
            self.requests.borrow_mut().push(req.clone());
            self.responses
                .borrow_mut()
                .pop_front()
                .expect("MockClient: unexpected extra call")
        }
    }

    /// A response whose content is `content` and total_tokens is `tokens`.
    fn resp(content: &str, tokens: i64) -> ChatResponse {
        serde_json::from_value(serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": content } }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": tokens }
        }))
        .unwrap()
    }

    fn valid_content() -> String {
        serde_json::to_string(&PerceptoreOutput {
            translated_text: "Ciao mondo".into(),
            updated_summary: "riassunto".into(),
            new_glossary_terms: vec![GlossaryTerm {
                source_term: "hello".into(),
                translation: "ciao".into(),
                term_type: "comune".into(),
                note: String::new(),
            }],
        })
        .unwrap()
    }

    /// Percettore content with a chosen translation, summary and terms.
    fn content_with(text: &str, summary: &str, terms: &[(&str, &str)]) -> String {
        serde_json::to_string(&PerceptoreOutput {
            translated_text: text.into(),
            updated_summary: summary.into(),
            new_glossary_terms: terms
                .iter()
                .map(|(s, t)| GlossaryTerm {
                    source_term: (*s).into(),
                    translation: (*t).into(),
                    term_type: "comune".into(),
                    note: String::new(),
                })
                .collect(),
        })
        .unwrap()
    }

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::db::init_schema(&c).unwrap();
        // Seed document id=1 so translations_cache's FK is satisfied.
        c.execute(
            "INSERT INTO documents (id, file_path, file_hash, title, total_pages)
             VALUES (1, '/tmp/x.pdf', 'hash', 'x', 10)",
            [],
        )
        .unwrap();
        c
    }

    /// Seed the single reading session for document 1 (needed to persist the
    /// rolling summary). Returns nothing; the session is found by document_id.
    fn seed_session(c: &Connection) {
        crate::documents::open_or_create_session(c, 1).unwrap();
    }

    fn params<'a>(text: &'a str) -> TranslateParams<'a> {
        TranslateParams {
            document_id: 1,
            page_number: 3,
            target_language: "it",
            page_text: text,
            model: "openai/gpt-4o",
            update_context: true,
        }
    }

    // --- Cache hit -----------------------------------------------------------

    #[test]
    fn cache_hit_returns_cached_without_calling_client() {
        let c = conn();
        c.execute(
            "INSERT INTO translations_cache
                (document_id, page_number, target_language, source_text, translated_text, created_at)
             VALUES (1, 3, 'it', 'Hello', 'Ciao (cache)', '2026-07-13T00:00:00Z')",
            [],
        )
        .unwrap();

        let client = MockClient::new(vec![]); // any call would panic (empty queue)
        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert_eq!(out.translated_text, "Ciao (cache)");
        assert!(out.from_cache);
        assert_eq!(client.calls(), 0, "cache hit must not call the model");
    }

    /// A cache hit whose stored `source_text` DIFFERS from the request's
    /// `page_text` must be treated as a MISS: the model is called and the row is
    /// overwritten so a poisoned row self-heals (defence-in-depth, ticket 16).
    #[test]
    fn cache_hit_with_mismatched_source_text_is_a_miss_and_overwrites() {
        let c = conn();
        seed_session(&c);
        // A poisoned row: page 3 stored the WRONG source text (from another page).
        c.execute(
            "INSERT INTO translations_cache
                (document_id, page_number, target_language, source_text, translated_text, created_at)
             VALUES (1, 3, 'it', 'WRONG source', 'Traduzione avvelenata', '2026-07-13T00:00:00Z')",
            [],
        )
        .unwrap();

        let client = MockClient::new(vec![Ok(resp(&valid_content(), 400))]);
        // Request page 3 with its REAL text — source_text differs from the row.
        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert!(!out.from_cache, "mismatched source_text must be a miss");
        assert_eq!(out.translated_text, "Ciao mondo", "re-translated, not the poisoned value");
        assert_eq!(client.calls(), 1, "the model is called on a source mismatch");

        // The poisoned row was OVERWRITTEN with the correct source + translation.
        let (stored, src): (String, String) = c
            .query_row(
                "SELECT translated_text, source_text FROM translations_cache
                  WHERE document_id=1 AND page_number=3 AND target_language='it'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, "Ciao mondo", "row overwritten with the fresh translation");
        assert_eq!(src, "Hello", "row overwritten with the correct source text");
        // Exactly one row for this key (overwrite, not a duplicate insert).
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM translations_cache
                  WHERE document_id=1 AND page_number=3 AND target_language='it'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "the key still holds exactly one (healed) row");
    }

    /// The exact poisoning repro from the diagnosis: write page 10 with page-9
    /// text, then request page 10 with page-10 text. The page-9 translation must
    /// NOT be served — the page re-translates and the row is corrected.
    #[test]
    fn poisoning_repro_stale_write_then_correct_read_retranslates() {
        let c = conn();
        seed_session(&c);
        // Run #1 of the reactive race poisoned page 10 with page-9's text.
        c.execute(
            "INSERT INTO translations_cache
                (document_id, page_number, target_language, source_text, translated_text, created_at)
             VALUES (1, 10, 'it', 'page 9 text', 'Ignoranza', '2026-07-13T00:00:00Z')",
            [],
        )
        .unwrap();

        let client = MockClient::new(vec![Ok(resp(
            &content_with("Traduzione pagina 10", "s", &[]),
            500,
        ))]);
        let p = TranslateParams {
            document_id: 1,
            page_number: 10,
            target_language: "it",
            page_text: "page 10 text",
            model: "openai/gpt-4o",
            update_context: true,
        };

        let out = translate_page(&c, &client, &p).unwrap();

        assert_ne!(out.translated_text, "Ignoranza", "must NOT serve the page-9 translation");
        assert_eq!(out.translated_text, "Traduzione pagina 10");
        assert!(!out.from_cache, "the mismatched row is not served as a cache hit");
        assert_eq!(client.calls(), 1, "the page is re-translated");

        // A subsequent visit with the SAME correct text is now a legitimate hit.
        let out2 = translate_page(&c, &client, &p).unwrap();
        assert!(out2.from_cache, "healed row is served on the next matching visit");
        assert_eq!(out2.translated_text, "Traduzione pagina 10");
        assert_eq!(client.calls(), 1, "no extra model call once healed");
    }

    // --- Cache miss ----------------------------------------------------------

    #[test]
    fn cache_miss_calls_client_saves_and_records_tokens() {
        let c = conn();
        let client = MockClient::new(vec![Ok(resp(&valid_content(), 1801))]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert_eq!(out.translated_text, "Ciao mondo");
        assert!(!out.from_cache);
        assert_eq!(out.total_tokens, Some(1801), "usage.total_tokens recorded");
        assert_eq!(client.calls(), 1);

        // Persisted with the UNIQUE key and source text.
        let (stored, src): (String, String) = c
            .query_row(
                "SELECT translated_text, source_text FROM translations_cache
                  WHERE document_id=1 AND page_number=3 AND target_language='it'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, "Ciao mondo");
        assert_eq!(src, "Hello");
    }

    #[test]
    fn second_visit_reads_from_cache_no_second_call() {
        let c = conn();
        let client = MockClient::new(vec![Ok(resp(&valid_content(), 500))]);

        let first = translate_page(&c, &client, &params("Hello")).unwrap();
        assert!(!first.from_cache);

        let second = translate_page(&c, &client, &params("Hello")).unwrap();
        assert!(second.from_cache);
        assert_eq!(second.translated_text, "Ciao mondo");
        assert_eq!(client.calls(), 1, "no second model call for a cached page");
    }

    // --- Layered parsing through the service --------------------------------

    #[test]
    fn malformed_then_valid_succeeds_after_one_correction_retry() {
        let c = conn();
        let client = MockClient::new(vec![
            Ok(resp("sorry, here is the translation…", 100)),
            Ok(resp(&format!("```json\n{}\n```", valid_content()), 120)),
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo");
        assert_eq!(out.total_tokens, Some(120));
        assert_eq!(client.calls(), 2, "exactly one correction retry");
    }

    #[test]
    fn malformed_twice_yields_parse_failed_error() {
        let c = conn();
        let client = MockClient::new(vec![
            Ok(resp("not json", 10)),
            Ok(resp("still not json", 10)),
        ]);

        let err = translate_page(&c, &client, &params("Hello")).unwrap_err();
        assert!(matches!(err, LlmError::ParseFailed(_)));
        assert_eq!(client.calls(), 2, "only one retry, then give up");

        // Nothing cached on failure.
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM translations_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // --- Bug #1: model-agnostic 404 fallback through the service ------------

    /// A 404 "No endpoints found" on the full body must trigger one downgraded
    /// retry (research §2) that succeeds — the page still translates.
    #[test]
    fn unsupported_params_404_recovers_via_downgraded_retry() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Err(LlmError::UnsupportedParams(
                "404 Not Found: {\"error\":{\"message\":\"No endpoints found that can handle \
                 the requested parameters.\",\"code\":404}}"
                    .into(),
            )),
            Ok(resp(&valid_content(), 321)),
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo");
        assert_eq!(out.total_tokens, Some(321));
        assert_eq!(client.calls(), 2, "full body 404 then a downgraded retry that succeeds");

        // The downgraded retry dropped `provider` first (it was already None in
        // the default body) then `response_format`.
        let reqs = client.requests.borrow();
        assert!(reqs[0].response_format.is_some(), "first attempt sent response_format");
        assert!(reqs[1].response_format.is_none(), "retry stripped response_format");
    }

    /// A non-degradable 404 (e.g. genuinely missing model) is surfaced, not
    /// retried into oblivion.
    #[test]
    fn non_degradable_http_error_is_surfaced_without_param_relaxation() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![Err(LlmError::Http("404 Not Found: model not found".into()))]);

        let err = translate_page(&c, &client, &params("Hello")).unwrap_err();
        assert!(matches!(err, LlmError::Http(_)));
        assert_eq!(client.calls(), 1, "no param-relaxation retry for a plain HTTP 404");
    }

    /// After the fallback degrades the body on the first chunk, later chunks of
    /// the SAME page must start already-degraded — no repeated 404 re-probe
    /// (finding 2b). With the fix the second chunk succeeds in one call.
    #[test]
    fn degraded_shape_is_reused_by_later_chunks_no_reprobe() {
        let c = conn();
        seed_session(&c);
        // ~12000 chars -> exactly two chunks at the 8000 threshold.
        let big = "lorem ipsum ".repeat(1000);
        assert_eq!(split_into_chunks(&big, CHUNK_CHAR_THRESHOLD).len(), 2, "precondition: two chunks");

        let client = MockClient::new(vec![
            // chunk 1: full body 404, then a downgraded retry succeeds.
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
            Ok(resp(&content_with("PART0", "s0", &[]), 10)),
            // chunk 2: must already be degraded -> ONE successful call, no 404.
            Ok(resp(&content_with("PART1", "s1", &[]), 10)),
        ]);

        let out = translate_page(&c, &client, &params_page(&big)).unwrap();

        assert_eq!(client.calls(), 3, "chunk1: 2 calls; chunk2: 1 call (no re-probe)");
        assert!(out.translated_text.contains("PART0") && out.translated_text.contains("PART1"));

        let reqs = client.requests.borrow();
        assert!(reqs[0].response_format.is_some(), "chunk1 first attempt sent response_format");
        assert!(reqs[1].response_format.is_none(), "chunk1 retry stripped response_format");
        assert!(
            reqs[2].response_format.is_none(),
            "chunk2 starts already-degraded, no response_format re-probe"
        );
    }

    /// The JSON-correction retry must be issued on the already-degraded body, so
    /// it does not re-send the param the model rejected (finding 2a). Sequence:
    /// full-body 404 -> degraded call returns malformed JSON -> correction retry
    /// on the degraded body returns valid JSON. That is exactly 3 calls; a
    /// re-probe would make 4.
    #[test]
    fn correction_retry_reuses_degraded_body_no_reprobe() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
            Ok(resp("not json at all", 10)),
            Ok(resp(&valid_content(), 55)),
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert_eq!(out.translated_text, "Ciao mondo");
        assert_eq!(out.total_tokens, Some(55));
        assert_eq!(client.calls(), 3, "404 + degraded malformed + degraded correction retry");

        let reqs = client.requests.borrow();
        assert!(reqs[1].response_format.is_none(), "degraded call stripped response_format");
        assert!(
            reqs[2].response_format.is_none(),
            "correction retry reuses the degraded body (no re-probe)"
        );
        assert!(
            reqs[2].messages.iter().any(|m| m.content.contains(crate::llm::CORRECTION_PROMPT)),
            "correction retry carries the correction prompt"
        );
    }

    // --- Missing API key (EC03) ---------------------------------------------

    #[test]
    fn missing_api_key_propagates_ec03_without_caching() {
        let c = conn();
        let client = MockClient::new(vec![Err(LlmError::MissingApiKey)]);

        let err = translate_page(&c, &client, &params("Hello")).unwrap_err();
        assert_eq!(err, LlmError::MissingApiKey);
        assert!(err.user_message().contains("EC03"));

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM translations_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "no cache write when the key is missing");
    }

    // --- Percettore context in the prompt (ticket 09) -----------------------

    #[test]
    fn prompt_carries_summary_and_locked_glossary_as_absolute() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Contesto delle pagine precedenti.").unwrap();
        // A locked term (as ticket 10 would create) must be flagged absolute.
        c.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'board', 'consiglio', 'tecnico', 1, '', 1)",
            [],
        )
        .unwrap();

        let client = MockClient::new(vec![Ok(resp(&valid_content(), 100))]);
        translate_page(&c, &client, &params("The board met.")).unwrap();

        let prompt = client.user_prompt(0);
        assert!(prompt.contains("Contesto delle pagine precedenti."), "summary in prompt");
        assert!(prompt.contains("Termini BLOCCATI (vincolo assoluto"), "absolute heading");
        assert!(prompt.contains("board => consiglio"), "locked term rendered");
    }

    // --- Summary persistence (ticket 09) ------------------------------------

    #[test]
    fn updated_summary_persisted_to_session_after_page() {
        let c = conn();
        seed_session(&c);
        let client =
            MockClient::new(vec![Ok(resp(&content_with("Tradotto", "Nuovo riassunto pag. 3.", &[]), 200))]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.updated_summary.as_deref(), Some("Nuovo riassunto pag. 3."));

        // Reloads from the DB (persist + reload).
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            "Nuovo riassunto pag. 3."
        );
    }

    // --- Glossary population deduped (ticket 09) ----------------------------

    #[test]
    fn new_terms_inserted_unlocked_with_page_and_deduped() {
        let c = conn();
        seed_session(&c);
        // Pre-existing locked term must survive untouched.
        c.execute(
            "INSERT INTO glossary
                 (document_id, source_term, translation, type, locked, note, first_seen_page)
             VALUES (1, 'board', 'CONSIGLIO', 'tecnico', 1, '', 1)",
            [],
        )
        .unwrap();

        let client = MockClient::new(vec![Ok(resp(
            &content_with("t", "s", &[("board", "altra"), ("CEO", "ad")]),
            100,
        ))]);
        translate_page(&c, &client, &params("x")).unwrap();

        let entries = crate::glossary::list_glossary(&c, 1).unwrap();
        assert_eq!(entries.len(), 2, "board deduped, CEO added");

        let board = entries.iter().find(|e| e.source_term == "board").unwrap();
        assert!(board.locked, "locked term preserved");
        assert_eq!(board.translation, "CONSIGLIO", "locked translation untouched");

        let ceo = entries.iter().find(|e| e.source_term == "CEO").unwrap();
        assert!(!ceo.locked, "new term is unlocked");
        assert_eq!(ceo.first_seen_page, 3, "first_seen_page = current page");
    }

    // --- Compression flow (EC05) --------------------------------------------

    #[test]
    fn over_threshold_summary_triggers_recompression_and_drops_under() {
        let c = conn();
        seed_session(&c);
        // Default limit 1000 -> threshold 800 tokens -> 3200 chars. Seed 4000
        // chars (~1000 tokens), which is over the threshold.
        let long_summary = "a ".repeat(2000); // 4000 chars
        crate::documents::set_rolling_summary(&c, 1, &long_summary).unwrap();
        assert!(needs_compression(&long_summary, 1000), "precondition: over threshold");

        // Model returns a short, recompressed summary.
        let short = "Riassunto compresso.";
        let client = MockClient::new(vec![Ok(resp(&content_with("Tradotto", short, &[]), 500))]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        // The prompt for this page requested recompression.
        assert!(
            client.user_prompt(0).contains(crate::llm::COMPRESSION_INSTRUCTION),
            "next-page prompt requests recompression"
        );
        // The resulting summary is back under the threshold.
        assert_eq!(out.updated_summary.as_deref(), Some(short));
        assert!(!needs_compression(short, 1000), "compressed summary is under threshold");
        assert_eq!(crate::documents::get_rolling_summary(&c, 1).unwrap(), short);
    }

    #[test]
    fn under_threshold_summary_does_not_request_recompression() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Breve riassunto.").unwrap();

        let client = MockClient::new(vec![Ok(resp(&valid_content(), 100))]);
        translate_page(&c, &client, &params("Hello")).unwrap();

        assert!(
            !client.user_prompt(0).contains(crate::llm::COMPRESSION_INSTRUCTION),
            "no recompression request under threshold"
        );
    }

    // --- Chunking (EC04) -----------------------------------------------------

    #[test]
    fn split_into_chunks_preserves_content_and_respects_limit() {
        let text = "lorem ipsum ".repeat(2000); // 24000 chars
        let chunks = split_into_chunks(&text, CHUNK_CHAR_THRESHOLD);
        assert!(chunks.len() > 1, "large text is split");
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHAR_THRESHOLD));
        assert_eq!(chunks.concat(), text, "no content lost, order preserved");

        // Small text stays a single chunk.
        assert_eq!(split_into_chunks("short", CHUNK_CHAR_THRESHOLD), vec!["short".to_string()]);
    }

    #[test]
    fn large_page_chunks_into_multiple_calls_and_recomposes() {
        let c = conn();
        seed_session(&c);
        // ~24000 chars -> at least 3 chunks with the 8000 threshold.
        let big = "lorem ipsum dolor ".repeat(1400);
        let chunks = split_into_chunks(&big, CHUNK_CHAR_THRESHOLD);
        let n = chunks.len();
        assert!(n >= 2, "precondition: multiple chunks");

        // One canned response per chunk, each with a distinct ordered marker.
        let responses: Vec<_> = (0..n)
            .map(|i| {
                Ok(resp(
                    &content_with(&format!("PART{i}"), &format!("riassunto dopo chunk {i}"), &[]),
                    10,
                ))
            })
            .collect();
        let client = MockClient::new(responses);

        let out = translate_page(&c, &client, &params_page(&big)).unwrap();

        assert_eq!(client.calls(), n, "one model call per chunk");
        // Recomposed translation contains every part in order.
        for i in 0..n {
            assert!(out.translated_text.contains(&format!("PART{i}")), "PART{i} present");
        }
        let pos_first = out.translated_text.find("PART0").unwrap();
        let pos_last = out.translated_text.find(&format!("PART{}", n - 1)).unwrap();
        assert!(pos_first < pos_last, "parts recomposed in order");

        // Summary updated exactly once, to the last chunk's summary.
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            format!("riassunto dopo chunk {}", n - 1)
        );
        // Total tokens summed across chunk calls.
        assert_eq!(out.total_tokens, Some(10 * n as i64));
    }

    fn params_page(text: &str) -> TranslateParams<'_> {
        TranslateParams {
            document_id: 1,
            page_number: 3,
            target_language: "it",
            page_text: text,
            model: "openai/gpt-4o",
            update_context: true,
        }
    }

    // --- Prefetch: cache-only, no context mutation (ticket 12) --------------

    /// A prefetch (`update_context: false`) of a later page must cache the
    /// translation but leave `sessions.rolling_summary` and the glossary
    /// untouched — advancing the percettore out of order would corrupt context.
    #[test]
    fn prefetch_caches_translation_without_touching_summary_or_glossary() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Riassunto originale.").unwrap();
        let before_glossary = crate::glossary::list_glossary(&c, 1).unwrap().len();

        // Model would advance the summary and add a term — both must be ignored.
        let client = MockClient::new(vec![Ok(resp(
            &content_with("Tradotto in anticipo", "Riassunto AVANZATO (da ignorare)", &[("new", "nuovo")]),
            300,
        ))]);
        let prefetch = TranslateParams {
            document_id: 1,
            page_number: 4, // the NEXT page (N+1)
            target_language: "it",
            page_text: "Next page text.",
            model: "openai/gpt-4o",
            update_context: false,
        };

        let out = translate_page(&c, &client, &prefetch).unwrap();
        assert!(!out.from_cache);
        assert_eq!(out.updated_summary, None, "prefetch does not report/persist a summary");
        assert_eq!(client.calls(), 1);

        // The translation IS cached for page 4.
        let cached: String = c
            .query_row(
                "SELECT translated_text FROM translations_cache
                  WHERE document_id=1 AND page_number=4 AND target_language='it'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cached, "Tradotto in anticipo");

        // The context is UNCHANGED: summary unchanged, no glossary rows added.
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            "Riassunto originale.",
            "prefetch must not advance the rolling summary"
        );
        assert_eq!(
            crate::glossary::list_glossary(&c, 1).unwrap().len(),
            before_glossary,
            "prefetch must not add glossary terms"
        );
    }

    /// A prefetch that hits the cache is a no-op: no model call, no context write.
    #[test]
    fn prefetch_cache_hit_is_a_noop() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Riassunto originale.").unwrap();
        c.execute(
            "INSERT INTO translations_cache
                (document_id, page_number, target_language, source_text, translated_text, created_at)
             VALUES (1, 4, 'it', 'Next', 'Gia in cache', '2026-07-13T00:00:00Z')",
            [],
        )
        .unwrap();

        let client = MockClient::new(vec![]); // any call panics (empty queue)
        let prefetch = TranslateParams {
            document_id: 1,
            page_number: 4,
            target_language: "it",
            page_text: "Next",
            model: "openai/gpt-4o",
            update_context: false,
        };

        let out = translate_page(&c, &client, &prefetch).unwrap();
        assert!(out.from_cache);
        assert_eq!(out.translated_text, "Gia in cache");
        assert_eq!(client.calls(), 0, "cached prefetch makes no model call");
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            "Riassunto originale."
        );
    }
}

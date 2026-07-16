//! Per-page translation service (SPECIFICATION §3.2/§3.3/§4.4, UC02, tickets
//! 08 & 09).
//!
//! On arriving at a page: if a translation is already cached
//! (`translations_cache` keyed by document_id + page_number + target_language)
//! it is returned immediately **without** calling the model and **without**
//! re-running the percettore (the cached page keeps its earlier
//! summary/glossary effect). Otherwise the page is split into small
//! translate-only units (STC-08) and, once per page on real navigation, a single
//! **lean perceptor-update** call (STC-10) derives the new summary + glossary
//! terms from a compact selected glossary — WITHOUT re-translating the page
//! (budget-safe). Each response is parsed with the layered fallback (direct →
//! block extraction → one correction retry → error). Afterwards the recomposed
//! translation is cached; on real navigation `sessions.rolling_summary` is
//! updated once (recompressed when over the limit, EC05) and the new glossary
//! terms are inserted deduped — but only when the perceptor-update succeeded.
//!
//! Between the page-level cache and the model there is a **per-unit resume
//! cache** (`unit_translations`, ticket 09): every unit is written to it the
//! moment it succeeds, *before* the per-page perceptor call. If a **unit** (or a
//! transport error) then fails mid-page, the units already done survive, so a
//! retry only re-translates the missing units and re-runs the perceptor once. A
//! **perceptor-update** failure no longer aborts the page (STC-10): the page is
//! cached and returned anyway, with the context simply not advanced. Per-unit
//! entries are keyed by `(document_id, page_number, unit_index, target_language)`
//! plus a `source_hash` of the unit body, so a changed paragraph (or a different
//! target language) misses and is re-translated. The page-level row stays the
//! "page fully done" signal.
//!
//! The service takes `&Connection` + `&dyn ChatClient`, so tests inject a mock
//! client and an in-memory DB — no network required.

use crate::glossary;
use crate::llm::{
    build_perceptor_update_messages, build_perceptor_update_request, build_translate_only_messages,
    build_translate_only_request, calibrate_chars_per_token, complete_with_fallback, est_tokens,
    needs_compression, parse_translation, ChatClient, ChatMessage, ChatRequest, GlossaryTerm,
    LlmError, Usage, DEFAULT_CHARS_PER_TOKEN, CORRECTION_PROMPT,
};
use crate::{documents, settings};
use rusqlite::{params, Connection, OptionalExtension};

// --- Modello di budget token (STC-01/STC-08) ---------------------------------

/// Tetto di output **per-unità** (token) sulle chiamate translate-only del
/// percorso a budget stretto (STC-01/D5). Piccolo rispetto al `max_tokens`
/// per-pagina (che resta per la sola chiamata percettore) ma abbastanza capiente
/// da assorbire la verbosità di un modello locale: alzato da 768 a **1024**
/// (ticket 11) perché con 768 un paragrafo denso + un po' di ridondanza sfondava
/// il tetto → `finish_reason:"length"` a metà frase. 1024 lascia comunque ampio
/// headroom di input e margine al **retry su troncamento** (che raddoppia questo
/// budget, vedi [`translate_page`]). È anche la riserva di output nella formula
/// del budget. NB: quando la pagina è UNA sola unità (degradazione cloud o pagina
/// corta) si usa invece il `max_tokens` di pagina, così una pagina intera non
/// viene troncata.
pub const OUT_UNIT_TOKENS: u32 = 1024;

/// Numero massimo di **retry su troncamento** di una singola unità (ticket 11):
/// se una chiamata translate-only ritorna `finish_reason == "length"` con
/// contenuto NON vuoto (traduzione tagliata a metà, [`LlmError::OutputTruncated`])
/// si ritenta con `max_tokens` raddoppiato, limitato dall'headroom del contesto.
/// Cap basso (1-2 tentativi) per non moltiplicare le chiamate: dopo l'ultimo, o
/// quando l'headroom non consente di crescere, il parziale viene rifiutato con un
/// errore EC08 azionabile ([`LlmError::OutputBudgetExhausted`]) — mai cachato.
const TRUNCATION_MAX_RETRIES: u32 = 2;

/// Margine (frazione) che assorbe l'imprecisione dell'euristica `chars/token` nel
/// dimensionamento del budget (STC-01, ~15%).
const BUDGET_MARGIN: f64 = 0.15;

/// Cap sui termini **unlocked** selezionati per unità (D4: word-boundary +
/// morfologia, cap unlocked 10-20, locked uncapped).
const UNLOCKED_GLOSSARY_CAP: usize = 16;

/// Riserva di token per la porzione **unlocked** del glossario selezionato nella
/// formula del budget (stima; i locked sono stimati a parte perché sempre inclusi
/// in ogni prompt di unità).
const GLOSSARY_UNLOCKED_RESERVE_TOKENS: u32 = 256;

/// Dimensione minima (token) di un'unità di traduzione: evita che una formula di
/// budget molto stretta produca un limite assurdo/degenere.
const MIN_BUDGET_UNIT_TEXT: u32 = 256;

/// Fattore prudente token-output per token-input di una unità: una traduzione può
/// espandersi (es. EN→IT), quindi il tetto di output per unità sul percorso a
/// budget cresce fino a ~2× l'input, mai sotto [`OUT_UNIT_TOKENS`]. Così un
/// paragrafo grande (tipico sul cloud, dove l'headroom è ampio) non viene
/// troncato, mentre i paragrafi piccoli restano a `out_unit`.
const OUTPUT_TOKENS_PER_INPUT: u32 = 2;

/// Cuscinetto di sicurezza (token) sottratto dalla finestra nel calcolo
/// dell'headroom di output per unità, così `prompt + output ≤ n_ctx` (guardia EC08).
const OUTPUT_HEADROOM_SAFETY_TOKENS: u32 = 64;

/// Taglia FISSA (token) delle finestre di packing delle unità (decisione L1,
/// decision-brief-latency-03 §L1): le unità-paragrafo di [`split_into_units`]
/// vengono impacchettate da [`pack_units`] in finestre da ~512 token PRIMA
/// della traduzione, così il CoT del modello locale (~500 token per chiamata,
/// misure ticket 01) si paga 1-2 volte per pagina invece di una volta per
/// paragrafo. La taglia è una COSTANTE, NON derivata dal `budget_unit_text`
/// dinamico: è la proprietà che rende la cache per-unità stabile ai "repack"
/// (ticket 02: 2/2 finestre stabili a taglia fissa vs 0/2 col budget dinamico)
/// — il packing dipende solo dal testo della pagina e da questa costante.
/// Clampata al `budget_unit_text` corrente SOLO quando questo è più stretto di
/// 512 (edge case: summary/glossario enormi che restringono il budget).
const PACK_TARGET_TOKENS: u32 = 512;

/// Riserva di output (token) per il **chain-of-thought** nel cap di una
/// finestra multi-unità ([`window_output_cap`], ticket 04). Il modello locale
/// genera ~500 token di CoT per chiamata PRIMA della traduzione (misure ticket
/// 01): con le finestre da ~[`PACK_TARGET_TOKENS`] il solo fattore
/// [`OUTPUT_TOKENS_PER_INPUT`] (2× l'input, l'espansione EN→IT) non basta più
/// a coprire CoT + traduzione, e ogni troncamento pagherebbe un'intera
/// chiamata di retry. La riserva è sommata al fattore scalato, sempre bounded
/// da `p.max_tokens` e dall'headroom del contesto; il retry-troncamento
/// ([`TRUNCATION_MAX_RETRIES`]) resta l'ultima rete di sicurezza.
const COT_RESERVE_TOKENS: u32 = 512;

/// Cap INIZIALE di output per una **finestra multi-unità** (ticket 04): scala
/// col corpo (`body_tokens × OUTPUT_TOKENS_PER_INPUT`, l'espansione della
/// traduzione) PIÙ la riserva CoT ([`COT_RESERVE_TOKENS`]), mai sotto
/// [`OUT_UNIT_TOKENS`], sempre bounded dal `max_tokens` del provider e
/// dall'headroom residuo del contesto (`prompt + output ≤ n_ctx`, guardia
/// EC08). Con una finestra piena (~512 token) e i default locali (max_tokens
/// 2048, n_ctx 4096) il cap risulta ~1536: spazio per CoT (~500) + traduzione
/// (fino a ~2× input) senza pagare il giro extra del retry-troncamento.
/// Funzione pura, unit-testabile.
fn window_output_cap(body_tokens: u32, max_tokens: u32, headroom: u32) -> u32 {
    body_tokens
        .saturating_mul(OUTPUT_TOKENS_PER_INPUT)
        .saturating_add(COT_RESERVE_TOKENS)
        .max(OUT_UNIT_TOKENS)
        .min(max_tokens)
        .min(headroom)
}

/// Calcola `budget_unit_text` (token), la dimensione massima di un'unità di
/// traduzione (STC-01). `budget_input = floor((n_ctx − out_unit) × (1 − margine))`,
/// da cui si sottraggono le stime di system minimale, riassunto compatto e
/// glossario selezionato. Con `n_ctx` grande (cloud) il risultato è enorme → una
/// sola unità = pagina intera (degradazione D2). Non scende mai sotto
/// [`MIN_BUDGET_UNIT_TEXT`]. Funzione pura, unit-testabile senza rete/DB.
fn compute_budget_unit_text(
    n_ctx: u32,
    out_unit: u32,
    system_est: u32,
    summary_est: u32,
    glossary_est: u32,
    margin: f64,
) -> u32 {
    let after_out = n_ctx.saturating_sub(out_unit) as f64;
    let budget_input = (after_out * (1.0 - margin)).floor().max(0.0) as u32;
    budget_input
        .saturating_sub(system_est)
        .saturating_sub(summary_est)
        .saturating_sub(glossary_est)
        .max(MIN_BUDGET_UNIT_TEXT)
}

/// Separa un'unità nel corpo (senza spazi di coda) e nel separatore finale (gli
/// spazi/newline di coda prodotti da [`split_into_units`]). Ricomponendo
/// `corpo_tradotto + separatore` si preservano i confini di paragrafo nel
/// riassemblaggio: il `concat` delle unità tradotte riproduce la struttura della
/// pagina.
fn split_unit_body_sep(unit: &str) -> (&str, &str) {
    let body = unit.trim_end();
    (body, &unit[body.len()..])
}

/// Inputs for a single page translation.
pub struct TranslateParams<'a> {
    pub document_id: i64,
    pub page_number: i64,
    pub target_language: &'a str,
    pub page_text: &'a str,
    pub model: &'a str,
    /// Upper bound on generated tokens for each call (ticket 02), taken from the
    /// active provider config. Local providers reserve output headroom within a
    /// small `n_ctx` (default 2048); cloud keeps a generous value (4096) so long
    /// pages are not truncated. Never the whole context window.
    pub max_tokens: u32,
    /// Context window (`n_ctx`) of the active provider (STC-07). Drives the
    /// budget model (STC-08): a small `n_ctx` (local ~4096) yields several small
    /// units with a selective glossary; a large `n_ctx` (cloud) makes the budget
    /// non-binding so the page becomes a single unit (degrade to the previous
    /// whole-page behaviour, D2).
    pub n_ctx: u32,
    /// Whether this translation should advance the percettore context (ticket
    /// 09): `true` on real navigation — persists the rolling summary and inserts
    /// glossary terms; `false` on **prefetch** (ticket 12) — caches only the
    /// `translated_text` so a page translated out of order never corrupts the
    /// summary/glossary. The current context is still used read-only as prompt
    /// input either way.
    pub update_context: bool,
    /// Liveness predicate checked at the top of every unit iteration (ticket
    /// 06, L3/L4): when `Some(f)` and `f()` returns `false`, the loop stops
    /// BEFORE starting a new unit/window (never mid an in-flight HTTP call)
    /// and the call returns `LlmError::Cancelled`. Used to stop a stale job
    /// (the user navigated to another page) or a prefetch that yielded the
    /// single local-provider slot to a higher-priority on-demand request.
    /// `None` means "always current" -- the default for every caller that
    /// does not need this coordination (e.g. existing tests, single-shot
    /// callers).
    pub is_current: Option<&'a dyn Fn() -> bool>,
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
    /// True when the per-page perceptor-update failed to advance the context
    /// (STC-10 observability, ticket 02): the strict JSON could not be parsed
    /// (even after the correction retry) or the call errored at transport level,
    /// so the rolling summary was NOT advanced for this page — while the page
    /// translation is still returned and cached (STC-10 invariant, never lost).
    /// Recovered glossary terms may still have been inserted (tolerant
    /// extraction). Always `false` on a cache hit and on prefetch (the percettore
    /// is not run) and on a full success. The frontend surfaces it as a
    /// non-intrusive "context not advanced for this page" note.
    pub perceptor_update_failed: bool,
}

// --- Split a unità (STC-02, cablato nel flusso da STC-08) --------------------
//
// Divide una pagina in unità di traduzione a livello di paragrafo entro un budget
// di token (`budget_unit_text` di STC-01). Riusa `est_tokens` per il
// dimensionamento e la ricostruzione a righe di `src/lib/pdfExtract.ts`. È il
// sostituto del vecchio split a soglia-char nel flusso di `translate_page`.

/// Split a page's reconstructed text into **paragraph-level units**, each within
/// `budget_tokens` (il `budget_unit_text` del Ticket 01). Sizing riusa
/// [`est_tokens`] con `ratio` (chars/token, default
/// [`crate::llm::DEFAULT_CHARS_PER_TOKEN`]).
///
/// Un *paragrafo* è un blocco delimitato da una **riga vuota** (una sequenza di
/// spazi bianchi che contiene due o più `\n`), coerente con la ricostruzione di
/// `src/lib/pdfExtract.ts`. Un paragrafo che sta nel budget diventa una singola
/// unità; un paragrafo che lo eccede ripiega su uno **split a livello di frase**
/// (frasi/righe impacchettate fino al budget). Ogni unità porta con sé il proprio
/// separatore di paragrafo, quindi `split_into_units(t, b, r).concat() == t`
/// esattamente: nessun testo perso, ordine preservato.
///
/// Casi limite: input vuoto/whitespace → una sola unità uguale all'input; liste
/// e righe (unite da `\n` singolo come fa `pdfExtract`) restano nello stesso
/// paragrafo e vengono impacchettate; una singola frase più grande del budget
/// resta un "atomo" non divisibile (unica eccezione al vincolo di budget).
///
/// Cablata nel flusso live da STC-08 (`translate_page`): sostituisce lo split a
/// soglia-char (`split_into_chunks`) come unità di traduzione del percorso a
/// budget. Con un budget ampio (cloud) restituisce una sola unità = pagina intera
/// (degradazione D2).
pub fn split_into_units(text: &str, budget_tokens: u32, ratio: f64) -> Vec<String> {
    // Pagina vuota / solo spazi: una sola unità che riproduce l'input.
    if text.trim().is_empty() {
        return vec![text.to_string()];
    }

    let mut units: Vec<String> = Vec::new();
    for (body, sep) in split_paragraphs(text) {
        if est_tokens(&body, ratio) <= budget_tokens {
            // Il paragrafo sta nel budget: una sola unità (corpo + separatore).
            units.push(format!("{body}{sep}"));
        } else {
            // Paragrafo troppo grande: ripiega su frasi/righe impacchettate fino
            // al budget. Il separatore di coda viaggia sull'ultima unità così il
            // round-trip resta esatto.
            let mut pieces = pack_sentences(&body, budget_tokens, ratio);
            match pieces.last_mut() {
                Some(last) => last.push_str(&sep),
                None => pieces.push(sep),
            }
            units.extend(pieces);
        }
    }
    units
}

/// Suddivide `text` in coppie `(corpo, separatore)`, dove il separatore è una
/// riga vuota (run di spazi con ≥2 `\n`). La concatenazione di tutti i
/// `corpo + separatore` riproduce esattamente `text`. Whitespace non separatore
/// (a capo singolo, spazi) resta dentro il corpo.
fn split_paragraphs(text: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut body_start = 0usize;
    let mut i = 0usize;
    while i < n {
        if chars[i].is_whitespace() {
            // Estende il run di spazi e conta i newline.
            let ws_start = i;
            let mut newlines = 0usize;
            let mut j = i;
            while j < n && chars[j].is_whitespace() {
                if chars[j] == '\n' {
                    newlines += 1;
                }
                j += 1;
            }
            if newlines >= 2 {
                // Riga vuota → confine di paragrafo.
                let body: String = chars[body_start..ws_start].iter().collect();
                let sep: String = chars[ws_start..j].iter().collect();
                out.push((body, sep));
                body_start = j;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if body_start < n {
        let body: String = chars[body_start..n].iter().collect();
        out.push((body, String::new()));
    }
    out
}

/// Impacchetta le frasi di `body` in unità entro `budget` token, riusando
/// [`est_tokens`]. Una singola frase più grande del budget resta un'unità a sé
/// (atomo non divisibile). `pack_sentences(b, ...).concat() == b`.
fn pack_sentences(body: &str, budget: u32, ratio: f64) -> Vec<String> {
    let mut units: Vec<String> = Vec::new();
    let mut cur = String::new();
    for piece in split_sentences(body) {
        if cur.is_empty() {
            cur = piece;
        } else if est_tokens(&format!("{cur}{piece}"), ratio) <= budget {
            cur.push_str(&piece);
        } else {
            units.push(std::mem::take(&mut cur));
            cur = piece;
        }
    }
    if !cur.is_empty() {
        units.push(cur);
    }
    units
}

/// Impacchetta unità adiacenti prodotte da [`split_into_units`] in **finestre**
/// entro `budget` token, greedy e deterministico. Prototipata nel ticket 02 e
/// **cablata** nel flusso di [`translate_page`] dal ticket 04, con budget
/// [`PACK_TARGET_TOKENS`] clampato al `budget_unit_text` corrente (L1).
/// Motivazione (misure ticket 01):
/// il costo dominante sul provider locale è il CoT del modello, pagato **per
/// chiamata** (~500 token scartati); ridurre il numero di chiamate riduce la
/// latenza quasi linearmente, mentre il prefill è prefix-cached e costa ~0.
///
/// Proprietà (stesse garanzie di [`split_into_units`]):
/// - round-trip esatto: `pack_units(us, b, r).concat() == us.concat()`;
/// - nessuna finestra oltre `budget`, salvo l'unità singola già oltre budget
///   (atomo indivisibile, stessa eccezione dello split);
/// - ordine preservato; i separatori di paragrafo restano dentro le finestre.
///
/// La stabilità della finestra dipende SOLO da `(unità, budget, ratio)`: a parità
/// di budget il packing è riproducibile. Vedi i test `pack_*` per l'analisi dei
/// cache-miss quando il budget cambia (input alla decisione L1 del grilling 03).
pub fn pack_units(units: Vec<String>, budget: u32, ratio: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    for u in units {
        match cur.as_mut() {
            None => cur = Some(u),
            Some(acc) if est_tokens(format!("{acc}{u}").trim(), ratio) <= budget => {
                acc.push_str(&u);
            }
            Some(_) => {
                out.push(cur.take().expect("cur is Some in this branch"));
                cur = Some(u);
            }
        }
    }
    if let Some(acc) = cur {
        out.push(acc);
    }
    out
}

/// Spezza `text` in frasi preservando ogni carattere (`concat == text`). Un
/// confine di frase è `.`/`!`/`?` (con eventuali chiusure) seguito da spazio o
/// fine testo, oppure un `\n` (righe/liste; `pdfExtract` unisce le righe a capo
/// con `\n`). Gli spazi di coda sono assorbiti nella frase precedente così il
/// confine non va perso.
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < n {
        let c = chars[i];
        if c == '.' || c == '!' || c == '?' {
            // Assorbe punteggiatura terminale e caratteri di chiusura ripetuti.
            let mut j = i + 1;
            while j < n
                && matches!(
                    chars[j],
                    '.' | '!' | '?' | '"' | '\'' | ')' | ']' | '»' | '”' | '’'
                )
            {
                j += 1;
            }
            // Confine reale solo se seguito da spazio o fine (evita "3.14").
            if j >= n || chars[j].is_whitespace() {
                while j < n && chars[j].is_whitespace() {
                    j += 1;
                }
                out.push(chars[start..j].iter().collect());
                start = j;
                i = j;
                continue;
            }
            i = j;
            continue;
        }
        if c == '\n' {
            // A capo forzato: confine (liste, poesia, righe di pdfExtract).
            out.push(chars[start..=i].iter().collect());
            start = i + 1;
            i += 1;
            continue;
        }
        i += 1;
    }
    if start < n {
        out.push(chars[start..n].iter().collect());
    }
    out
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

// --- Cache per-unità (ticket 09) ---------------------------------------------
//
// Livello di ripresa a granularità di unità, sopra la cache di pagina. La pagina
// resta il segnale "fatta" (percettore avanzato una volta); questa cache evita di
// ritradurre le unità già completate quando una pagina si interrompe a metà (una
// unità in errore/timeout, oppure il percettore che fallisce dopo le unità).

/// Hash stabile (FNV-1a 64-bit) del corpo di un'unità, reso in esadecimale. Serve
/// a invalidare la cache per-unità quando il testo sorgente cambia: corpo diverso
/// → hash diverso → MISS → ritraduzione della sola unità cambiata. FNV-1a è
/// deterministico tra build e versioni del compilatore (a differenza di
/// `DefaultHasher`), quindi la cache persistita resta valida tra riavvii.
fn source_hash(body: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // offset basis FNV-1a 64-bit
    for b in body.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime 64-bit
    }
    format!("{hash:016x}")
}

/// Legge la traduzione cachata di una singola unità, valida solo se ancora
/// coerente col testo sorgente corrente. Ritorna `Some(translated_text)` soltanto
/// quando esiste una riga con lo stesso `source_hash` (invalidazione per cambio
/// testo) e per la stessa `target_language` (una lingua diversa non trova riga →
/// MISS). Il `translated_text` memorizzato è il **corpo** tradotto senza
/// separatore: il chiamante riappende il separatore di paragrafo in fase di
/// riassemblaggio.
fn unit_cache_lookup(
    conn: &Connection,
    document_id: i64,
    page_number: i64,
    unit_index: i64,
    target_language: &str,
    hash: &str,
) -> Result<Option<String>, LlmError> {
    conn.query_row(
        "SELECT translated_text FROM unit_translations
          WHERE document_id = ?1 AND page_number = ?2 AND unit_index = ?3
            AND target_language = ?4 AND source_hash = ?5",
        params![document_id, page_number, unit_index, target_language, hash],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| LlmError::Storage(e.to_string()))
}

/// Scrive (UPSERT sulla UNIQUE key) la traduzione di una singola unità appena
/// completata. Chiamata **subito dopo il successo dell'unità e PRIMA del
/// percettore**: è la vittoria di robustezza del ticket 09 — se il percettore (o
/// un'unità successiva) fallirà, le unità già tradotte restano in cache e un retry
/// ritradurrà solo le mancanti. L'UPSERT sovrascrive una riga precedente con lo
/// stesso indice ma hash diverso (testo pagina cambiato → riscrittura pulita).
fn unit_cache_insert(
    conn: &Connection,
    p: &TranslateParams,
    unit_index: i64,
    hash: &str,
    translated_text: &str,
) -> Result<(), LlmError> {
    conn.execute(
        "INSERT INTO unit_translations
             (document_id, page_number, unit_index, target_language, source_hash, translated_text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(document_id, page_number, unit_index, target_language) DO UPDATE SET
             source_hash     = excluded.source_hash,
             translated_text = excluded.translated_text,
             created_at      = excluded.created_at",
        params![
            p.document_id,
            p.page_number,
            unit_index,
            p.target_language,
            hash,
            translated_text
        ],
    )
    .map_err(|e| LlmError::Storage(e.to_string()))?;
    Ok(())
}

/// Rimuove le righe di unità con `unit_index >= unit_count` per questa
/// pagina/lingua: quando il testo pagina si accorcia (meno unità) le vecchie unità
/// in coda restano orfane e non vanno mai servite. Idempotente (no-op quando non
/// c'è nulla da potare); non serve mai una traduzione stale per hash comunque.
fn unit_cache_prune(
    conn: &Connection,
    p: &TranslateParams,
    unit_count: i64,
) -> Result<(), LlmError> {
    conn.execute(
        "DELETE FROM unit_translations
          WHERE document_id = ?1 AND page_number = ?2 AND target_language = ?3
            AND unit_index >= ?4",
        params![p.document_id, p.page_number, p.target_language, unit_count],
    )
    .map_err(|e| LlmError::Storage(e.to_string()))?;
    Ok(())
}

/// Outcome of the per-page perceptor-update call at the [`translate_page`]
/// boundary (STC-10, resilience/observability, ticket 02).
enum PerceptorUpdateResult {
    /// Strict JSON parsed: both the updated summary and the new glossary terms
    /// are available, so the context advances FULLY (summary + glossary).
    Full {
        output: crate::llm::PerceptorUpdateOutput,
        usage: Option<Usage>,
    },
    /// Strict JSON could NOT be parsed (even after the correction retry), but the
    /// `new_glossary_terms` array was tolerantly recovered. The summary CANNOT be
    /// advanced, yet the glossary can still grow: the terms are inserted anyway,
    /// decoupled from the summary (ticket 02, criterion b). `terms` may be empty
    /// (nothing recoverable) — either way this is a perceptor FAILURE for the UI
    /// signal, because the summary did not advance.
    Recovered {
        terms: Vec<GlossaryTerm>,
        usage: Option<Usage>,
    },
}

/// Call the client for the **lean perceptor-update** (STC-10, D5) and parse the
/// response with the layered fallback, including the single correction retry
/// (layer c). The lean contract asks ONLY for `{updated_summary,
/// new_glossary_terms}` (no `translated_text`): the model does not re-translate
/// the page, so the output stays small and budget-safe (no EC08 from a maxi
/// re-translation).
///
/// Resilience (ticket 02): when the strict parse fails on BOTH the first answer
/// and the correction retry, instead of returning an error that discards
/// everything, it attempts a **tolerant extraction** of just the glossary terms
/// (`new_glossary_terms`) from either answer and returns
/// [`PerceptorUpdateResult::Recovered`] — so a small local model that can't emit
/// a conformant `updated_summary` still contributes glossary terms. A
/// **transport** failure (network / EC08 empty+length / no content) has nothing
/// to extract from and is surfaced as `Err`; the caller treats both the `Err` and
/// the `Recovered` outcomes as "context not advanced" and signals the UI.
fn complete_and_parse_perceptor_update(
    client: &dyn ChatClient,
    req: ChatRequest,
) -> Result<PerceptorUpdateResult, LlmError> {
    // `complete_with_fallback` adds the model-agnostic param-relaxation retry
    // (research §2, bug #1) around the transport: a 404 "No endpoints found" or
    // a 400 unsupported-parameter downgrades the body (drop provider →
    // response_format → temperature) instead of failing outright. It returns the
    // body that worked, so we never re-probe the rejected param below.
    let (resp, working) = complete_with_fallback(client, &req)?;
    let content = resp.content()?.to_string();
    let usage = resp.usage.clone();

    if let Ok(out) = crate::llm::parse_perceptor_update(&content) {
        return Ok(PerceptorUpdateResult::Full { output: out, usage });
    }

    // (c) one correction retry: echo the bad answer, demand pure JSON. Build it
    // on the *working* (possibly degraded) body so the retry does not re-send the
    // param the model already rejected.
    let mut retry_req = working;
    retry_req.messages.push(ChatMessage::assistant(content.clone()));
    retry_req.messages.push(ChatMessage::user(CORRECTION_PROMPT));

    // A transport failure on the retry itself must not drop terms already
    // recoverable from the first answer: fall back to tolerant extraction on
    // `content` rather than propagating the error (resilience over strictness).
    let (content2, usage2) = match complete_with_fallback(client, &retry_req) {
        Ok((resp2, _working2)) => match resp2.content() {
            Ok(c) => (Some(c.to_string()), resp2.usage.clone().or(usage.clone())),
            Err(_) => (None, usage.clone()),
        },
        Err(_) => (None, usage.clone()),
    };

    // Strict parse of the corrected answer → full success.
    if let Some(c2) = &content2 {
        if let Ok(out) = crate::llm::parse_perceptor_update(c2) {
            return Ok(PerceptorUpdateResult::Full { output: out, usage: usage2 });
        }
    }

    // (d) tolerant recovery: extract the glossary terms from the corrected answer
    // first (freshest), then the original. Empty when nothing is recoverable.
    let mut terms = content2
        .as_deref()
        .map(crate::llm::extract_glossary_terms_tolerant)
        .unwrap_or_default();
    if terms.is_empty() {
        terms = crate::llm::extract_glossary_terms_tolerant(&content);
    }
    Ok(PerceptorUpdateResult::Recovered { terms, usage: usage2 })
}

/// Call the client for a **translate-only** unit (STC-08, D5) and extract the
/// translated text with the minimal, robust contract ([`parse_translation`]:
/// tiny JSON `{translated_text}` or plain text). Keeps the model-agnostic
/// param-relaxation fallback ([`complete_with_fallback`]) and surfaces EC08
/// (`OutputBudgetExhausted`, via [`crate::llm::ChatResponse::content`]) and any
/// other transport error unchanged. Returns the translation, the provider
/// `usage` when reported, and the **working request body** that succeeded so
/// later units of the same page start already-degraded (no repeated re-probe).
///
/// Unlike the percettore call there is **no** JSON-correction retry: the
/// translate-only contract always parses (plain-text fallback), so a malformed
/// answer becomes the literal translation rather than triggering a second call.
///
/// Uses [`crate::llm::ChatResponse::content_complete`] (not `content`) so a
/// **truncated** completion — non-empty text cut off at `finish_reason ==
/// "length"` — is refused as [`LlmError::OutputTruncated`] instead of being
/// accepted as a complete translation (ticket 11). The per-unit loop in
/// [`translate_page`] catches that and retries with a larger budget; the empty +
/// length case still surfaces as EC08 exactly as before.
fn complete_and_parse_translation(
    client: &dyn ChatClient,
    req: ChatRequest,
) -> Result<(String, Option<Usage>, ChatRequest), LlmError> {
    let (resp, working) = complete_with_fallback(client, &req)?;
    // EC08 (empty+length) / empty content / TRUNCATED (non-empty+length) all
    // surface here before we ever try to parse — a truncated unit must not be
    // accepted as complete (ticket 11).
    let content = resp.content_complete()?.to_string();
    let usage = resp.usage.clone();
    Ok((parse_translation(&content), usage, working))
}

/// Map a storage error into [`LlmError::Storage`].
fn storage<E: std::fmt::Display>(e: E) -> LlmError {
    LlmError::Storage(e.to_string())
}

/// Translate a page with the **budget-aware multi-call pipeline** (STC-08,
/// SPECIFICATION §3.2/§3.3/§4.4, UC02; decisions D1-D6).
///
/// Flow: a cache hit returns the stored `translated_text` immediately and does
/// **not** re-run the percettore (no summary/glossary rewrite for cached pages).
/// On a miss:
/// 1. the current `rolling_summary` and glossary are loaded (read-only for the
///    whole page, D6);
/// 2. `budget_unit_text` is derived from the provider `n_ctx` and the per-unit
///    output cap [`OUT_UNIT_TOKENS`] (STC-01) and the page is split into units
///    with [`split_into_units`] (paragraph, sentence fallback; STC-02), then
///    adjacent units are **packed into fixed-size windows** with [`pack_units`]
///    at [`PACK_TARGET_TOKENS`] clamped to the budget (ticket 04, L1) — the
///    per-call unit is the packed window. A large `n_ctx` (cloud) makes the
///    budget non-binding → **one unit = whole page** (degrade to the previous
///    behaviour, D2);
/// 3. each unit is translated with a **minimal translate-only call** — a tiny
///    system prompt, the compact read-only summary and only the glossary
///    **selected** for that unit ([`glossary::select_glossary`], locked-first,
///    STC-03), requested with a small `max_tokens` (D5). The degrade ladder and
///    EC08 handling stay in force per unit;
/// 4. the translated units are reassembled **in order** (separators preserved);
/// 5. only on real navigation (`update_context`), a single **perceptor-update**
///    call per page (D6) uses the LEAN contract (STC-10) — it asks ONLY for the
///    updated summary (EC05 compression) and the new glossary terms, with a
///    COMPACT selected glossary and NO re-translation of the page (budget-safe).
///
/// Afterwards, on REAL navigation (`update_context`) the recomposed translation
/// is cached page-level, `sessions.rolling_summary` is updated once and the new
/// glossary terms are inserted deduped (locked terms untouched) — but the context
/// advances ONLY when the perceptor-update succeeded: a failed perceptor-update is
/// swallowed (soft log), the page translation is still cached and returned, and
/// the context simply does not advance (STC-10 resilience). On prefetch
/// (`update_context == false`) the percettore step is skipped AND the page-level
/// cache is NOT written (ticket 01, option B): a prefetch warms only the per-unit
/// cache (STC-09). This keeps the real navigation a page miss so the percettore
/// runs and the glossary grows, while the warmed per-unit cache still serves the
/// units with no re-translation (latency unchanged).
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
                // A cache hit does not re-run the percettore, so there is no
                // update to fail (STC-10): never a false alarm.
                perceptor_update_failed: false,
            });
        }
    }

    // Load the percettore context ONCE for the whole page (D6: the same summary
    // version is passed read-only to every unit; it is advanced afterwards, once).
    let summary_limit = settings::get_summary_token_limit(conn).map_err(storage)?;
    let mut rolling_summary = documents::get_rolling_summary(conn, p.document_id).map_err(storage)?;
    let entries = glossary::list_glossary(conn, p.document_id).map_err(storage)?;
    // The full locked block is estimated below only to SIZE the unit budget; no
    // call ever ships the whole glossary. Both the per-unit translate calls
    // (STC-03) and the per-page perceptor call (STC-10) use the SELECTED subset.
    let (locked_all, _unlocked_all) = glossary::render_locked_unlocked(&entries);

    // --- Budget model (STC-01): size the translation units from n_ctx --------
    // Ratio: the stable default heuristic (calibration is persisted for telemetry
    // only), so the budget is deterministic and unit-testable. The ~15% margin
    // absorbs the chars/token approximation.
    let ratio = DEFAULT_CHARS_PER_TOKEN;
    let out_unit = OUT_UNIT_TOKENS;
    let system_est = est_tokens(&crate::llm::build_translate_only_system_prompt(), ratio);
    let summary_est = est_tokens(&rolling_summary, ratio);
    // Glossary reservation: the locked block is ALWAYS included in every unit
    // prompt (estimated exactly), plus a fixed allowance for the capped unlocked
    // selection (D4). Over-reserving only makes units smaller (safer), never
    // larger than the context allows.
    let glossary_est = est_tokens(&locked_all, ratio) + GLOSSARY_UNLOCKED_RESERVE_TOKENS;
    let budget_unit_text =
        compute_budget_unit_text(p.n_ctx, out_unit, system_est, summary_est, glossary_est, BUDGET_MARGIN);

    // Split the page into budget-sized units (STC-02), then PACK adjacent units
    // into fixed-size windows (ticket 04, L1): the per-call unit becomes the
    // packed window, so a dense page costs 1-2 LLM calls instead of one per
    // paragraph (the local model's ~500-token CoT is paid per call). The pack
    // budget is the FIXED constant, clamped to `budget_unit_text` only when the
    // dynamic budget is tighter (L1 clamp) — so the windows depend only on the
    // page text and the constant, keeping the per-unit cache stable across
    // repacks. With a single-paragraph page and a large n_ctx (cloud) this
    // still yields ONE unit = whole page → the previous whole-page behaviour
    // (degrade, D2).
    let units = pack_units(
        split_into_units(p.page_text, budget_unit_text, ratio),
        PACK_TARGET_TOKENS.min(budget_unit_text),
        ratio,
    );

    let mut translated_units: Vec<String> = Vec::with_capacity(units.len());
    let mut total_tokens_sum: i64 = 0;
    let mut saw_usage = false;
    let mut prompt_chars_sum: usize = 0;
    let mut prompt_tokens_sum: i64 = 0;
    // The request shape discovered to work for this page: once the fallback
    // strips a param a model rejects, later units start already-degraded so they
    // don't each pay the same failed 404 (bug #1 follow-up). `None` until the
    // first unit returns its working body.
    let mut working_shape: Option<ChatRequest> = None;

    for (idx, unit) in units.iter().enumerate() {
        // Stale-job cancellation (ticket 06, L3/L4): checked at the boundary
        // between units, never mid an in-flight HTTP call. A `false` here means
        // either the page is no longer the current one (navigation happened) or
        // this prefetch yielded the local-provider slot to an on-demand request.
        // Units already translated (and cached) in previous iterations of THIS
        // call are untouched -- returning `Err` here simply skips the
        // perceptor-update and page-level cache write that only happen on the
        // success path below (no ad-hoc rollback needed, ticket 09 partial-cache
        // semantics already cover this).
        if let Some(is_current) = p.is_current {
            if !is_current() {
                return Err(LlmError::Cancelled);
            }
        }

        // Preserve the paragraph separator across translation: translate the body
        // and re-append the trailing separator so the reassembly keeps structure.
        let (body, sep) = split_unit_body_sep(unit);
        if body.trim().is_empty() {
            // A whitespace-only unit (e.g. a blank page): no model call, keep it
            // verbatim so the round-trip holds. Non va in cache (niente da
            // ritradurre) e non altera l'allineamento degli indici.
            translated_units.push(unit.clone());
            continue;
        }

        // Cache per-unità (STC-09): se questo indice è già tradotto per lo stesso
        // testo (hash) e la stessa lingua → HIT, nessuna chiamata al modello. È il
        // livello di ripresa: dopo un'interruzione a metà pagina, solo le unità
        // mancanti vengono ritradotte.
        let hash = source_hash(body);
        if let Some(cached) = unit_cache_lookup(
            conn,
            p.document_id,
            p.page_number,
            idx as i64,
            p.target_language,
            &hash,
        )? {
            translated_units.push(format!("{cached}{sep}"));
            continue;
        }

        // Only the glossary SELECTED for this unit (D4/STC-03, locked-first): the
        // whole glossary never reaches these prompts (≈98% fewer tokens).
        let selected = glossary::select_glossary(body, &entries, Some(UNLOCKED_GLOSSARY_CAP));
        let (locked, unlocked) = glossary::render_locked_unlocked(&selected);

        let messages = build_translate_only_messages(
            p.target_language,
            body,
            &rolling_summary,
            &locked,
            &unlocked,
        );
        prompt_chars_sum += messages.iter().map(|m| m.content.chars().count()).sum::<usize>();

        // Context headroom for this unit: `n_ctx − prompt − safety`, the ceiling
        // that keeps `prompt + output ≤ n_ctx` (EC08 guard). It also caps how far
        // the truncation-retry may grow the output budget below.
        let prompt_est: u32 = messages.iter().map(|m| est_tokens(&m.content, ratio)).sum();
        let headroom = p
            .n_ctx
            .saturating_sub(prompt_est.saturating_add(OUTPUT_HEADROOM_SAFETY_TOKENS))
            .max(out_unit);

        // INITIAL per-unit output cap (the retry below may grow it on truncation).
        // A SINGLE unit (cloud degrade / short page) starts at the page
        // `max_tokens`, so the common non-truncating case is byte-for-byte
        // equivalent to the previous whole-page flow (D2, no cloud regression).
        // FINDING 2 decision (ticket 11): the truncation-detect + retry loop stays
        // active for ALL paths — single-unit/cloud included — because a truncated
        // page is strictly worse than a retried one; equivalence is preserved for
        // the common case by starting at `p.max_tokens`, and truncation there now
        // triggers a bounded retry (grown budget) rather than accepting a partial.
        // On the multi-unit path the cap scales with the packed window's body
        // (translation may expand, e.g. EN→IT) PLUS the CoT reserve (ticket 04:
        // ~500 reasoning tokens are emitted before the translation on the local
        // model, so 2× input alone would truncate a full 512-token window), is
        // never below `out_unit`, and is always bounded by the remaining context
        // window so `prompt + output ≤ n_ctx` — a large window on cloud is NOT
        // truncated (huge headroom), while local windows stay bounded
        // (~1536 ≤ max_tokens 2048 with the local defaults).
        let initial_max_tokens = if units.len() == 1 {
            p.max_tokens
        } else {
            window_output_cap(est_tokens(body, ratio), p.max_tokens, headroom)
        };

        // Translate the unit, RETRYING a truncated completion with a larger output
        // budget (ticket 11). A non-empty answer cut off at `finish_reason ==
        // "length"` (OutputTruncated) is NOT accepted — it would drop half a
        // paragraph — so we double `max_tokens`, bounded by `headroom`, up to
        // `TRUNCATION_MAX_RETRIES` times. If it still truncates when the budget
        // can no longer grow, the partial is refused with an actionable EC08
        // (OutputBudgetExhausted). The whole loop runs BEFORE `unit_cache_insert`,
        // so a truncated partial is never written to the per-unit cache and a retry
        // genuinely re-translates (criterion c).
        let mut unit_max_tokens = initial_max_tokens;
        let mut truncation_retries = 0u32;
        let (translated, usage, working) = loop {
            let mut req =
                build_translate_only_request(p.model, messages.clone(), unit_max_tokens);
            // Reuse the optional-param shape discovered on a previous unit so we
            // don't re-probe a param the model already rejected this page.
            if let Some(shape) = &working_shape {
                req.temperature = shape.temperature;
                req.response_format = shape.response_format.clone();
                req.provider = shape.provider.clone();
            }
            match complete_and_parse_translation(client, req) {
                Ok(ok) => break ok,
                Err(LlmError::OutputTruncated(reason)) => {
                    let grown = unit_max_tokens.saturating_mul(2).min(headroom);
                    if grown > unit_max_tokens && truncation_retries < TRUNCATION_MAX_RETRIES {
                        truncation_retries += 1;
                        unit_max_tokens = grown;
                    } else {
                        // Cannot grow further: refuse the partial (never cache it).
                        return Err(LlmError::OutputBudgetExhausted(reason));
                    }
                }
                Err(e) => return Err(e),
            }
        };
        working_shape = Some(working);

        // Scrittura IMMEDIATA della cache per-unità, PRIMA del percettore: se il
        // percettore (o un'unità successiva) fallirà più sotto, questa unità resta
        // salva e un retry non la ritradurrà (robustezza chiave STC-09; colma la
        // lacuna di STC-08 dove un fallimento del percettore scartava le
        // traduzioni riuscite).
        unit_cache_insert(conn, p, idx as i64, &hash, &translated)?;

        translated_units.push(format!("{translated}{sep}"));

        if let Some(u) = usage {
            saw_usage = true;
            total_tokens_sum += u.total_tokens;
            prompt_tokens_sum += u.prompt_tokens;
        }
    }

    // Poda le unità orfane (pagina accorciata rispetto a una visita precedente):
    // mai servite, rimosse per igiene della cache di ripresa.
    unit_cache_prune(conn, p, units.len() as i64)?;

    // Reassemble the translated units in order (separators are carried on each
    // unit, so `concat` reproduces the page structure).
    let translated_text = translated_units.concat();

    // --- Perceptor-update: ONCE per page, real navigation only (D5/D6/D10) ----
    // Lean contract (STC-10): ask ONLY for the updated summary + new glossary
    // terms — the model does NOT re-translate the page (that already came from the
    // units), so the output stays small and budget-safe (this was the last big
    // call that blew past a small window → EC08). Input is compact too: only the
    // glossary SELECTED for the whole page (union of per-unit relevance), never
    // the entire glossary. Skipped entirely on prefetch (ticket 12), as before.
    //
    // Resilienza (STC-10, fix chiave lato utente): un fallimento del
    // perceptor-update NON deve scartare la traduzione già prodotta. La chiamata è
    // avvolta in un `match` (niente `?` che aborta): su errore si logga in modo
    // soft, il summary/glossario NON avanza, ma la pagina viene comunque cachata e
    // restituita (coerente con la cache per-unità di STC-09).
    let mut new_terms: Vec<GlossaryTerm> = Vec::new();
    let mut summary_advanced = false;
    // STC-10 observability (ticket 02): true when the context could NOT be
    // advanced for this page (strict parse failed or the call errored), even if
    // glossary terms were tolerantly recovered. Surfaced to the UI.
    let mut perceptor_update_failed = false;
    if p.update_context {
        // Compact glossary for the perceptor prompt: only the terms relevant to
        // the page (locked-first, STC-03), never the whole glossary.
        let page_selected =
            glossary::select_glossary(p.page_text, &entries, Some(UNLOCKED_GLOSSARY_CAP));
        let (locked_sel, unlocked_sel) = glossary::render_locked_unlocked(&page_selected);

        let compress = needs_compression(&rolling_summary, summary_limit);
        let messages = build_perceptor_update_messages(
            p.target_language,
            p.page_text,
            &rolling_summary,
            &locked_sel,
            &unlocked_sel,
            compress,
            summary_limit,
        );
        let perceptor_prompt_chars: usize =
            messages.iter().map(|m| m.content.chars().count()).sum();

        let req = build_perceptor_update_request(p.model, messages, p.max_tokens);
        match complete_and_parse_perceptor_update(client, req) {
            Ok(PerceptorUpdateResult::Full { output, usage }) => {
                // Only advance the summary when the perceptor actually succeeded.
                rolling_summary = output.updated_summary;
                new_terms = output.new_glossary_terms;
                summary_advanced = true;
                prompt_chars_sum += perceptor_prompt_chars;
                if let Some(u) = usage {
                    saw_usage = true;
                    total_tokens_sum += u.total_tokens;
                    prompt_tokens_sum += u.prompt_tokens;
                }
            }
            Ok(PerceptorUpdateResult::Recovered { terms, usage }) => {
                // Partial: strict JSON unparseable, so the summary does NOT
                // advance — but glossary terms were tolerantly recovered and are
                // inserted anyway (decoupled from `summary_advanced`, ticket 02).
                // Still a failure for the UI signal (context not fully advanced).
                new_terms = terms;
                perceptor_update_failed = true;
                if let Some(u) = usage {
                    saw_usage = true;
                    total_tokens_sum += u.total_tokens;
                    prompt_tokens_sum += u.prompt_tokens;
                }
                eprintln!(
                    "[perceptor] update parziale document_id={} page={} lang={}: summary NON avanzato, {} termini recuperati",
                    p.document_id,
                    p.page_number,
                    p.target_language,
                    new_terms.len()
                );
            }
            Err(e) => {
                // Fallimento soft: la traduzione della pagina resta valida e viene
                // cachata sotto; il contesto (summary + glossario) semplicemente
                // non avanza per questa pagina. Il fallimento è ora osservabile
                // dall'utente via `perceptor_update_failed` (ticket 02), non solo
                // questo log.
                perceptor_update_failed = true;
                eprintln!(
                    "[perceptor] update fallito document_id={} page={} lang={}: {}",
                    p.document_id,
                    p.page_number,
                    p.target_language,
                    e.user_message()
                );
            }
        }
    }

    // Persist (ticket 01, opzione B). La cache di PAGINA si scrive SOLO su
    // navigazione reale (`update_context`), non sul prefetch: un prefetch scalda
    // esclusivamente la cache per-unità (STC-09). Così alla navigazione reale la
    // pagina è un MISS di pagina, la pipeline prosegue fino al percettore (il
    // glossario cresce) e le unità sono servite dalla cache per-unità (nessuna
    // ri-traduzione, latenza invariata). Sulla navigazione reale la riga di
    // pagina viene scritta ANCHE quando il perceptor-update è fallito: la
    // traduzione riuscita non va mai persa (STC-10). Il contesto percettore
    // (summary + glossario) avanza SOLO se il perceptor-update è riuscito; un
    // prefetch non deve mutare il contesto fuori ordine (ticket 12).
    if p.update_context {
        cache_insert(conn, p, &translated_text)?;
        // The summary advances ONLY on a full perceptor success.
        if summary_advanced {
            documents::set_rolling_summary(conn, p.document_id, &rolling_summary).map_err(storage)?;
        }
        // Glossary insertion is DECOUPLED from the summary (ticket 02, criterion
        // b): terms come from a full success OR from tolerant recovery on a
        // partial failure, so the glossary can grow even when the summary could
        // not advance. `insert_terms_deduped` leaves locked terms untouched and
        // dedups against existing rows, so this is safe either way. A no-op when
        // `new_terms` is empty.
        if !new_terms.is_empty() {
            glossary::insert_terms_deduped(conn, p.document_id, &new_terms, p.page_number)
                .map_err(storage)?;
        }

        // Calibrate the chars/token ratio from real usage (research §3) — stored
        // for cost telemetry; the budget/`needs_compression` keep the stable
        // default ratio.
        if let Some(ratio) = calibrate_chars_per_token(prompt_chars_sum, prompt_tokens_sum) {
            let _ =
                settings::set_setting(conn, settings::CHARS_PER_TOKEN_KEY, &format!("{ratio:.4}"));
        }
    }

    let total_tokens = if saw_usage { Some(total_tokens_sum) } else { None };
    if let Some(tokens) = total_tokens {
        // Cost telemetry (NFR04): logged rather than a schema column for the MVP.
        eprintln!(
            "[usage] document_id={} page={} lang={} units={} prefetch={} total_tokens={}",
            p.document_id,
            p.page_number,
            p.target_language,
            units.len(),
            !p.update_context,
            tokens
        );
    }

    Ok(TranslationResult {
        translated_text,
        from_cache: false,
        total_tokens,
        // Only report the advanced summary when it was actually persisted; a
        // prefetch reports `None` because it did not touch the context, and a
        // failed perceptor-update reports `None` too (context not advanced, STC-10).
        updated_summary: if p.update_context && summary_advanced {
            Some(rolling_summary)
        } else {
            None
        },
        // Only meaningful on real navigation (the percettore runs); a prefetch or
        // cache hit never sets it. True when the summary could not be advanced
        // (STC-10 observability, ticket 02).
        perceptor_update_failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatResponse, GlossaryTerm, PerceptoreOutput};
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

    /// Default params: a large `n_ctx` (cloud) so a single-paragraph page is ONE
    /// unit — the budget-aware pipeline degrades to the previous whole-page flow
    /// (D2). Most legacy tests use short single-paragraph pages → 1 translate
    /// call + 1 perceptor call on real navigation.
    fn params<'a>(text: &'a str) -> TranslateParams<'a> {
        TranslateParams {
            document_id: 1,
            page_number: 3,
            target_language: "it",
            page_text: text,
            model: "openai/gpt-4o",
            max_tokens: 4096,
            n_ctx: 128_000,
            update_context: true,
            is_current: None,
        }
    }

    /// Params emulating a small local context (`n_ctx = 4096`): the budget binds,
    /// so a multi-paragraph page splits into several translate-only units.
    fn params_small(text: &str) -> TranslateParams<'_> {
        TranslateParams {
            document_id: 1,
            page_number: 3,
            target_language: "it",
            page_text: text,
            model: "local-model",
            max_tokens: 2048,
            n_ctx: 4096,
            update_context: true,
            is_current: None,
        }
    }

    /// Un paragrafo "grande" (~340 token) con una frase-guida in testa. Col
    /// packing cablato (ticket 04) due paragrafi piccoli collasserebbero in UNA
    /// finestra da [`PACK_TARGET_TOKENS`]: due paragrafi così superano insieme
    /// i 512 token, quindi restano finestre separate e le fixture multi-unità
    /// continuano a esercitare il percorso multi-finestra. Il riempimento non
    /// contiene termini di glossario né confini di paragrafo.
    fn big_para(lead: &str) -> String {
        format!(
            "{lead} {}",
            "Testo di riempimento neutro che tiene il paragrafo sopra la soglia di packing. "
                .repeat(17)
        )
        .trim_end()
        .to_string()
    }

    /// A two-paragraph page (blank-line separated) → two units under any budget
    /// AND two packed windows (each paragraph is ~340 tokens, so the pair
    /// exceeds `PACK_TARGET_TOKENS`). Each paragraph mentions a different
    /// glossary term so per-unit selection is observable.
    fn two_paragraphs() -> String {
        format!(
            "{}\n\n{}",
            big_para("The board met today."),
            big_para("Every shareholder was paid.")
        )
    }

    /// A three-paragraph page whose paragraphs stay THREE separate packed
    /// windows (each ~340 tokens, any adjacent pair exceeds
    /// `PACK_TARGET_TOKENS`), with distinct AAA/BBB/CCC markers.
    fn three_paragraphs() -> String {
        format!(
            "{}\n\n{}\n\n{}",
            big_para("AAA uno."),
            big_para("BBB due."),
            big_para("CCC tre.")
        )
    }

    /// A response with empty content and `finish_reason == "length"` — the EC08
    /// output-budget-exhausted case (a reasoning model that burned its budget).
    fn resp_length() -> ChatResponse {
        serde_json::from_value(serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": null }, "finish_reason": "length" }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 }
        }))
        .unwrap()
    }

    /// A TRUNCATED response (ticket 11): NON-empty content but `finish_reason ==
    /// "length"` — the model hit the output budget mid-answer (text cut off).
    fn resp_truncated(content: &str, tokens: i64) -> ChatResponse {
        serde_json::from_value(serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "length" }],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": tokens }
        }))
        .unwrap()
    }

    // --- Ticket 02 / STC-08: the provider's max_tokens reaches the request ----

    #[test]
    fn translate_page_threads_the_provider_max_tokens_into_the_request() {
        let c = conn();
        seed_session(&c);
        // Single-paragraph page + large n_ctx → 1 translate unit + 1 perceptor.
        let client = MockClient::new(vec![
            Ok(resp("Ciao", 100)),               // translate-only unit
            Ok(resp(&valid_content(), 400)),     // perceptor-update
        ]);
        let p = TranslateParams {
            document_id: 1,
            page_number: 3,
            target_language: "it",
            page_text: "Hello",
            model: "local-model",
            max_tokens: 2048, // a local provider reserving output headroom
            n_ctx: 128_000,
            update_context: true,
            is_current: None,
        };
        translate_page(&c, &client, &p).unwrap();
        let sent = client.requests.borrow();
        // Single unit (degrade) keeps the page max_tokens; the perceptor call
        // always uses the page max_tokens.
        assert_eq!(sent[0].max_tokens, 2048, "unit request carries the provider's max_tokens");
        assert_eq!(sent[1].max_tokens, 2048, "perceptor request carries the provider's max_tokens");
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

        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 200)),       // translate-only unit
            Ok(resp(&valid_content(), 400)),   // perceptor-update
        ]);
        // Request page 3 with its REAL text — source_text differs from the row.
        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert!(!out.from_cache, "mismatched source_text must be a miss");
        assert_eq!(out.translated_text, "Ciao mondo", "re-translated, not the poisoned value");
        assert_eq!(client.calls(), 2, "the model is called (unit + perceptor) on a source mismatch");

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

        let client = MockClient::new(vec![
            Ok(resp("Traduzione pagina 10", 300)),          // translate-only unit
            Ok(resp(&content_with("ignored", "s", &[]), 200)), // perceptor-update
        ]);
        let p = TranslateParams {
            document_id: 1,
            page_number: 10,
            target_language: "it",
            page_text: "page 10 text",
            model: "openai/gpt-4o",
            max_tokens: 4096,
            n_ctx: 128_000,
            update_context: true,
            is_current: None,
        };

        let out = translate_page(&c, &client, &p).unwrap();

        assert_ne!(out.translated_text, "Ignoranza", "must NOT serve the page-9 translation");
        assert_eq!(out.translated_text, "Traduzione pagina 10");
        assert!(!out.from_cache, "the mismatched row is not served as a cache hit");
        assert_eq!(client.calls(), 2, "the page is re-translated (unit + perceptor)");

        // A subsequent visit with the SAME correct text is now a legitimate hit.
        let out2 = translate_page(&c, &client, &p).unwrap();
        assert!(out2.from_cache, "healed row is served on the next matching visit");
        assert_eq!(out2.translated_text, "Traduzione pagina 10");
        assert_eq!(client.calls(), 2, "no extra model call once healed");
    }

    // --- Cache miss ----------------------------------------------------------

    #[test]
    fn cache_miss_calls_client_saves_and_records_tokens() {
        let c = conn();
        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 1000)),    // translate-only unit
            Ok(resp(&valid_content(), 801)), // perceptor-update
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert_eq!(out.translated_text, "Ciao mondo");
        assert!(!out.from_cache);
        assert_eq!(out.total_tokens, Some(1801), "usage.total_tokens summed across unit + perceptor");
        assert_eq!(client.calls(), 2);

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
        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 250)),     // translate-only unit
            Ok(resp(&valid_content(), 250)), // perceptor-update
        ]);

        let first = translate_page(&c, &client, &params("Hello")).unwrap();
        assert!(!first.from_cache);

        let second = translate_page(&c, &client, &params("Hello")).unwrap();
        assert!(second.from_cache);
        assert_eq!(second.translated_text, "Ciao mondo");
        assert_eq!(client.calls(), 2, "no extra model calls for a cached page");
    }

    // --- Layered parsing through the service --------------------------------

    /// The JSON-correction retry lives on the PERCEPTOR call (the translate-only
    /// contract needs no correction: plain text always parses). A malformed
    /// perceptor answer followed by a valid one succeeds after one retry.
    #[test]
    fn perceptor_malformed_then_valid_succeeds_after_one_correction_retry() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 100)),                                  // translate-only unit
            Ok(resp("sorry, here is the summary…", 100)),                 // perceptor malformed
            Ok(resp(&format!("```json\n{}\n```", valid_content()), 120)), // perceptor correction
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo", "translation from the unit call");
        assert_eq!(out.updated_summary.as_deref(), Some("riassunto"), "summary from the corrected perceptor");
        assert_eq!(client.calls(), 3, "unit + perceptor malformed + one correction retry");
    }

    /// STC-10 resilience (criterion c): a perceptor-update that fails (malformed
    /// twice, so its own correction retry gives up) must NOT discard the page
    /// translation — the units' text is still returned AND page-cached, the
    /// summary is NOT advanced (`updated_summary == None`) and no glossary terms
    /// are inserted; the function returns `Ok`.
    #[test]
    fn perceptor_failure_still_returns_and_caches_the_page_summary_not_advanced() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Riassunto originale.").unwrap();
        let before_glossary = crate::glossary::list_glossary(&c, 1).unwrap().len();

        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 10)),     // translate-only unit (succeeds)
            Ok(resp("not json", 10)),       // perceptor malformed
            Ok(resp("still not json", 10)), // perceptor correction still malformed
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo", "translation from the unit is returned");
        assert_eq!(out.updated_summary, None, "summary not advanced on perceptor failure");
        assert_eq!(client.calls(), 3, "unit + perceptor + one correction retry");

        // The page IS cached (resilience) with the correct source + translation.
        let (stored, src): (String, String) = c
            .query_row(
                "SELECT translated_text, source_text FROM translations_cache
                  WHERE document_id=1 AND page_number=3 AND target_language='it'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(stored, "Ciao mondo", "page cached despite the perceptor failure");
        assert_eq!(src, "Hello");

        // Context NOT advanced: summary unchanged, no glossary terms added.
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            "Riassunto originale.",
            "rolling summary not advanced"
        );
        assert_eq!(
            crate::glossary::list_glossary(&c, 1).unwrap().len(),
            before_glossary,
            "no glossary terms inserted on perceptor failure"
        );
    }

    /// STC-10 resilience (ticket 02, criterion b): when the strict perceptor JSON
    /// fails to parse (here: a response with the `new_glossary_terms` array but a
    /// TRUNCATED `updated_summary`, as the small local model produces routinely),
    /// the glossary terms are TOLERANTLY recovered and inserted EVEN THOUGH the
    /// summary cannot be advanced — glossary insertion is decoupled from
    /// `summary_advanced`. The page is still cached and the failure is signalled.
    #[test]
    fn perceptor_partial_json_recovers_and_inserts_glossary_terms_summary_not_advanced() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Riassunto originale.").unwrap();
        let before = crate::glossary::list_glossary(&c, 1).unwrap().len();

        // Perceptor answers with recoverable terms but a summary cut off mid-word:
        // strict parse (needs BOTH fields) fails on both the first answer and the
        // correction retry, so today the terms would be lost silently.
        let partial = "Ecco l'aggiornamento del contesto.\n\
            {\"new_glossary_terms\": [\
              {\"source_term\":\"hobbit\",\"translation\":\"hobbit\",\"type\":\"nome proprio\",\"note\":\"\"}\
            ], \"updated_summary\": \"Bilbo lascia la Contea per un'avvent";

        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 10)), // translate-only unit (succeeds)
            Ok(resp(partial, 10)),      // perceptor: strict parse fails
            Ok(resp(partial, 20)),      // perceptor correction: still strict-fails
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        // Translation is preserved and cached (STC-10 invariant).
        assert_eq!(out.translated_text, "Ciao mondo");
        // Summary NOT advanced (couldn't be parsed) — reported as None.
        assert_eq!(out.updated_summary, None, "summary not advanced on partial perceptor");
        // The failure is SIGNALLED to the UI (non-silent).
        assert!(out.perceptor_update_failed, "partial perceptor failure is surfaced");
        assert_eq!(client.calls(), 3, "unit + perceptor + one correction retry");

        // The recovered glossary term WAS inserted, decoupled from the summary.
        let terms = crate::glossary::list_glossary(&c, 1).unwrap();
        assert_eq!(terms.len(), before + 1, "recovered term inserted despite summary not advancing");
        assert!(
            terms.iter().any(|t| t.source_term == "hobbit"),
            "the tolerant-extracted term is present"
        );

        // Summary genuinely unchanged.
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            "Riassunto originale.",
            "rolling summary not advanced"
        );

        // Page cached despite the perceptor failure.
        let stored: String = c
            .query_row(
                "SELECT translated_text FROM translations_cache
                  WHERE document_id=1 AND page_number=3 AND target_language='it'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "Ciao mondo");
    }

    /// STC-10 observability (ticket 02, criterion a): a perceptor-update that
    /// fails at the TRANSPORT level (no content to recover terms from) still
    /// returns the translation, caches it, and SIGNALS the failure via
    /// `perceptor_update_failed` — never a silent `eprintln!`-only swallow.
    #[test]
    fn perceptor_transport_failure_surfaces_signal_and_keeps_translation() {
        let c = conn();
        seed_session(&c);
        let before = crate::glossary::list_glossary(&c, 1).unwrap().len();

        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 10)),                    // unit succeeds
            Err(LlmError::Http("500 boom".into())),        // perceptor transport error
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo", "translation preserved");
        assert!(out.perceptor_update_failed, "transport failure surfaced to the UI");
        assert_eq!(out.updated_summary, None, "summary not advanced");
        assert_eq!(client.calls(), 2, "unit + perceptor (no correction retry on transport error)");
        assert_eq!(
            crate::glossary::list_glossary(&c, 1).unwrap().len(),
            before,
            "no terms recoverable from a transport error"
        );

        // Page still cached.
        let stored: String = c
            .query_row(
                "SELECT translated_text FROM translations_cache
                  WHERE document_id=1 AND page_number=3 AND target_language='it'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "Ciao mondo");
    }

    /// A fully successful perceptor-update reports `perceptor_update_failed ==
    /// false` (no false alarm) and advances the summary as before.
    #[test]
    fn perceptor_success_does_not_signal_failure() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 100)),     // unit
            Ok(resp(&valid_content(), 100)), // perceptor OK
        ]);
        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert!(!out.perceptor_update_failed, "success is not a failure");
        assert_eq!(out.updated_summary.as_deref(), Some("riassunto"));
    }

    // --- Bug #1: model-agnostic 404 fallback through the service ------------

    /// A 404 "No endpoints found" on the PERCEPTOR body (which sends the rich
    /// response_format) must trigger one downgraded retry (research §2) that
    /// succeeds. The translate-only unit call already succeeded before it.
    #[test]
    fn unsupported_params_404_recovers_via_downgraded_retry() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 100)), // translate-only unit (plain text)
            Err(LlmError::UnsupportedParams(
                "404 Not Found: {\"error\":{\"message\":\"No endpoints found that can handle \
                 the requested parameters.\",\"code\":404}}"
                    .into(),
            )),
            Ok(resp(&valid_content(), 321)), // perceptor downgraded retry succeeds
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Ciao mondo", "translation from the unit call");
        assert_eq!(out.updated_summary.as_deref(), Some("riassunto"));
        assert_eq!(client.calls(), 3, "unit + perceptor 404 + downgraded perceptor retry");

        let reqs = client.requests.borrow();
        // The translate-only unit call never sends the rich response_format (D5).
        assert!(reqs[0].response_format.is_none(), "translate-only sends no response_format");
        // The perceptor's first attempt sent response_format; the retry stripped it.
        assert!(reqs[1].response_format.is_some(), "perceptor first attempt sent response_format");
        assert!(reqs[2].response_format.is_none(), "perceptor retry stripped response_format");
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

    /// After the fallback degrades the body on the first unit, later units of the
    /// SAME page must start already-degraded — no repeated re-probe (finding 2b).
    /// Translate-only requests send no response_format, so the strippable param
    /// here is `temperature`; the second unit then succeeds in one call.
    #[test]
    fn degraded_shape_is_reused_by_later_units_no_reprobe() {
        let c = conn();
        seed_session(&c);
        // Two big paragraphs + small n_ctx -> two packed translate-only windows.
        let page = two_paragraphs();
        assert_eq!(
            pack_units(split_into_units(&page, 4096, RATIO), PACK_TARGET_TOKENS, RATIO).len(),
            2,
            "precondition: two packed windows"
        );

        let client = MockClient::new(vec![
            // unit 1: full body rejected, then a downgraded retry succeeds.
            Err(LlmError::UnsupportedParams("no endpoints found".into())),
            Ok(resp("PART0", 10)),
            // unit 2: must already be degraded -> ONE successful call, no re-probe.
            Ok(resp("PART1", 10)),
            // perceptor-update for the page.
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);

        let out = translate_page(&c, &client, &params_small(&page)).unwrap();

        assert_eq!(client.calls(), 4, "unit1: 2 calls; unit2: 1 call; perceptor: 1 call");
        assert!(out.translated_text.contains("PART0") && out.translated_text.contains("PART1"));

        let reqs = client.requests.borrow();
        assert!(reqs[0].response_format.is_none(), "translate-only sends no response_format");
        assert!(reqs[0].temperature.is_some(), "unit1 first attempt sent temperature");
        assert!(reqs[1].temperature.is_none(), "unit1 retry stripped temperature");
        assert!(
            reqs[2].temperature.is_none(),
            "unit2 starts already-degraded, no temperature re-probe"
        );
    }

    /// The perceptor JSON-correction retry must be issued on the already-degraded
    /// body, so it does not re-send the param the model rejected (finding 2a).
    /// Sequence: unit ok -> perceptor 404 -> degraded perceptor returns malformed
    /// JSON -> correction retry on the degraded body returns valid JSON. Four
    /// calls total; a re-probe would make five.
    #[test]
    fn correction_retry_reuses_degraded_body_no_reprobe() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp("Ciao mondo", 10)),                             // translate-only unit
            Err(LlmError::UnsupportedParams("no endpoints found".into())), // perceptor 404
            Ok(resp("not json at all", 10)),                        // degraded perceptor malformed
            Ok(resp(&valid_content(), 55)),                         // degraded correction valid
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        assert_eq!(out.translated_text, "Ciao mondo");
        assert_eq!(out.updated_summary.as_deref(), Some("riassunto"));
        assert_eq!(client.calls(), 4, "unit + 404 + degraded malformed + degraded correction retry");

        let reqs = client.requests.borrow();
        assert!(reqs[2].response_format.is_none(), "degraded perceptor call stripped response_format");
        assert!(
            reqs[3].response_format.is_none(),
            "correction retry reuses the degraded body (no re-probe)"
        );
        assert!(
            reqs[3].messages.iter().any(|m| m.content.contains(crate::llm::CORRECTION_PROMPT)),
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

        let client = MockClient::new(vec![
            Ok(resp("Il consiglio si e riunito.", 100)), // translate-only unit
            Ok(resp(&valid_content(), 100)),             // perceptor-update
        ]);
        translate_page(&c, &client, &params("The board met.")).unwrap();

        // The unit's translate-only prompt carries the read-only summary and the
        // locked term SELECTED for the unit (STC-03), rendered as an absolute
        // constraint.
        let prompt = client.user_prompt(0);
        assert!(prompt.contains("Contesto delle pagine precedenti."), "summary in prompt");
        assert!(prompt.contains("Termini BLOCCATI (vincolo assoluto"), "absolute heading");
        assert!(prompt.contains("board => consiglio"), "selected locked term rendered");
    }

    // --- Summary persistence (ticket 09) ------------------------------------

    #[test]
    fn updated_summary_persisted_to_session_after_page() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp("Tradotto", 100)), // translate-only unit
            Ok(resp(&content_with("ignored", "Nuovo riassunto pag. 3.", &[]), 200)), // perceptor
        ]);

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

        let client = MockClient::new(vec![
            Ok(resp("tradotto", 100)), // translate-only unit
            Ok(resp(
                &content_with("t", "s", &[("board", "altra"), ("CEO", "ad")]),
                100,
            )), // perceptor proposes terms
        ]);
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
        let client = MockClient::new(vec![
            Ok(resp("Tradotto", 100)),                    // translate-only unit
            Ok(resp(&content_with("ignored", short, &[]), 500)), // perceptor recompresses
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();

        // Recompression is requested on the PERCEPTOR prompt (the summary is
        // owned by the once-per-page perceptor call, D6), not the unit prompt.
        assert!(
            client.user_prompt(1).contains(crate::llm::COMPRESSION_INSTRUCTION),
            "perceptor prompt requests recompression"
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

        let client = MockClient::new(vec![
            Ok(resp("Tradotto", 100)),       // translate-only unit
            Ok(resp(&valid_content(), 100)), // perceptor-update
        ]);
        translate_page(&c, &client, &params("Hello")).unwrap();

        assert!(
            !client.user_prompt(1).contains(crate::llm::COMPRESSION_INSTRUCTION),
            "no recompression request under threshold"
        );
    }

    // --- STC-08: budget-aware multi-unit flow -------------------------------

    /// The budget formula: a small n_ctx yields a modest (but usable) unit budget;
    /// a large n_ctx (cloud) yields an enormous one so the page is never chunked.
    #[test]
    fn compute_budget_unit_text_small_vs_large_n_ctx() {
        // Local: n_ctx 4096, out_unit 768, ~15% margin, minus system/summary/gloss.
        let local = compute_budget_unit_text(4096, 768, 120, 0, 256, BUDGET_MARGIN);
        assert!(local >= MIN_BUDGET_UNIT_TEXT, "never below the floor");
        assert!(local < 4096, "a small window yields a bounded unit budget: {local}");
        assert!(local > 1500, "still comfortably larger than a real paragraph: {local}");

        // Cloud: a huge n_ctx makes the budget non-binding (≫ any real page).
        let cloud = compute_budget_unit_text(128_000, 768, 120, 300, 256, BUDGET_MARGIN);
        assert!(cloud > 100_000, "a large window barely constrains the unit budget: {cloud}");

        // Degenerate window: never underflows, floored at the minimum.
        assert_eq!(compute_budget_unit_text(100, 768, 50, 50, 50, BUDGET_MARGIN), MIN_BUDGET_UNIT_TEXT);
    }

    /// Small context + a multi-paragraph page → several translate-only calls
    /// (one per PACKED window since ticket 04, not per paragraph), plus a single
    /// perceptor call; the translated windows recompose in order (AC: split →
    /// pack → translate → reassemble). The three big paragraphs stay three
    /// windows here, so the multi-window path is exercised.
    #[test]
    fn small_context_splits_page_into_units_and_recomposes() {
        let c = conn();
        seed_session(&c);
        let page = three_paragraphs();
        let n = pack_units(split_into_units(&page, 4096, RATIO), PACK_TARGET_TOKENS, RATIO).len();
        assert_eq!(n, 3, "precondition: three packed windows, got {n}");

        // One plain translation per unit + one perceptor-update for the page.
        let mut responses: Vec<_> = (0..n)
            .map(|i| Ok(resp(&format!("T{i}"), 10)))
            .collect();
        responses.push(Ok(resp(&content_with("ignored", "riassunto finale", &[]), 7)));
        let client = MockClient::new(responses);

        let out = translate_page(&c, &client, &params_small(&page)).unwrap();

        assert_eq!(client.calls(), n + 1, "one call per packed window + one perceptor call");
        for i in 0..n {
            assert!(out.translated_text.contains(&format!("T{i}")), "T{i} present");
        }
        let first = out.translated_text.find("T0").unwrap();
        let last = out.translated_text.find(&format!("T{}", n - 1)).unwrap();
        assert!(first < last, "units recomposed in order");

        // The perceptor advanced the summary exactly once for the page.
        assert_eq!(
            crate::documents::get_rolling_summary(&c, 1).unwrap(),
            "riassunto finale"
        );
        // Total tokens summed across every unit call + the perceptor call.
        assert_eq!(out.total_tokens, Some(10 * n as i64 + 7));
    }

    /// Each translate-only unit carries ONLY the glossary selected for that unit
    /// (STC-03) and a small per-unit `max_tokens` (out_unit) — never the whole
    /// glossary, never the page `max_tokens`.
    #[test]
    fn units_carry_selected_glossary_and_small_max_tokens() {
        let c = conn();
        seed_session(&c);
        // Three unlocked terms: one per paragraph, one that appears in neither.
        for (t, tr) in [("board", "consiglio"), ("shareholder", "azionista"), ("dividend", "dividendo")] {
            c.execute(
                "INSERT INTO glossary
                     (document_id, source_term, translation, type, locked, note, first_seen_page)
                 VALUES (1, ?1, ?2, 'comune', 0, '', 1)",
                params![t, tr],
            )
            .unwrap();
        }

        let client = MockClient::new(vec![
            Ok(resp("T uno", 10)),  // unit 0
            Ok(resp("T due", 10)),  // unit 1
            Ok(resp(&content_with("ignored", "s", &[]), 5)), // perceptor
        ]);
        let page = two_paragraphs();
        translate_page(&c, &client, &params_small(&page)).unwrap();

        assert_eq!(client.calls(), 3, "two windows + one perceptor");
        let reqs = client.requests.borrow();
        // Multi-window path (ticket 04): il cap per finestra scala col corpo
        // della finestra (2×) PIÙ la riserva CoT — non più il piccolo out_unit
        // fisso — e resta ben sotto il max_tokens di pagina (2048) e n_ctx.
        let windows = pack_units(split_into_units(&page, 4096, RATIO), PACK_TARGET_TOKENS, RATIO);
        for (i, w) in windows.iter().enumerate() {
            // Stesso calcolo del flusso reale (window_output_cap), con headroom
            // volutamente non vincolante in questo scenario (prompt piccolo vs
            // n_ctx 4096) — così il test non duplica la formula in lockstep.
            let expected = window_output_cap(est_tokens(w.trim_end(), RATIO), 2048, u32::MAX);
            assert_eq!(
                reqs[i].max_tokens, expected,
                "finestra {i}: cap dal window_output_cap (corpo×2 + riserva CoT)"
            );
        }
        // The invariant that actually prevents EC08: prompt + output <= n_ctx.
        for r in [&reqs[0], &reqs[1]] {
            let prompt_est: u32 = r.messages.iter().map(|m| est_tokens(&m.content, RATIO)).sum();
            assert!(prompt_est + r.max_tokens <= 4096, "unit request fits within n_ctx");
        }

        // Per-unit selection: unit 0 sees only "board", unit 1 only "shareholder";
        // "dividend" (absent from both paragraphs) reaches neither prompt.
        let p0 = client.user_prompt(0);
        let p1 = client.user_prompt(1);
        assert!(p0.contains("board => consiglio") && !p0.contains("shareholder"));
        assert!(p1.contains("shareholder => azionista") && !p1.contains("board"));
        assert!(!p0.contains("dividend") && !p1.contains("dividend"), "absent term never sent");
    }

    /// STC-10 (criterion a): the perceptor-update call uses the LEAN contract —
    /// its request carries the lean response_format (no `translated_text`), its
    /// prompt does not ask to translate, and it ships only the SELECTED glossary
    /// for the page, never the whole glossary.
    #[test]
    fn perceptor_update_uses_the_lean_contract_and_compact_glossary() {
        let c = conn();
        seed_session(&c);
        // Three unlocked terms; only "board" appears in the page text.
        for (t, tr) in [
            ("board", "consiglio"),
            ("shareholder", "azionista"),
            ("dividend", "dividendo"),
        ] {
            c.execute(
                "INSERT INTO glossary
                     (document_id, source_term, translation, type, locked, note, first_seen_page)
                 VALUES (1, ?1, ?2, 'comune', 0, '', 1)",
                params![t, tr],
            )
            .unwrap();
        }

        let client = MockClient::new(vec![
            Ok(resp("Il consiglio si e riunito.", 10)), // translate-only unit
            Ok(resp(&content_with("ignored", "s", &[]), 10)), // lean perceptor-update
        ]);
        translate_page(&c, &client, &params("The board met.")).unwrap();

        let reqs = client.requests.borrow();
        // The unit call sends NO response_format; the perceptor sends the LEAN one.
        assert!(reqs[0].response_format.is_none(), "translate-only sends no schema");
        let rf = reqs[1]
            .response_format
            .clone()
            .expect("perceptor sends a response_format");
        assert_eq!(rf["json_schema"]["name"], "perceptor_update", "lean schema, not the full one");
        assert!(
            rf["json_schema"]["schema"]["properties"]
                .get("translated_text")
                .is_none(),
            "the lean schema does not ask for translated_text"
        );

        // The perceptor prompt does not ask to translate and ships only "board".
        let perceptor_prompt = client.user_prompt(1);
        assert!(perceptor_prompt.contains("NIENTE traduzione"), "prompt forbids re-translation");
        assert!(!perceptor_prompt.contains("translated_text"), "no translated_text in the prompt");
        assert!(perceptor_prompt.contains("board => consiglio"), "selected term present");
        assert!(!perceptor_prompt.contains("shareholder"), "absent term not sent (compact glossary)");
        assert!(!perceptor_prompt.contains("dividend"), "absent term not sent (compact glossary)");
    }

    /// Large context (cloud) + a single-paragraph page → ONE unit = whole page,
    /// using the page `max_tokens` (degrade to the previous behaviour, D2; no
    /// cloud regression).
    #[test]
    fn large_context_degrades_to_a_single_unit_equivalent() {
        let c = conn();
        seed_session(&c);
        let page = "This is a single paragraph page with several sentences. It has no blank \
lines, so it stays a single translation unit even though it is not short.";
        assert_eq!(split_into_units(page, 1_000_000, RATIO).len(), 1, "single paragraph → one unit");

        let client = MockClient::new(vec![
            Ok(resp("Traduzione intera della pagina.", 100)), // the single unit
            Ok(resp(&valid_content(), 200)),                  // perceptor-update
        ]);
        let out = translate_page(&c, &client, &params(page)).unwrap();

        assert_eq!(out.translated_text, "Traduzione intera della pagina.", "whole page from one call");
        let reqs = client.requests.borrow();
        // Exactly one translate-only unit call (response_format omitted), then the
        // perceptor (response_format present).
        let unit_calls = reqs.iter().filter(|r| r.response_format.is_none()).count();
        assert_eq!(unit_calls, 1, "one unit = whole page");
        assert_eq!(reqs[0].max_tokens, 4096, "single unit keeps the page max_tokens (no truncation)");
    }

    /// The perceptor-update runs exactly once per page on real navigation, and
    /// never on prefetch (D5/D6).
    #[test]
    fn perceptor_runs_once_on_nav_and_never_on_prefetch() {
        // Real navigation: several units, exactly one perceptor call.
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp("T uno", 10)),
            Ok(resp("T due", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        let page = two_paragraphs();
        translate_page(&c, &client, &params_small(&page)).unwrap();
        let perceptor_calls = client
            .requests
            .borrow()
            .iter()
            .filter(|r| r.response_format.is_some())
            .count();
        assert_eq!(perceptor_calls, 1, "exactly one perceptor-update per page on nav");

        // Prefetch of another page: units only, no perceptor call at all.
        let c2 = conn();
        seed_session(&c2);
        let client2 = MockClient::new(vec![Ok(resp("T uno", 10)), Ok(resp("T due", 10))]);
        let prefetch = TranslateParams {
            document_id: 1,
            page_number: 5,
            target_language: "it",
            page_text: &page,
            model: "local-model",
            max_tokens: 2048,
            n_ctx: 4096,
            update_context: false,
            is_current: None,
        };
        translate_page(&c2, &client2, &prefetch).unwrap();
        assert_eq!(client2.calls(), 2, "prefetch = units only");
        let perceptor_calls2 = client2
            .requests
            .borrow()
            .iter()
            .filter(|r| r.response_format.is_some())
            .count();
        assert_eq!(perceptor_calls2, 0, "prefetch never runs the perceptor");
    }

    /// D6: the SAME summary version is passed read-only to every unit of the page
    /// (it is not advanced incrementally between units).
    #[test]
    fn same_summary_version_passed_to_every_unit() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "RIASSUNTO-VERSIONE-UNICA.").unwrap();

        let client = MockClient::new(vec![
            Ok(resp("T uno", 10)),
            Ok(resp("T due", 10)),
            // Perceptor advances the summary — this must NOT leak into the units.
            Ok(resp(&content_with("ignored", "RIASSUNTO-AVANZATO.", &[]), 10)),
        ]);
        let page = two_paragraphs();
        translate_page(&c, &client, &params_small(&page)).unwrap();

        let p0 = client.user_prompt(0);
        let p1 = client.user_prompt(1);
        assert!(p0.contains("RIASSUNTO-VERSIONE-UNICA."), "unit 0 sees the loaded summary");
        assert!(p1.contains("RIASSUNTO-VERSIONE-UNICA."), "unit 1 sees the SAME summary version");
        assert!(!p1.contains("RIASSUNTO-AVANZATO."), "the perceptor update never leaks into a unit");
    }

    /// EC08 (`OutputBudgetExhausted`) on a unit surfaces from the whole page and
    /// leaves nothing cached.
    #[test]
    fn output_budget_exhausted_on_a_unit_surfaces_ec08() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![Ok(resp_length())]); // first unit exhausts the budget

        let page = two_paragraphs();
        let err = translate_page(&c, &client, &params_small(&page)).unwrap_err();
        assert!(matches!(err, LlmError::OutputBudgetExhausted(_)), "EC08 surfaces per unit");
        assert!(err.user_message().contains("EC08"));

        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM translations_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "a failed unit caches nothing");
    }

    // --- Ticket 11: detect & retry truncated units --------------------------

    /// A unit whose first completion is TRUNCATED (non-empty + finish_reason
    /// "length") must be RETRIED with a larger `max_tokens`; the retry completes
    /// and only the COMPLETE translation is cached — the partial is never written
    /// to the per-unit cache (criteria a/b/c).
    #[test]
    fn truncated_unit_is_retried_with_larger_budget_and_only_complete_is_cached() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp_truncated("…con milioni o", 100)), // unit truncated mid-sentence
            Ok(resp("Testo completo.", 200)),          // retry completes
            Ok(resp(&valid_content(), 300)),           // perceptor-update
        ]);

        let out = translate_page(&c, &client, &params("Hello")).unwrap();
        assert_eq!(out.translated_text, "Testo completo.", "the complete retry wins");
        assert_eq!(client.calls(), 3, "unit truncated + retry + perceptor");

        // The retry grew the output budget beyond the first attempt.
        let reqs = client.requests.borrow();
        assert!(
            reqs[1].max_tokens > reqs[0].max_tokens,
            "retry doubles the output budget ({} -> {})",
            reqs[0].max_tokens,
            reqs[1].max_tokens
        );

        // Only the COMPLETE translation is in the per-unit cache (no partial).
        let cached: String = c
            .query_row(
                "SELECT translated_text FROM unit_translations
                  WHERE document_id=1 AND page_number=3 AND unit_index=0 AND target_language='it'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cached, "Testo completo.", "the truncated partial was never cached");
    }

    /// A unit that keeps truncating even after the bounded retries surfaces an
    /// actionable EC08 (`OutputBudgetExhausted`) and caches NOTHING — never a
    /// partial page (criteria b/c).
    #[test]
    fn persistently_truncated_unit_surfaces_ec08_and_caches_nothing() {
        let c = conn();
        seed_session(&c);
        // Every attempt truncates: initial + TRUNCATION_MAX_RETRIES growths.
        let client = MockClient::new(vec![
            Ok(resp_truncated("part 1", 10)),
            Ok(resp_truncated("part 1 e 2", 10)),
            Ok(resp_truncated("part 1 e 2 e 3", 10)),
        ]);

        let page = two_paragraphs();
        let err = translate_page(&c, &client, &params_small(&page)).unwrap_err();
        assert!(
            matches!(err, LlmError::OutputBudgetExhausted(_)),
            "persistent truncation escalates to EC08, got {err:?}"
        );
        assert!(err.user_message().contains("EC08"));
        assert_eq!(
            client.calls(),
            1 + TRUNCATION_MAX_RETRIES as usize,
            "initial attempt + bounded retries, then give up"
        );
        assert_eq!(unit_rows(&c), 0, "a truncated unit is never cached");
        assert_eq!(page_rows(&c), 0, "no page cached on failure");
    }

    /// The retry ceiling never lets `prompt + output` exceed `n_ctx` (EC08 guard
    /// preserved through the growth): every request the retry issues still fits.
    #[test]
    fn truncation_retry_stays_within_the_context_window() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![
            Ok(resp_truncated("PART0-troncato", 10)), // unit 0 truncates once
            Ok(resp("PART0", 10)),                    // then completes
            Ok(resp("PART1", 10)),                    // unit 1
            Ok(resp(&content_with("ignored", "s", &[]), 10)), // perceptor
        ]);
        let page = two_paragraphs();
        translate_page(&c, &client, &params_small(&page)).unwrap();

        let reqs = client.requests.borrow();
        // The retried request (index 1) grew but still fits within n_ctx=4096.
        assert!(reqs[1].max_tokens > reqs[0].max_tokens, "budget grew on retry");
        for r in reqs.iter() {
            let prompt_est: u32 = r.messages.iter().map(|m| est_tokens(&m.content, RATIO)).sum();
            assert!(
                prompt_est + r.max_tokens <= 4096,
                "prompt + output must fit n_ctx (got {} + {})",
                prompt_est,
                r.max_tokens
            );
        }
    }

    // --- Prefetch: cache-only, no context mutation (ticket 12) --------------

    /// A prefetch (`update_context: false`) of a later page warms ONLY the
    /// per-unit cache (STC-09), NOT the page-level cache (ticket 01, option B),
    /// and leaves `sessions.rolling_summary` and the glossary untouched —
    /// advancing the percettore or writing the page row out of order would keep
    /// the later real navigation from advancing the context.
    #[test]
    fn prefetch_caches_translation_without_touching_summary_or_glossary() {
        let c = conn();
        seed_session(&c);
        crate::documents::set_rolling_summary(&c, 1, "Riassunto originale.").unwrap();
        let before_glossary = crate::glossary::list_glossary(&c, 1).unwrap().len();

        // The percettore is skipped on prefetch, so a single translate-only
        // response is all that is consumed (the whole page is one unit here).
        let client = MockClient::new(vec![Ok(resp("Tradotto in anticipo", 300))]);
        let prefetch = TranslateParams {
            document_id: 1,
            page_number: 4, // the NEXT page (N+1)
            target_language: "it",
            page_text: "Next page text.",
            model: "openai/gpt-4o",
            max_tokens: 4096,
            n_ctx: 128_000,
            update_context: false,
            is_current: None,
        };

        let out = translate_page(&c, &client, &prefetch).unwrap();
        assert!(!out.from_cache);
        assert_eq!(out.translated_text, "Tradotto in anticipo");
        assert_eq!(out.updated_summary, None, "prefetch does not report/persist a summary");
        assert_eq!(client.calls(), 1);

        // NUOVO CONTRATTO (opzione B): il prefetch NON scrive la cache di pagina
        // per page 4 — solo la cache per-unità viene scaldata.
        assert_eq!(page_rows(&c), 0, "il prefetch non scrive la cache di pagina");
        assert_eq!(unit_rows(&c), 1, "il prefetch scalda la cache per-unità (STC-09)");

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
            max_tokens: 4096,
            n_ctx: 128_000,
            update_context: false,
            is_current: None,
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

    /// REGRESSIONE (ticket 01, opzione B): un prefetch di P (`update_context:
    /// false`) NON deve scrivere la cache di PAGINA — scalda solo la cache
    /// per-unità (STC-09). Alla navigazione reale su P (`update_context: true`,
    /// stesso testo) la pagina è quindi un MISS di pagina: la pipeline prosegue,
    /// le unità sono servite dalla cache per-unità (nessuna ri-traduzione), il
    /// percettore gira UNA volta e il glossario cresce.
    ///
    /// Prima del fix questo test FALLISCE: il prefetch scrive la cache di pagina,
    /// la navigazione reale è un cache-hit che ritorna prima del percettore →
    /// zero chiamate, glossario resta a 0.
    #[test]
    fn prefetch_warmed_page_advances_context_on_real_navigation_without_retranslating() {
        let c = conn();
        seed_session(&c);
        assert_eq!(
            crate::glossary::list_glossary(&c, 1).unwrap().len(),
            0,
            "precondizione: glossario vuoto"
        );

        // --- Prefetch di P (N+1) con update_context=false -------------------
        // Una sola risposta: la traduzione dell'unica unità. Il percettore è
        // saltato sul prefetch, quindi nessuna risposta percettore serve qui.
        let client_prefetch = MockClient::new(vec![Ok(resp("Ciao mondo", 100))]);
        let prefetch = TranslateParams {
            document_id: 1,
            page_number: 4,
            target_language: "it",
            page_text: "Hello world.",
            model: "openai/gpt-4o",
            max_tokens: 4096,
            n_ctx: 128_000,
            update_context: false,
            is_current: None,
        };
        let out_pf = translate_page(&c, &client_prefetch, &prefetch).unwrap();
        assert!(!out_pf.from_cache);
        assert_eq!(client_prefetch.calls(), 1, "prefetch: solo la traduzione dell'unità");

        // Il prefetch NON scrive la cache di pagina, ma SCALDA la cache per-unità.
        assert_eq!(page_rows(&c), 0, "il prefetch non scrive la cache di pagina (opzione B)");
        assert_eq!(unit_rows(&c), 1, "il prefetch scalda la cache per-unità (STC-09)");

        // --- Navigazione reale su P con update_context=true -----------------
        // Coda con la SOLA risposta del percettore: se le unità venissero
        // ri-tradotte, la prima pop verrebbe consumata dalla traduzione e il
        // percettore andrebbe in panic sulla coda vuota. Quindi client.calls()==1
        // dimostra che l'unità è servita dalla cache per-unità (nessuna
        // ri-traduzione) e che il percettore gira esattamente una volta.
        let client_nav = MockClient::new(vec![Ok(resp(
            &content_with("ignorato", "Riassunto avanzato", &[("hello", "ciao")]),
            200,
        ))]);
        let nav = TranslateParams { update_context: true, ..prefetch };
        let out_nav = translate_page(&c, &client_nav, &nav).unwrap();

        assert!(!out_nav.from_cache, "navigazione reale: miss di pagina, non un cache-hit");
        assert_eq!(out_nav.translated_text, "Ciao mondo", "traduzione servita dalle unità cachate");
        assert_eq!(
            client_nav.calls(),
            1,
            "esattamente 1 chiamata: solo il percettore (nessuna ri-traduzione delle unità)"
        );

        // Il glossario è cresciuto da 0 al termine proposto dal percettore.
        let glossary = crate::glossary::list_glossary(&c, 1).unwrap();
        assert_eq!(glossary.len(), 1, "il glossario cresce col termine proposto");
        assert_eq!(glossary[0].source_term, "hello");
        assert_eq!(glossary[0].translation, "ciao");

        // Ora la pagina è cachata come "completa": un accesso successivo è un HIT.
        assert_eq!(page_rows(&c), 1, "la navigazione reale scrive la cache di pagina");
    }

    // --- Stale-job cancellation (ticket 06, L3/L4) ---------------------------

    /// `is_current` turning `false` once the first unit has reached the model
    /// stops the loop at the SECOND unit's boundary (`Err(LlmError::Cancelled)`)
    /// WITHOUT calling the model for that unit. The first unit -- already
    /// translated and cached in this same call -- stays a valid cache HIT on a
    /// later call (zero model calls to re-translate it).
    #[test]
    fn is_current_false_before_second_unit_cancels_without_extra_calls_and_keeps_prior_cache() {
        let c = conn();
        seed_session(&c);
        let client = MockClient::new(vec![Ok(resp("UN", 10))]);
        // Becomes non-current exactly once the first unit's call has landed,
        // so the SECOND unit's boundary check (top of the next loop iteration)
        // is what observes `false` and cancels -- not the first.
        let is_current = || client.calls() == 0;
        // Post-merge ticket 04: two_paragraphs() ora ritorna String (paragrafi
        // "grandi" che restano due finestre impacchettate distinte).
        let page = two_paragraphs();
        let p = TranslateParams { is_current: Some(&is_current), ..params_small(&page) };

        let result = translate_page(&c, &client, &p);
        assert!(
            matches!(result, Err(LlmError::Cancelled)),
            "job stops with Cancelled once the second unit's boundary sees is_current() == false"
        );
        assert_eq!(client.calls(), 1, "only the first unit reached the model; the second never did");

        // The first unit's translation was written to the per-unit cache
        // before cancellation: a fresh, always-current call must HIT it and
        // make only ONE model call to translate the second unit (plus the
        // perceptor-update call, since this resumed run is real navigation:
        // `params_small`'s `update_context: true`).
        let client2 = MockClient::new(vec![
            Ok(resp("DEUX", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        let p2 = TranslateParams { is_current: None, ..params_small(&page) };
        let out2 = translate_page(&c, &client2, &p2).unwrap();
        assert_eq!(
            client2.calls(),
            2,
            "first unit served from cache (zero calls for it); only the second unit + perceptor call the model"
        );
        assert!(out2.translated_text.contains("UN"), "cached first unit resurfaces intact");
        assert!(out2.translated_text.contains("DEUX"), "second unit translated fresh after resuming");
    }

    // --- PROTOTYPE Ticket 02: paragraph-level unit splitting ----------------

    use crate::llm::DEFAULT_CHARS_PER_TOKEN as RATIO;

    /// A realistic multi-paragraph page: paragraphs are separated by blank lines
    /// (as a reconstruction that preserves paragraph breaks would emit).
    fn sample_prose() -> &'static str {
        "La luce del mattino filtrava tra le persiane della vecchia biblioteca. \
Marta sfogliava con cura un volume ingiallito, cercando un passaggio che ricordava \
solo a metà. Il silenzio era rotto soltanto dal fruscio delle pagine.\n\n\
Fuori, la città cominciava a svegliarsi. I primi tram passavano rumorosi lungo il \
viale, e il profumo del caffè saliva dai bar appena aperti. Marta alzò lo sguardo \
un istante, poi tornò al suo libro.\n\n\
Trovò finalmente la frase che cercava. La rilesse tre volte, come per imprimersela \
nella memoria, e sorrise. Era esattamente ciò di cui aveva bisogno per il suo saggio."
    }

    /// A technical multi-paragraph passage resembling the "Build a Large Language
    /// Model (From Scratch)" book — the kind of page we must chunk within budget.
    fn sample_technical() -> &'static str {
        "Large language models (LLMs) are deep neural networks trained on massive \
amounts of text data. The transformer architecture, introduced in 2017, is the \
backbone of nearly all modern LLMs. It relies on a mechanism called self-attention, \
which lets the model weigh the relevance of every token to every other token in the \
sequence.\n\n\
Before text can be fed to the model it must be tokenized. A tokenizer splits raw \
text into smaller units called tokens, then maps each token to an integer ID. These \
IDs are looked up in an embedding matrix to produce dense vectors. Positional \
information is added so the model knows the order of the tokens.\n\n\
Training proceeds by next-token prediction. Given a sequence of tokens, the model \
predicts the probability distribution of the following token, and the cross-entropy \
loss between the prediction and the true next token is minimized with gradient \
descent. Repeated over billions of tokens, this simple objective yields surprisingly \
capable models."
    }

    #[test]
    fn units_roundtrip_reproduces_source_exactly() {
        for text in [
            sample_prose(),
            sample_technical(),
            "un solo paragrafo senza interruzioni di riga.",
            "riga uno\nriga due\nriga tre", // single-\n joined lines (pdfExtract)
            "- primo\n- secondo\n- terzo\n\nUn paragrafo dopo la lista.",
        ] {
            for budget in [64u32, 256, 512, 1024] {
                let units = split_into_units(text, budget, RATIO);
                assert_eq!(
                    units.concat(),
                    text,
                    "round-trip must reproduce the source (budget={budget})"
                );
            }
        }
    }

    #[test]
    fn each_unit_fits_the_budget_when_sentences_are_splittable() {
        let budget = 64u32;
        for text in [sample_prose(), sample_technical()] {
            let units = split_into_units(text, budget, RATIO);
            for u in &units {
                assert!(
                    est_tokens(u.trim(), RATIO) <= budget,
                    "unit over budget ({} > {budget}): {:?}",
                    est_tokens(u.trim(), RATIO),
                    u
                );
            }
        }
    }

    #[test]
    fn oversized_paragraph_splits_at_sentence_boundaries() {
        // One paragraph (no blank line) of several sentences, budget too small to
        // hold it whole -> must break into >1 unit, each ending at a sentence
        // boundary.
        let para = "Prima frase molto chiara. Seconda frase altrettanto chiara. \
Terza frase per riempire il budget. Quarta frase conclusiva.";
        let budget = 8u32; // ~32 chars: forces per-sentence splitting
        let units = split_into_units(para, budget, RATIO);
        assert!(units.len() > 1, "an oversized paragraph must be split");
        assert_eq!(units.concat(), para, "no text lost when sentence-splitting");
        for u in &units {
            let t = u.trim_end();
            assert!(
                t.ends_with('.') || t.ends_with('!') || t.ends_with('?'),
                "each sentence-level unit ends at a sentence boundary: {u:?}"
            );
        }
    }

    #[test]
    fn order_is_preserved() {
        let text = "AAA primo paragrafo.\n\nBBB secondo paragrafo.\n\nCCC terzo paragrafo.";
        let units = split_into_units(text, 512, RATIO);
        let joined = units.concat();
        let a = joined.find("AAA").unwrap();
        let b = joined.find("BBB").unwrap();
        let c = joined.find("CCC").unwrap();
        assert!(a < b && b < c, "paragraph order preserved");
    }

    #[test]
    fn empty_and_whitespace_input_round_trip() {
        for text in ["", "   ", "\n\n", "  \n\t  \n "] {
            let units = split_into_units(text, 512, RATIO);
            assert_eq!(units.concat(), text, "whitespace-only input round-trips");
        }
        assert_eq!(split_into_units("", 512, RATIO), vec!["".to_string()]);
    }

    #[test]
    fn small_paragraphs_become_one_unit_each() {
        // Each paragraph fits the budget -> exactly one unit per paragraph.
        let units = split_into_units(sample_prose(), 512, RATIO);
        assert_eq!(units.len(), 3, "three paragraphs -> three units");
    }

    /// Measurement (acceptance): report the token size of every unit produced on
    /// the two realistic inputs at budgets in the 512-1024 range. Run with
    /// `cargo test -- --nocapture` to see the printed distribution.
    #[test]
    fn measure_unit_token_sizes_on_realistic_pages() {
        for (name, text) in [("prose", sample_prose()), ("technical", sample_technical())] {
            for budget in [512u32, 1024] {
                let units = split_into_units(text, budget, RATIO);
                let sizes: Vec<u32> = units.iter().map(|u| est_tokens(u.trim(), RATIO)).collect();
                let total: u32 = sizes.iter().sum();
                eprintln!(
                    "[measure] page={name} budget={budget} units={} sizes={sizes:?} total_tokens~={total}",
                    units.len()
                );
                assert_eq!(units.concat(), text, "measurement round-trip holds");
                assert!(!units.is_empty());
            }
        }
    }

    // --- Prototipo pack_units (local-translation-latency ticket 02) ---------

    /// Pagina densa sintetica: 18 paragrafi da ~40 token l'uno (~700 token
    /// totali), la forma che oggi produce 18 chiamate sequenziali.
    fn sample_dense_page() -> String {
        (1..=18)
            .map(|i| {
                format!(
                    "Paragrafo {i}: la scienza procede per domande, non per risposte \
definitive, e ogni esperimento ben costruito apre nuove incognite che nessun \
manuale aveva previsto o catalogato prima."
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    #[test]
    fn pack_units_roundtrip_reproduces_source_exactly() {
        for text in [sample_prose(), sample_technical(), &sample_dense_page()] {
            for budget in [64u32, 256, 512, 900] {
                let units = split_into_units(text, budget, RATIO);
                let packed = pack_units(units, budget, RATIO);
                assert_eq!(
                    packed.concat(),
                    text,
                    "packed round-trip must reproduce the source (budget={budget})"
                );
            }
        }
    }

    #[test]
    fn pack_units_windows_fit_the_budget() {
        for text in [sample_prose(), sample_technical(), &sample_dense_page()] {
            let budget = 256u32;
            let packed = pack_units(split_into_units(text, budget, RATIO), budget, RATIO);
            for w in &packed {
                assert!(
                    est_tokens(w.trim(), RATIO) <= budget,
                    "window over budget ({} > {budget}): {:?}",
                    est_tokens(w.trim(), RATIO),
                    w
                );
            }
        }
    }

    #[test]
    fn pack_units_reduces_call_count_on_dense_page() {
        let page = sample_dense_page();
        let budget = 900u32; // budget_unit_text tipico locale (n_ctx 4096)
        let units = split_into_units(&page, budget, RATIO);
        assert!(units.len() >= 15, "precondition: many per-paragraph units");
        let packed = pack_units(units.clone(), budget, RATIO);
        assert!(
            packed.len() * 4 <= units.len(),
            "packing must cut calls at least 4x ({} -> {})",
            units.len(),
            packed.len()
        );
    }

    #[test]
    fn pack_units_keeps_oversized_atom_as_own_window() {
        // Una singola frase-atomo oltre budget resta una finestra a sé e non
        // ingloba l'unità successiva.
        let atom = "parola ".repeat(200); // ~350 token, nessun confine di frase
        let units = vec![atom.clone(), "Frase corta.".to_string()];
        let packed = pack_units(units, 64, RATIO);
        assert_eq!(packed.len(), 2, "atom stays alone, short unit follows");
        assert_eq!(packed[0], atom);
    }

    #[test]
    fn pack_units_preserves_degenerate_inputs() {
        assert_eq!(pack_units(vec!["".to_string()], 512, RATIO), vec!["".to_string()]);
        assert_eq!(pack_units(Vec::new(), 512, RATIO), Vec::<String>::new());
        assert_eq!(
            pack_units(vec!["   \n\n".to_string()], 512, RATIO),
            vec!["   \n\n".to_string()]
        );
    }

    /// Analisi cache-miss (acceptance ticket 02): con la chiave attuale
    /// `(unit_index, source_hash)`, quante finestre sopravvivono a un repack con
    /// budget diverso (±200 token, es. summary/glossario cambiati)? Confronta le
    /// tre semantiche candidate per L1. Run con `--nocapture` per i numeri.
    #[test]
    fn measure_pack_repack_cache_stability() {
        let page = sample_dense_page();
        let (budget_a, budget_b) = (900u32, 700u32);

        // Semantica 1 — per-paragrafo (oggi): lo split non dipende dal budget
        // finché i paragrafi restano sotto entrambi i budget → cache stabile.
        let para_a = split_into_units(&page, budget_a, RATIO);
        let para_b = split_into_units(&page, budget_b, RATIO);
        let para_stable = para_a
            .iter()
            .zip(&para_b)
            .filter(|(a, b)| source_hash(a) == source_hash(b))
            .count();
        assert_eq!(para_stable, para_a.len(), "per-paragraph units are budget-stable");

        // Semantica 2 — finestre col budget dinamico: budget diverso → finestre
        // diverse → MISS su (index, hash) e anche su hash-only.
        let win_a = pack_units(para_a.clone(), budget_a, RATIO);
        let win_b = pack_units(para_b.clone(), budget_b, RATIO);
        let by_index = win_a
            .iter()
            .zip(&win_b)
            .filter(|(a, b)| source_hash(a) == source_hash(b))
            .count();
        let hashes_b: std::collections::HashSet<String> =
            win_b.iter().map(|w| source_hash(w)).collect();
        let by_hash_only = win_a.iter().filter(|w| hashes_b.contains(&source_hash(w))).count();

        // Semantica 3 — finestre a taglia FISSA (costante, indipendente dal
        // budget dinamico): repack identico per costruzione.
        let fixed = 512u32;
        let fix_a = pack_units(para_a.clone(), fixed, RATIO);
        let fix_b = pack_units(para_b.clone(), fixed, RATIO);
        let fixed_stable = fix_a
            .iter()
            .zip(&fix_b)
            .filter(|(a, b)| source_hash(a) == source_hash(b))
            .count();
        assert_eq!(fixed_stable, fix_a.len(), "fixed-size windows are budget-stable");

        eprintln!(
            "[measure] repack {budget_a}->{budget_b}: per-paragraph stable={para_stable}/{}; \
dynamic windows: a={} b={} stable_by_index={by_index} stable_by_hash={by_hash_only}; \
fixed({fixed}) windows: a={} b={} stable={fixed_stable}",
            para_a.len(),
            win_a.len(),
            win_b.len(),
            fix_a.len(),
            fix_b.len()
        );
    }

    // --- Ticket 04: packing cablato nella pipeline ---------------------------

    /// Le finestre che la pipeline produrrà per `page` coi parametri locali di
    /// `params_small`: split a un budget ampio (ogni paragrafo delle fixture è
    /// ben sotto il `budget_unit_text` interno ~2200, quindi lo split è
    /// identico) e packing alla costante L1.
    fn expected_windows(page: &str) -> Vec<String> {
        pack_units(split_into_units(page, 4096, RATIO), PACK_TARGET_TOKENS, RATIO)
    }

    /// Pagina densa (18 paragrafi piccoli) + n_ctx locale → le chiamate LLM sono
    /// UNA PER FINESTRA impacchettata (1-2 per pagina) + il perceptor, NON una
    /// per paragrafo (le ~18+1 di prima del ticket 04).
    #[test]
    fn dense_page_translates_in_few_packed_windows_not_one_call_per_paragraph() {
        let c = conn();
        seed_session(&c);
        let page = sample_dense_page();
        let windows = expected_windows(&page);
        assert!(
            (1..=2).contains(&windows.len()),
            "precondition L1: la pagina densa impacchetta in 1-2 finestre, got {}",
            windows.len()
        );

        let mut responses: Vec<_> = (0..windows.len())
            .map(|i| Ok(resp(&format!("W{i}"), 10)))
            .collect();
        responses.push(Ok(resp(&content_with("ignored", "s", &[]), 7)));
        let client = MockClient::new(responses);

        let out = translate_page(&c, &client, &params_small(&page)).unwrap();

        assert_eq!(
            client.calls(),
            windows.len() + 1,
            "una chiamata per FINESTRA impacchettata + il perceptor (non 18+1)"
        );
        for i in 0..windows.len() {
            assert!(out.translated_text.contains(&format!("W{i}")), "W{i} presente");
        }
    }

    /// Round-trip strutturale: il mock "traduce" ogni finestra restituendo il
    /// corpo ricevuto (echo) → il testo ricomposto è byte-identico alla pagina:
    /// i separatori di paragrafo DENTRO le finestre viaggiano col testo e tutti
    /// i 18 paragrafi restano nell'ordine giusto.
    #[test]
    fn packed_windows_roundtrip_preserves_all_paragraphs_in_order() {
        let c = conn();
        seed_session(&c);
        let page = sample_dense_page();
        let windows = expected_windows(&page);

        let mut responses: Vec<_> = windows
            .iter()
            .map(|w| Ok(resp(split_unit_body_sep(w).0, 10)))
            .collect();
        responses.push(Ok(resp(&content_with("ignored", "s", &[]), 7)));
        let client = MockClient::new(responses);

        let out = translate_page(&c, &client, &params_small(&page)).unwrap();

        assert_eq!(out.translated_text, page, "echo → ricomposizione byte-identica");
        let mut last = 0usize;
        for i in 1..=18 {
            let marker = format!("Paragrafo {i}:");
            let pos = out
                .translated_text
                .find(&marker)
                .unwrap_or_else(|| panic!("{marker} deve essere presente"));
            assert!(pos >= last, "{marker} nell'ordine originale");
            last = pos;
        }
    }

    /// Transizione dalla cache PER-PARAGRAFO (scritta dalla versione precedente
    /// del flusso) alla cache PER-FINESTRA: le vecchie righe (18 unit_index con
    /// l'hash dei singoli paragrafi) NON vengono mai servite (hash diverso →
    /// MISS), le finestre vengono tradotte e sovrascritte via UPSERT, e
    /// `unit_cache_prune` elimina le righe in coda oltre il nuovo unit_count.
    #[test]
    fn per_paragraph_cache_rows_transition_to_per_window_rows() {
        let c = conn();
        seed_session(&c);
        let page = sample_dense_page();

        // Semina la cache come l'avrebbe scritta la versione per-paragrafo.
        let paragraphs = split_into_units(&page, 4096, RATIO);
        assert!(paragraphs.len() >= 15, "precondition: molte unità per-paragrafo");
        for (i, u) in paragraphs.iter().enumerate() {
            let body = split_unit_body_sep(u).0;
            c.execute(
                "INSERT INTO unit_translations
                     (document_id, page_number, unit_index, target_language, source_hash, translated_text, created_at)
                 VALUES (1, 3, ?1, 'it', ?2, ?3, '2026-01-01T00:00:00Z')",
                params![i as i64, source_hash(body), format!("OLD{i}")],
            )
            .unwrap();
        }
        assert_eq!(unit_rows(&c), paragraphs.len() as i64, "cache per-paragrafo seminata");

        let windows = expected_windows(&page);
        let mut responses: Vec<_> = (0..windows.len())
            .map(|i| Ok(resp(&format!("NEW{i}"), 10)))
            .collect();
        responses.push(Ok(resp(&content_with("ignored", "s", &[]), 7)));
        let client = MockClient::new(responses);

        let out = translate_page(&c, &client, &params_small(&page)).unwrap();

        assert_eq!(
            client.calls(),
            windows.len() + 1,
            "le vecchie righe per-paragrafo sono MISS per hash: ogni finestra è tradotta"
        );
        assert!(!out.translated_text.contains("OLD"), "nessuna riga stale servita");
        for i in 0..windows.len() {
            assert!(out.translated_text.contains(&format!("NEW{i}")), "NEW{i} presente");
        }
        assert_eq!(
            unit_rows(&c),
            windows.len() as i64,
            "UPSERT sulle prime finestre + prune delle righe oltre il nuovo unit_count"
        );
    }

    /// Clamp L1: quando `budget_unit_text` è più stretto di PACK_TARGET_TOKENS
    /// (qui n_ctx minuscolo → il budget degenera al floor MIN_BUDGET_UNIT_TEXT
    /// = 256 < 512), le finestre rispettano il budget più stretto, non la
    /// costante: due paragrafi da ~195 token (assieme ~390 ≤ 512 ma > 256) NON
    /// vengono impacchettati insieme.
    #[test]
    fn pack_budget_is_clamped_to_a_tighter_unit_budget() {
        let c = conn();
        seed_session(&c);
        let filler =
            "testo di prova che riempie il paragrafo fino a circa duecento token stimati. "
                .repeat(10);
        let page = format!("Alfa. {}\n\nBeta. {}", filler.trim_end(), filler.trim_end());
        // Sanity: la coppia sta sotto la costante 512 ma sopra il floor 256.
        let pair_tokens = est_tokens(page.trim(), RATIO);
        assert!(
            pair_tokens > MIN_BUDGET_UNIT_TEXT && pair_tokens <= PACK_TARGET_TOKENS,
            "fixture calibrata male: {pair_tokens} token"
        );

        let client = MockClient::new(vec![
            Ok(resp("W0", 10)),
            Ok(resp("W1", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 7)),
        ]);
        let p = TranslateParams {
            document_id: 1,
            page_number: 3,
            target_language: "it",
            page_text: &page,
            model: "local-model",
            max_tokens: 2048,
            n_ctx: 1200, // budget_unit_text degenera al floor 256 (< 512)
            update_context: true,
            is_current: None,
        };
        let out = translate_page(&c, &client, &p).unwrap();

        assert_eq!(
            client.calls(),
            3,
            "due finestre (budget clampato a 256) + perceptor; senza clamp sarebbero 1+1"
        );
        assert!(out.translated_text.contains("W0") && out.translated_text.contains("W1"));
    }

    /// Dimensionamento output per finestra (ticket 04): con una finestra piena
    /// (~512 token) il cap iniziale lascia spazio a CoT (~500 misurati) +
    /// traduzione (~2× input) — ≥ ~1500 quando headroom e max_tokens lo
    /// consentono — e resta sempre ≤ headroom e ≤ max_tokens; le finestre
    /// piccole tengono il floor out_unit.
    #[test]
    fn window_output_cap_reserves_cot_and_respects_bounds() {
        // Finestra piena, default locali: 512×2 + 512 = 1536 ≥ ~1500.
        assert_eq!(window_output_cap(512, 2048, 3000), 1536);
        // Bounded dall'headroom residuo del contesto (prompt+output ≤ n_ctx)...
        assert_eq!(window_output_cap(512, 2048, 1200), 1200);
        // ...e dal max_tokens del provider.
        assert_eq!(window_output_cap(512, 1024, 3000), 1024);
        // Finestra piccola: mai sotto il floor OUT_UNIT_TOKENS.
        assert_eq!(window_output_cap(10, 2048, 3000), OUT_UNIT_TOKENS);
    }

    // --- STC-09: per-unit resume cache --------------------------------------

    fn unit_rows(c: &Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM unit_translations", [], |r| r.get(0))
            .unwrap()
    }
    fn page_rows(c: &Connection) -> i64 {
        c.query_row("SELECT COUNT(*) FROM translations_cache", [], |r| r.get(0))
            .unwrap()
    }

    /// source_hash is stable and content-sensitive (invalidation primitive).
    #[test]
    fn source_hash_is_stable_and_content_sensitive() {
        assert_eq!(source_hash("hello"), source_hash("hello"), "deterministic");
        assert_ne!(source_hash("hello"), source_hash("hell0"), "content-sensitive");
        // Hex, 16 chars (64-bit).
        assert_eq!(source_hash("x").len(), 16);
    }

    /// (b) STC-10 + STC-09: a perceptor failure caches the page (resilience) AND
    /// preserves the per-unit rows, so a retry is a page-level cache HIT (zero
    /// model calls). The failed run did not advance the summary.
    #[test]
    fn perceptor_failure_caches_page_and_units_retry_is_a_cache_hit() {
        let c = conn();
        seed_session(&c);
        let page = two_paragraphs(); // two packed windows under small n_ctx
        assert_eq!(
            pack_units(split_into_units(&page, 4096, RATIO), PACK_TARGET_TOKENS, RATIO).len(),
            2,
            "precondition: two packed windows"
        );

        // Run 1: both units OK, then the perceptor fails twice -> soft-swallowed.
        let client1 = MockClient::new(vec![
            Ok(resp("UNO", 10)),            // unit 0 ok -> cached
            Ok(resp("DUE", 10)),            // unit 1 ok -> cached
            Ok(resp("not json", 10)),       // perceptor malformed
            Ok(resp("still not json", 10)), // correction still malformed
        ]);
        let out1 = translate_page(&c, &client1, &params_small(&page)).unwrap();
        assert!(out1.translated_text.contains("UNO") && out1.translated_text.contains("DUE"));
        assert_eq!(out1.updated_summary, None, "summary not advanced by the failed perceptor");

        assert_eq!(page_rows(&c), 1, "page cached despite the perceptor failure (STC-10)");
        assert_eq!(unit_rows(&c), 2, "both translated units are preserved in the resume cache");
        // Summary was NOT advanced (perceptor never produced one).
        assert_eq!(crate::documents::get_rolling_summary(&c, 1).unwrap(), "");

        // Run 2 (retry): the page is a cache HIT -> ZERO model calls.
        let client2 = MockClient::new(vec![]); // any call panics on the empty queue
        let out2 = translate_page(&c, &client2, &params_small(&page)).unwrap();
        assert!(out2.from_cache, "healed page served from the page-level cache");
        assert_eq!(client2.calls(), 0, "retry makes no model call");
        assert_eq!(
            out2.translated_text, out1.translated_text,
            "reassembled translation is stable"
        );
    }

    /// (b') A LATER unit errors mid-page; a retry translates only the missing unit
    /// (the earlier one is a per-unit HIT), then the perceptor.
    #[test]
    fn mid_page_unit_error_then_retry_translates_only_the_missing_unit() {
        let c = conn();
        seed_session(&c);
        let page = two_paragraphs();

        // Run 1: unit 0 OK, unit 1 fails with a plain transport error -> aborts.
        let client1 = MockClient::new(vec![
            Ok(resp("UNO", 10)),
            Err(LlmError::Http("boom".into())),
        ]);
        let err = translate_page(&c, &client1, &params_small(&page)).unwrap_err();
        assert!(matches!(err, LlmError::Http(_)));
        assert_eq!(unit_rows(&c), 1, "only the first (successful) unit is cached");
        assert_eq!(page_rows(&c), 0, "page not cached");

        // Run 2: unit 0 HIT (no call); only unit 1 + the perceptor are called.
        let client2 = MockClient::new(vec![
            Ok(resp("DUE", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        let out = translate_page(&c, &client2, &params_small(&page)).unwrap();
        assert_eq!(client2.calls(), 2, "unit 0 reused; only unit 1 + perceptor called");
        assert!(out.translated_text.contains("UNO") && out.translated_text.contains("DUE"));
        assert!(out.translated_text.find("UNO").unwrap() < out.translated_text.find("DUE").unwrap());
    }

    /// (c) Invalidation: changing ONE paragraph's source (indices preserved)
    /// misses only that unit; the unchanged units are reused.
    #[test]
    fn changing_one_unit_source_invalidates_only_that_unit() {
        let c = conn();
        seed_session(&c);
        let page1 = three_paragraphs();
        assert_eq!(
            pack_units(split_into_units(&page1, 4096, RATIO), PACK_TARGET_TOKENS, RATIO).len(),
            3,
            "precondition: three packed windows"
        );

        let client1 = MockClient::new(vec![
            Ok(resp("T0", 10)),
            Ok(resp("T1", 10)),
            Ok(resp("T2", 10)),
            Ok(resp(&content_with("ignored", "s1", &[]), 10)),
        ]);
        translate_page(&c, &client1, &params_small(&page1)).unwrap();
        assert_eq!(client1.calls(), 4, "first run: three windows + perceptor");

        // Only the middle paragraph changes; boundaries (and thus indices) hold.
        let page2 = format!(
            "{}\n\n{}\n\n{}",
            big_para("AAA uno."),
            big_para("XXX due modificato."),
            big_para("CCC tre.")
        );
        let client2 = MockClient::new(vec![
            Ok(resp("T1-nuovo", 10)),                          // only the changed unit
            Ok(resp(&content_with("ignored", "s2", &[]), 10)), // perceptor re-runs
        ]);
        let out = translate_page(&c, &client2, &params_small(&page2)).unwrap();

        assert_eq!(client2.calls(), 2, "only the changed unit + perceptor; unchanged units reused");
        assert!(out.translated_text.contains("T0"), "unit 0 reused from cache");
        assert!(out.translated_text.contains("T1-nuovo"), "unit 1 re-translated");
        assert!(out.translated_text.contains("T2"), "unit 2 reused from cache");
        assert!(!out.translated_text.contains("T1\n"), "the stale unit-1 translation is not served");
    }

    /// (d) A different target_language misses the per-unit cache entirely; the
    /// original-language units remain cached and untouched.
    #[test]
    fn different_target_language_misses_the_per_unit_cache() {
        let c = conn();
        seed_session(&c);
        let page = two_paragraphs();

        let it = MockClient::new(vec![
            Ok(resp("UNO", 10)),
            Ok(resp("DUE", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        translate_page(&c, &it, &params_small(&page)).unwrap();

        // Same page/indices but a different target language -> all units miss.
        let fr = MockClient::new(vec![
            Ok(resp("UN", 10)),
            Ok(resp("DEUX", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        let p_fr = TranslateParams {
            target_language: "fr",
            ..params_small(&page)
        };
        let out = translate_page(&c, &fr, &p_fr).unwrap();

        assert_eq!(fr.calls(), 3, "different language: both units + perceptor re-translated");
        assert!(out.translated_text.contains("UN") && out.translated_text.contains("DEUX"));

        let it_units: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM unit_translations WHERE target_language='it'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let fr_units: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM unit_translations WHERE target_language='fr'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(it_units, 2, "Italian units untouched");
        assert_eq!(fr_units, 2, "French units cached separately");
    }

    /// (e) Reassembly from the per-unit cache is byte-identical to the uncached
    /// full run.
    #[test]
    fn cached_reassembly_matches_the_uncached_result() {
        let page = three_paragraphs();

        // Fresh full run (no per-unit reuse).
        let ca = conn();
        seed_session(&ca);
        let a = MockClient::new(vec![
            Ok(resp("T0", 10)),
            Ok(resp("T1", 10)),
            Ok(resp("T2", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        let out_a = translate_page(&ca, &a, &params_small(&page)).unwrap();

        // A run where a UNIT fails mid-page (earlier units cached, page NOT
        // cached), then a retry that re-translates only the missing unit and
        // reassembles from the per-unit cache for the rest.
        let cb = conn();
        seed_session(&cb);
        let b1 = MockClient::new(vec![
            Ok(resp("T0", 10)),
            Ok(resp("T1", 10)),
            Err(LlmError::Http("boom".into())), // unit 2 fails -> page aborts
        ]);
        translate_page(&cb, &b1, &params_small(&page)).unwrap_err();
        let b2 = MockClient::new(vec![
            Ok(resp("T2", 10)),                               // only the missing unit
            Ok(resp(&content_with("ignored", "s", &[]), 10)), // perceptor
        ]);
        let out_b = translate_page(&cb, &b2, &params_small(&page)).unwrap();

        assert_eq!(
            out_b.translated_text, out_a.translated_text,
            "per-unit-cache reassembly is byte-identical to the uncached result"
        );
    }

    /// A shrinking page prunes orphan high-index unit rows (hygiene).
    #[test]
    fn shrinking_page_prunes_orphan_unit_rows() {
        let c = conn();
        seed_session(&c);
        let page3 = three_paragraphs();
        let client1 = MockClient::new(vec![
            Ok(resp("T0", 10)),
            Ok(resp("T1", 10)),
            Ok(resp("T2", 10)),
            Ok(resp(&content_with("ignored", "s", &[]), 10)),
        ]);
        translate_page(&c, &client1, &params_small(&page3)).unwrap();
        assert_eq!(unit_rows(&c), 3, "three units cached");

        // The page now has only two paragraphs (same first two bodies -> HITs).
        let page2 = format!("{}\n\n{}", big_para("AAA uno."), big_para("BBB due."));
        let client2 = MockClient::new(vec![Ok(resp(&content_with("ignored", "s", &[]), 10))]);
        translate_page(&c, &client2, &params_small(&page2)).unwrap();
        assert_eq!(client2.calls(), 1, "two units reused; only the perceptor called");
        assert_eq!(unit_rows(&c), 2, "the orphan third unit row was pruned");
    }

    /// (g) A normal cached page still short-circuits without touching the
    /// perceptor OR the per-unit path (page-level 'done' semantics unchanged).
    #[test]
    fn full_page_cache_hit_still_short_circuits_before_units() {
        let c = conn();
        seed_session(&c);
        // First full run caches page + units.
        let client1 = MockClient::new(vec![
            Ok(resp("Ciao", 10)),
            Ok(resp(&valid_content(), 10)),
        ]);
        translate_page(&c, &client1, &params("Hello")).unwrap();
        assert_eq!(unit_rows(&c), 1, "single unit cached too");

        // Second visit: page-level hit -> no model call at all, perceptor untouched.
        let client2 = MockClient::new(vec![]); // any call panics
        let out = translate_page(&c, &client2, &params("Hello")).unwrap();
        assert!(out.from_cache, "served from the page-level cache");
        assert_eq!(client2.calls(), 0, "page hit short-circuits before the unit loop");
    }
}

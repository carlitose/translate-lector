// Pure, framework-free helpers for the page-translation flow (ticket 08).
// Kept out of the Svelte component so the error classification is unit-testable
// without a DOM or the Tauri bridge.

/** Shape returned by the `translate_page` core command. */
export interface TranslationResult {
  translated_text: string;
  from_cache: boolean;
  total_tokens: number | null;
  /**
   * Rolling summary after this page (percettore, ticket 09). `null` on a cache
   * hit, where the percettore is not re-run. The glossary panel (ticket 10)
   * will surface glossary changes; the summary is exposed here for state.
   */
  updated_summary?: string | null;
  /**
   * True when the per-page perceptor-update failed to advance the document
   * context (STC-10 observability, ticket 02): the strict JSON could not be
   * parsed even after the correction retry, or the call errored — so the rolling
   * summary was NOT advanced for this page. The translation itself is always
   * produced and cached regardless (STC-10 invariant); glossary terms may still
   * have been recovered. `false`/absent on a cache hit, on prefetch, and on a
   * full success. Surfaced as a non-intrusive status note (never a modal).
   */
  perceptor_update_failed?: boolean;
}

/**
 * Shape returned by the `advance_context` core command (two-phase arrival,
 * ticket 01). Phase 2 runs the once-per-page perceptor-update OFF the response
 * path: `translate_page` delivers the text instantly, then the frontend calls
 * `advance_context` to grow the summary/glossary. A re-visit of an already
 * advanced page is a no-op (`advanced: false`, no failure).
 */
export interface AdvanceContextResult {
  /** True when the rolling summary was advanced this call (full perceptor success). */
  advanced: boolean;
  /** Rolling summary after this page; `null` on a no-op/recovery/failure. */
  updated_summary?: string | null;
  /**
   * True when the perceptor could not advance the context (unparseable JSON even
   * after the correction retry, or a transport error). The translation is always
   * delivered regardless; surfaced as the non-intrusive context note. Never true
   * on a no-op (re-visit of a healthy page).
   */
  perceptor_update_failed?: boolean;
  /** Sum of the perceptor call(s) tokens; `null` on a no-op or no usage. */
  total_tokens?: number | null;
}

function errorText(err: unknown): string {
  return typeof err === 'string' ? err : String(err);
}

/**
 * True when a rejected `invoke` error is the EC03 "missing/invalid API key"
 * case. The core encodes it by prefixing the message with `EC03:`.
 */
export function isMissingKeyError(err: unknown): boolean {
  return errorText(err).includes('EC03');
}

/** True when the error is the EC02 "no connection" case (offline). */
export function isOfflineError(err: unknown): boolean {
  return errorText(err).includes('EC02');
}

/** True when the error is the EC07 "rate limit" case (429). */
export function isRateLimitError(err: unknown): boolean {
  return errorText(err).includes('EC07');
}

/**
 * True when the error is the EC08 "output budget exhausted" case: the model
 * returned empty content with `finish_reason: length` (a reasoning model that
 * burned its completion budget in a small context window, ticket 03).
 */
export function isOutputBudgetError(err: unknown): boolean {
  return errorText(err).includes('EC08');
}

/** User-facing hint shown when the key is missing (points at ⚙️). Provider-neutral:
 *  the active provider may be OpenRouter or a local server, and its key is stored
 *  per-provider, so the hint must not name a specific provider (ticket 11). */
export const MISSING_KEY_HINT =
  'API key mancante o non valida per il provider selezionato. Configurala in ⚙️ (in alto a destra).';

/** EC02: offline — cached pages stay readable, new ones can't be translated. */
export const OFFLINE_HINT =
  'Nessuna connessione. Le pagine già tradotte restano leggibili dalla cache; ' +
  'le nuove verranno tradotte al ritorno della rete.';

/** EC07: rate limit — the core already retried with backoff before surfacing. */
export const RATE_LIMIT_HINT =
  'Limite di richieste raggiunto (rate limit). Riprova tra poco.';

/** EC08: the local model ran out of completion budget (reasoning in a small
 *  context window). Actionable: switch to a non-reasoning model, shorten the
 *  text, or raise the server's context (n_ctx). */
export const OUTPUT_BUDGET_HINT =
  'Il modello locale ha esaurito il budget di token (probabile reasoning entro una finestra piccola). ' +
  'Usa un modello non-reasoning, riduci il testo, o aumenta il context (n_ctx) del server.';

/**
 * Normalise a caught `invoke` error into a message for the right pane. The
 * edge cases get dedicated hints (EC03 → ⚙️, EC02 → offline, EC07 → rate
 * limit, EC08 → output-budget exhausted); anything else is passed through as
 * text.
 */
export function translationErrorMessage(err: unknown): string {
  if (isMissingKeyError(err)) return MISSING_KEY_HINT;
  if (isOfflineError(err)) return OFFLINE_HINT;
  if (isRateLimitError(err)) return RATE_LIMIT_HINT;
  if (isOutputBudgetError(err)) return OUTPUT_BUDGET_HINT;
  return errorText(err);
}

// --- Per-page status (§3.1 bottom bar) --------------------------------------

/** Translation status of the current page, shown in the bottom bar (§3.1). */
export type PageStatus = 'idle' | 'loading' | 'cached' | 'translated' | 'error';

/** Bottom-bar label for a page status (matches §3.1 "● Tradotto (cache)"). */
export function pageStatusLabel(status: PageStatus): string {
  switch (status) {
    case 'loading':
      return '⏳ Traduzione in corso…';
    case 'cached':
      return '● Tradotto (cache)';
    case 'translated':
      return '● Tradotto';
    case 'error':
      return '⚠ Errore di traduzione';
    default:
      return '';
  }
}

/** The status of a completed translation, distinguishing a cache hit. */
export function resultStatus(fromCache: boolean): PageStatus {
  return fromCache ? 'cached' : 'translated';
}

/**
 * Non-intrusive note shown when the per-page perceptor-update failed to advance
 * the document context (ticket 02). The page IS translated and readable; only
 * the rolling summary/glossary context did not advance for this page. Kept short
 * and informational — no modal, no error styling (the translation succeeded).
 */
export const CONTEXT_NOT_ADVANCED_HINT =
  'ⓘ Contesto non aggiornato per questa pagina (glossario/riassunto): la traduzione è comunque completa.';

/**
 * The context note to show for a completed translation result. Returns the
 * non-intrusive hint when the core reported a perceptor-update failure, or an
 * empty string otherwise (full success, cache hit, prefetch, or an older core
 * that does not send the flag).
 */
export function contextNote(
  result: Partial<TranslationResult> | Partial<AdvanceContextResult>
): string {
  return result.perceptor_update_failed ? CONTEXT_NOT_ADVANCED_HINT : '';
}

// --- Obsolete-request handling (ticket 12) ----------------------------------

/**
 * Identity of a translation request: navigating away while a page translates
 * must not clobber the current view, so a returned result is only applied when
 * its key still matches the page/language on screen.
 */
export interface RequestKey {
  documentId: number;
  pageNumber: number;
  targetLanguage: string;
}

/** Stable string identity for a request (document, page, language). */
export function requestKey(key: RequestKey): string {
  return `${key.documentId}|${key.pageNumber}|${key.targetLanguage}`;
}

/**
 * Whether a result for `requested` is still relevant to the page currently on
 * screen (`current`). A mismatch means the user navigated away — the stale
 * result must be dropped, not shown.
 */
export function isCurrentRequest(requested: RequestKey, current: RequestKey): boolean {
  return requestKey(requested) === requestKey(current);
}

// --- Page↔text invariant (ticket 16) ----------------------------------------

/**
 * Gate for the translation effect: only translate when the extracted text
 * actually belongs to the page currently on screen. On navigation `currentPage`
 * is set synchronously while `reconstructedText` is refreshed only after the
 * async page render, so the effect can otherwise fire once with the PREVIOUS
 * page's text under the new page number — poisoning the per-page cache. Requiring
 * `reconstructedPage === currentPage` (and non-empty text) closes that race so
 * `translate_page` is never called with a `page_text` that does not match its
 * `page_number`.
 */
export function shouldTranslate(
  reconstructedPage: number,
  currentPage: number,
  text: string
): boolean {
  return reconstructedPage === currentPage && text.trim().length > 0;
}

// --- Concurrent-navigation guard (finding 2) --------------------------------

/**
 * True when the navigation identified by `myToken` is still the latest one
 * (`currentToken`), i.e. no newer navigation started while its async page
 * render was in flight. Callers capture a monotonically increasing token at
 * the start of a render and only COMMIT the page↔text state when this holds,
 * so a slow, superseded render can never overwrite the state of the page now
 * on screen (which would otherwise leave `reconstructedPage !== currentPage`
 * and silently skip a later needed re-translate).
 */
export function isLatestNav(myToken: number, currentToken: number): boolean {
  return myToken === currentToken;
}

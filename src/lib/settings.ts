// Pure, framework-free helpers for the full Settings screen (ticket 13, §3.5).
// Kept out of the Svelte component so the language/summary-limit logic is
// unit-testable without a DOM or the Tauri bridge. Mirrors the core defaults so
// the UI shows the same choices the core would use.

/** Curated target-language list (decision D4) — 15 common languages. The user
 *  can still type any other language via the free-text field. Shared by the
 *  reader's language selector and the Settings default-language picker. */
export const LANGUAGES: { code: string; label: string }[] = [
  { code: 'it', label: 'Italiano' },
  { code: 'en', label: 'Inglese' },
  { code: 'es', label: 'Spagnolo' },
  { code: 'fr', label: 'Francese' },
  { code: 'de', label: 'Tedesco' },
  { code: 'pt', label: 'Portoghese' },
  { code: 'nl', label: 'Olandese' },
  { code: 'ru', label: 'Russo' },
  { code: 'zh', label: 'Cinese' },
  { code: 'ja', label: 'Giapponese' },
  { code: 'ko', label: 'Coreano' },
  { code: 'ar', label: 'Arabo' },
  { code: 'hi', label: 'Hindi' },
  { code: 'tr', label: 'Turco' },
  { code: 'pl', label: 'Polacco' }
];

/** Default target language when none is configured (decision D4: Italiano).
 *  Mirrors the core's `DEFAULT_TARGET_LANGUAGE`. */
export const DEFAULT_TARGET_LANGUAGE = 'it';

/** Default rolling-summary token budget (decision D5: ~1000). Mirrors the
 *  core's `DEFAULT_SUMMARY_TOKEN_LIMIT`. */
export const DEFAULT_SUMMARY_TOKEN_LIMIT = 1000;

/** Whether a language code is one of the curated common options. */
export function isCommonLanguage(code: string): boolean {
  return LANGUAGES.some((l) => l.code === code);
}

/**
 * Resolve the effective default language: the stored value when it is a
 * non-blank string, otherwise the default. Mirrors the core's fallback.
 */
export function resolveLanguage(stored: string | null | undefined): string {
  if (typeof stored === 'string' && stored.trim().length > 0) {
    return stored.trim();
  }
  return DEFAULT_TARGET_LANGUAGE;
}

/**
 * Parse a summary-limit input into a positive integer, falling back to the
 * default when the value is blank, non-numeric or not positive. Mirrors the
 * core's `get_summary_token_limit` fallback so the UI and core agree.
 */
export function parseSummaryLimit(input: string | number | null | undefined): number {
  const n =
    typeof input === 'number' ? input : Number.parseInt(String(input ?? '').trim(), 10);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : DEFAULT_SUMMARY_TOKEN_LIMIT;
}

/**
 * Normalise a stored prefetch flag into a boolean. Mirrors the core's
 * `get_prefetch_enabled`: defaults ON (decision D5) when unset; a stored
 * `"false"`/`"0"` (case-insensitive) turns it off, anything else keeps it on.
 */
export function resolvePrefetch(stored: string | null | undefined): boolean {
  if (stored == null) return true;
  const v = stored.trim().toLowerCase();
  if (v === 'false' || v === '0') return false;
  if (v.length === 0) return true;
  return true;
}

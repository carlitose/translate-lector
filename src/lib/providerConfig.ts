// Pure, framework-free helpers for the minimal provider-config panel
// (ticket 07). Kept out of the Svelte component so the model-default and
// key-validation logic is unit-testable without a DOM.

/** Default OpenRouter model when none is configured (decision D5). Updated
 *  (July 2026, ticket 14) to a current model that supports temperature +
 *  structured_outputs, so the default call is not rejected by the router. */
export const DEFAULT_MODEL = 'anthropic/claude-sonnet-4.6';

/** A few common OpenRouter model ids offered in the dropdown (D5), refreshed to
 *  the current July-2026 catalog (ticket 14). The user can still type any other
 *  id via the free-text field. */
export const COMMON_MODELS: { id: string; label: string }[] = [
  { id: 'anthropic/claude-opus-4.8', label: 'Claude Opus 4.8 (Anthropic)' },
  { id: 'anthropic/claude-sonnet-4.6', label: 'Claude Sonnet 4.6 (Anthropic)' },
  { id: 'anthropic/claude-haiku-4.5', label: 'Claude Haiku 4.5 (Anthropic)' },
  { id: 'google/gemini-3.5-flash', label: 'Gemini 3.5 Flash (Google)' },
  { id: 'google/gemini-3.1-pro-preview', label: 'Gemini 3.1 Pro Preview (Google)' },
  { id: 'openai/gpt-4.1', label: 'GPT-4.1 (OpenAI)' }
];

/**
 * Resolve the effective model id: the stored value when it is a non-blank
 * string, otherwise the default. Mirrors the core's `get_model` fallback so
 * the UI shows the same choice the core would use.
 */
export function resolveModel(stored: string | null | undefined): string {
  if (typeof stored === 'string' && stored.trim().length > 0) {
    return stored.trim();
  }
  return DEFAULT_MODEL;
}

/** Whether a typed API key is acceptable to save (non-empty once trimmed). */
export function isValidKey(key: string | null | undefined): boolean {
  return typeof key === 'string' && key.trim().length > 0;
}

/** Whether a given model id is one of the curated common options. */
export function isCommonModel(id: string): boolean {
  return COMMON_MODELS.some((m) => m.id === id);
}

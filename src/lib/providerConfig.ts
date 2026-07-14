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

/**
 * A selectable LLM provider preset for the settings UI. Mirrors the core's
 * built-in `provider_presets()` (`settings.rs`): `id`/`label`/`base_url` are the
 * built-in defaults; `base_url`/`model` are user-overridable at runtime and
 * resolved by the core via `get_provider_config`. Decision D5 dropped the
 * `requires_key` flag — every provider always saves a key; local servers just
 * suggest a harmless dummy placeholder so the field "just works" without auth.
 */
export interface ProviderPreset {
  /** Stable id shared with the core: `openrouter` | `unsloth` | … */
  id: string;
  /** UI label, e.g. "OpenRouter (cloud)". */
  label: string;
  /** Built-in default `/v1/chat/completions` endpoint (overridable). */
  base_url: string;
  /** OpenRouter (cloud) offers the curated {@link COMMON_MODELS} dropdown; local
   *  providers use a free-text model field (the loaded model tag varies). */
  cloud: boolean;
  /** Suggested placeholder for the API-key field (D5). Local no-auth servers
   *  suggest a dummy like `local`; auth'd providers hint their key format. */
  dummyKey?: string;
}

/**
 * Built-in provider presets, mirroring the core's `provider_presets()` in
 * `src-tauri/src/settings.rs` (design §2). The active provider defaults to
 * `unsloth` (decision D3) in the core; the UI reads the real value via
 * `get_active_provider`. Base-URLs are starting defaults the user can override.
 */
export const PROVIDERS: ProviderPreset[] = [
  {
    id: 'openrouter',
    label: 'OpenRouter (cloud)',
    base_url: 'https://openrouter.ai/api/v1/chat/completions',
    cloud: true,
    dummyKey: 'sk-or-…'
  },
  {
    id: 'unsloth',
    label: 'Unsloth Studio (locale)',
    base_url: 'http://localhost:8888/v1/chat/completions',
    cloud: false,
    dummyKey: 'sk-unsloth-…'
  },
  {
    id: 'lmstudio',
    label: 'LM Studio (locale)',
    base_url: 'http://localhost:1234/v1/chat/completions',
    cloud: false,
    dummyKey: 'local'
  },
  {
    id: 'ollama',
    label: 'Ollama (locale)',
    base_url: 'http://localhost:11434/v1/chat/completions',
    cloud: false,
    dummyKey: 'local'
  },
  {
    id: 'llamaserver',
    label: 'llama.cpp server (locale)',
    base_url: 'http://127.0.0.1:8080/v1/chat/completions',
    cloud: false,
    dummyKey: 'local'
  }
];

/** The preset for `id`, or `undefined` when the id is not a known provider. */
export function providerById(id: string | null | undefined): ProviderPreset | undefined {
  return PROVIDERS.find((p) => p.id === id);
}

/**
 * Whether `id` is a known **local** provider (a non-cloud preset such as Unsloth
 * / LM Studio / Ollama / llama.cpp). Only local providers get the onboarding
 * reachability hint — the cloud provider (OpenRouter) never does. An unknown id
 * is treated as non-local (no hint). Ticket 09 (D3/D7).
 */
export function isLocalProvider(id: string | null | undefined): boolean {
  const p = providerById(id);
  return p !== undefined && !p.cloud;
}

/**
 * Whether to show the non-blocking "local server unreachable" onboarding hint:
 * only when the active provider is local AND the reachability probe came back
 * `false` (ticket 09). Cloud providers, reachable local servers, and an
 * indeterminate probe never trigger it. It never blocks using the app or
 * switching to OpenRouter — it is purely informational.
 */
export function shouldShowLocalHint(
  id: string | null | undefined,
  reachable: boolean
): boolean {
  return isLocalProvider(id) && reachable === false;
}

/**
 * Onboarding hint shown when the active local provider is not reachable (D3/D7).
 * Mirrors the core `LlmError::Unreachable` copy but stays generic (no base_url),
 * since it fires proactively from a health check, not from a failed translation.
 */
export const LOCAL_UNREACHABLE_HINT =
  'Server locale non raggiungibile. Avvia il server (es. Unsloth Studio) ' +
  'oppure apri ⚙️ Impostazioni per verificarne l’indirizzo o passare a OpenRouter.';

/**
 * Resolve the effective base-URL for a provider: the stored override when it is a
 * non-blank string, otherwise the provider's built-in default (empty for an
 * unknown id). Mirrors the core's `get_provider_config` base-URL fallback.
 */
export function resolveBaseUrl(
  stored: string | null | undefined,
  providerId: string | null | undefined
): string {
  if (typeof stored === 'string' && stored.trim().length > 0) {
    return stored.trim();
  }
  return providerById(providerId)?.base_url ?? '';
}

/**
 * Whether a typed API key is acceptable to save. Under decision D5 every
 * provider always carries a key, so this is a plain non-empty check (an alias of
 * {@link isValidKey}); local providers rely on the suggested dummy placeholder.
 */
export function keyAcceptable(key: string | null | undefined): boolean {
  return isValidKey(key);
}

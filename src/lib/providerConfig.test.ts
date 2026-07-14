import { describe, it, expect } from 'vitest';
import {
  DEFAULT_MODEL,
  resolveModel,
  isValidKey,
  isCommonModel,
  COMMON_MODELS,
  PROVIDERS,
  providerById,
  resolveBaseUrl,
  keyAcceptable,
  isLocalProvider,
  shouldShowLocalHint,
  LOCAL_UNREACHABLE_HINT,
  DEFAULT_N_CTX_LOCAL,
  DEFAULT_N_CTX_CLOUD,
  resolveNctx
} from './providerConfig';

describe('resolveModel', () => {
  it('returns the default when unset (null/undefined)', () => {
    expect(resolveModel(null)).toBe(DEFAULT_MODEL);
    expect(resolveModel(undefined)).toBe(DEFAULT_MODEL);
  });

  it('returns the default for a blank/whitespace value', () => {
    expect(resolveModel('')).toBe(DEFAULT_MODEL);
    expect(resolveModel('   ')).toBe(DEFAULT_MODEL);
  });

  it('returns the trimmed stored value when present', () => {
    expect(resolveModel(' openai/gpt-4o ')).toBe('openai/gpt-4o');
  });

  it('default is the current (July 2026) model and is in the common list', () => {
    expect(DEFAULT_MODEL).toBe('anthropic/claude-sonnet-4.6');
    expect(isCommonModel(DEFAULT_MODEL)).toBe(true);
  });

  it('common models are the refreshed July-2026 catalog (ticket 14)', () => {
    expect(COMMON_MODELS.map((m) => m.id)).toEqual([
      'anthropic/claude-opus-4.8',
      'anthropic/claude-sonnet-4.6',
      'anthropic/claude-haiku-4.5',
      'google/gemini-3.5-flash',
      'google/gemini-3.1-pro-preview',
      'openai/gpt-4.1'
    ]);
    // The old reasoning sonnet-5 default (bug #1) is gone.
    expect(isCommonModel('anthropic/claude-sonnet-5')).toBe(false);
  });
});

describe('isValidKey', () => {
  it('rejects empty/whitespace/nullish keys', () => {
    expect(isValidKey('')).toBe(false);
    expect(isValidKey('   ')).toBe(false);
    expect(isValidKey(null)).toBe(false);
    expect(isValidKey(undefined)).toBe(false);
  });

  it('accepts a non-empty key', () => {
    expect(isValidKey('sk-or-abc123')).toBe(true);
  });
});

describe('isCommonModel', () => {
  it('recognises curated ids and rejects unknown ones', () => {
    expect(isCommonModel(COMMON_MODELS[0].id)).toBe(true);
    expect(isCommonModel('some/unknown-model')).toBe(false);
  });
});

describe('PROVIDERS', () => {
  it('mirrors the core presets: the five expected ids in order', () => {
    expect(PROVIDERS.map((p) => p.id)).toEqual([
      'openrouter',
      'unsloth',
      'lmstudio',
      'ollama',
      'llamaserver'
    ]);
  });

  it('openrouter is the only cloud provider and points at OpenRouter', () => {
    const cloud = PROVIDERS.filter((p) => p.cloud).map((p) => p.id);
    expect(cloud).toEqual(['openrouter']);
    expect(providerById('openrouter')?.base_url).toBe(
      'https://openrouter.ai/api/v1/chat/completions'
    );
  });

  it('every local provider suggests a dummy key placeholder (D5)', () => {
    for (const p of PROVIDERS.filter((p) => !p.cloud)) {
      expect(p.dummyKey && p.dummyKey.length > 0).toBe(true);
    }
    expect(providerById('lmstudio')?.dummyKey).toBe('local');
  });
});

describe('n_ctx presets (ticket 07)', () => {
  it('openrouter (cloud) keeps a large n_ctx so the budget never constrains', () => {
    expect(providerById('openrouter')?.n_ctx).toBe(DEFAULT_N_CTX_CLOUD);
    expect(DEFAULT_N_CTX_CLOUD).toBe(128000);
  });

  it('every local provider defaults to the small local n_ctx', () => {
    for (const p of PROVIDERS.filter((p) => !p.cloud)) {
      expect(p.n_ctx).toBe(DEFAULT_N_CTX_LOCAL);
    }
    expect(DEFAULT_N_CTX_LOCAL).toBe(4096);
  });
});

describe('resolveNctx', () => {
  it('returns the parsed stored override when it is a positive integer', () => {
    expect(resolveNctx('8192', 'unsloth')).toBe(8192);
    expect(resolveNctx('  8192  ', 'unsloth')).toBe(8192);
  });

  it('falls back to the provider preset when unset/blank/invalid/zero', () => {
    expect(resolveNctx(null, 'unsloth')).toBe(DEFAULT_N_CTX_LOCAL);
    expect(resolveNctx('   ', 'lmstudio')).toBe(DEFAULT_N_CTX_LOCAL);
    expect(resolveNctx('abc', 'ollama')).toBe(DEFAULT_N_CTX_LOCAL);
    expect(resolveNctx('0', 'unsloth')).toBe(DEFAULT_N_CTX_LOCAL);
    expect(resolveNctx(null, 'openrouter')).toBe(DEFAULT_N_CTX_CLOUD);
  });

  it('falls back to the conservative local default for an unknown provider', () => {
    expect(resolveNctx(undefined, 'mystery')).toBe(DEFAULT_N_CTX_LOCAL);
  });
});

describe('providerById', () => {
  it('resolves a known id and returns undefined otherwise', () => {
    expect(providerById('ollama')?.label).toBe('Ollama (locale)');
    expect(providerById('nope')).toBeUndefined();
    expect(providerById(null)).toBeUndefined();
    expect(providerById(undefined)).toBeUndefined();
  });
});

describe('resolveBaseUrl', () => {
  it('returns the trimmed stored override when present', () => {
    expect(resolveBaseUrl('  http://host:9/v1/chat/completions  ', 'ollama')).toBe(
      'http://host:9/v1/chat/completions'
    );
  });

  it('falls back to the preset default when unset/blank', () => {
    expect(resolveBaseUrl(null, 'lmstudio')).toBe('http://localhost:1234/v1/chat/completions');
    expect(resolveBaseUrl('   ', 'ollama')).toBe('http://localhost:11434/v1/chat/completions');
  });

  it('yields an empty string for an unknown provider with no override', () => {
    expect(resolveBaseUrl(undefined, 'mystery')).toBe('');
  });
});

describe('keyAcceptable', () => {
  it('behaves like isValidKey (non-empty) under D5', () => {
    expect(keyAcceptable('')).toBe(false);
    expect(keyAcceptable('   ')).toBe(false);
    expect(keyAcceptable(null)).toBe(false);
    expect(keyAcceptable('local')).toBe(true);
    expect(keyAcceptable('sk-or-abc')).toBe(isValidKey('sk-or-abc'));
  });
});

describe('isLocalProvider', () => {
  it('is true for every local preset and false for the cloud one', () => {
    expect(isLocalProvider('unsloth')).toBe(true);
    expect(isLocalProvider('lmstudio')).toBe(true);
    expect(isLocalProvider('ollama')).toBe(true);
    expect(isLocalProvider('llamaserver')).toBe(true);
    expect(isLocalProvider('openrouter')).toBe(false);
  });

  it('treats unknown/nullish ids as non-local (no hint)', () => {
    expect(isLocalProvider('mystery')).toBe(false);
    expect(isLocalProvider(null)).toBe(false);
    expect(isLocalProvider(undefined)).toBe(false);
  });
});

describe('shouldShowLocalHint', () => {
  it('shows the hint only for a local provider that is unreachable', () => {
    expect(shouldShowLocalHint('unsloth', false)).toBe(true);
  });

  it('never shows the hint when the local server is reachable', () => {
    expect(shouldShowLocalHint('unsloth', true)).toBe(false);
  });

  it('never shows the hint for the cloud provider, even when unreachable', () => {
    // D4/D3: OpenRouter is cloud — no local-server onboarding hint.
    expect(shouldShowLocalHint('openrouter', false)).toBe(false);
    expect(shouldShowLocalHint('openrouter', true)).toBe(false);
  });

  it('has actionable Italian copy pointing at the server and ⚙️', () => {
    expect(LOCAL_UNREACHABLE_HINT).toContain('Server locale non raggiungibile');
    expect(LOCAL_UNREACHABLE_HINT).toContain('⚙️');
  });
});

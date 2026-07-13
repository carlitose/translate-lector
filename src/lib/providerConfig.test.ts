import { describe, it, expect } from 'vitest';
import {
  DEFAULT_MODEL,
  resolveModel,
  isValidKey,
  isCommonModel,
  COMMON_MODELS
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

  it('default is the D5 model and is in the common list', () => {
    expect(DEFAULT_MODEL).toBe('anthropic/claude-sonnet-5');
    expect(isCommonModel(DEFAULT_MODEL)).toBe(true);
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

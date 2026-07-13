import { describe, it, expect } from 'vitest';
import {
  LANGUAGES,
  DEFAULT_TARGET_LANGUAGE,
  DEFAULT_SUMMARY_TOKEN_LIMIT,
  isCommonLanguage,
  resolveLanguage,
  parseSummaryLimit,
  resolvePrefetch
} from './settings';

describe('LANGUAGES (D4 curated list)', () => {
  it('offers the 15 curated languages with Italiano as the default', () => {
    expect(LANGUAGES).toHaveLength(15);
    expect(LANGUAGES[0]).toEqual({ code: 'it', label: 'Italiano' });
    expect(DEFAULT_TARGET_LANGUAGE).toBe('it');
    expect(isCommonLanguage('it')).toBe(true);
  });
});

describe('isCommonLanguage', () => {
  it('recognises curated codes and rejects unknown ones', () => {
    expect(isCommonLanguage('es')).toBe(true);
    expect(isCommonLanguage('xx')).toBe(false);
  });
});

describe('resolveLanguage', () => {
  it('returns the default when unset/blank', () => {
    expect(resolveLanguage(null)).toBe('it');
    expect(resolveLanguage(undefined)).toBe('it');
    expect(resolveLanguage('')).toBe('it');
    expect(resolveLanguage('   ')).toBe('it');
  });

  it('returns the trimmed stored value when present', () => {
    expect(resolveLanguage(' es ')).toBe('es');
    expect(resolveLanguage('klingon')).toBe('klingon');
  });
});

describe('parseSummaryLimit', () => {
  it('falls back to the default for blank/non-numeric/non-positive input', () => {
    expect(parseSummaryLimit('')).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
    expect(parseSummaryLimit('   ')).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
    expect(parseSummaryLimit('abc')).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
    expect(parseSummaryLimit('0')).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
    expect(parseSummaryLimit('-50')).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
    expect(parseSummaryLimit(null)).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
    expect(parseSummaryLimit(undefined)).toBe(DEFAULT_SUMMARY_TOKEN_LIMIT);
  });

  it('parses a positive integer (flooring floats)', () => {
    expect(parseSummaryLimit('800')).toBe(800);
    expect(parseSummaryLimit(1200)).toBe(1200);
    expect(parseSummaryLimit('1000.9')).toBe(1000);
  });

  it('default matches the D5 budget', () => {
    expect(DEFAULT_SUMMARY_TOKEN_LIMIT).toBe(1000);
  });
});

describe('resolvePrefetch', () => {
  it('defaults ON when unset (D5)', () => {
    expect(resolvePrefetch(null)).toBe(true);
    expect(resolvePrefetch(undefined)).toBe(true);
    expect(resolvePrefetch('')).toBe(true);
  });

  it('turns off only for false/0 (case-insensitive)', () => {
    expect(resolvePrefetch('false')).toBe(false);
    expect(resolvePrefetch('FALSE')).toBe(false);
    expect(resolvePrefetch('0')).toBe(false);
    expect(resolvePrefetch('true')).toBe(true);
    expect(resolvePrefetch('1')).toBe(true);
  });
});

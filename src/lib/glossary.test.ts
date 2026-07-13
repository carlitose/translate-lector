import { describe, it, expect } from 'vitest';
import {
  isValidTranslation,
  toUpdateArgs,
  typeLabel,
  type GlossaryTermRecord
} from './glossary';

const base: GlossaryTermRecord = {
  id: 7,
  source_term: 'board',
  translation: 'consiglio',
  term_type: 'tecnico',
  locked: false,
  note: '',
  first_seen_page: 3
};

describe('isValidTranslation', () => {
  it('rejects empty/whitespace translations', () => {
    expect(isValidTranslation('')).toBe(false);
    expect(isValidTranslation('   ')).toBe(false);
    expect(isValidTranslation(null)).toBe(false);
    expect(isValidTranslation(undefined)).toBe(false);
  });

  it('accepts a non-empty translation', () => {
    expect(isValidTranslation('consiglio')).toBe(true);
    expect(isValidTranslation('  consiglio  ')).toBe(true);
  });
});

describe('toUpdateArgs', () => {
  it('maps a term to the core invoke args, trimming text', () => {
    const edited: GlossaryTermRecord = {
      ...base,
      translation: '  consiglio di amministrazione  ',
      note: '  vincolante  ',
      locked: true
    };
    expect(toUpdateArgs(edited)).toEqual({
      id: 7,
      translation: 'consiglio di amministrazione',
      note: 'vincolante',
      locked: true
    });
  });

  it('carries only the mutable fields (no source_term/type/page)', () => {
    const args = toUpdateArgs(base);
    expect(Object.keys(args).sort()).toEqual(['id', 'locked', 'note', 'translation']);
  });
});

describe('typeLabel', () => {
  it('falls back to a dash for empty types', () => {
    expect(typeLabel('')).toBe('—');
    expect(typeLabel('   ')).toBe('—');
  });

  it('passes through a real type', () => {
    expect(typeLabel('tecnico')).toBe('tecnico');
  });
});

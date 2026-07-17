import { describe, it, expect } from 'vitest';
import {
  isValidTranslation,
  isValidNewTerm,
  toUpdateArgs,
  toAddArgs,
  typeLabel,
  pageLabel,
  type GlossaryTermRecord,
  type AddTermFormValues,
  type AddTermResult
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

describe('isValidNewTerm', () => {
  it('requires both source term and translation (trimmed) to be non-empty', () => {
    expect(isValidNewTerm('', 'consiglio')).toBe(false);
    expect(isValidNewTerm('   ', 'consiglio')).toBe(false);
    expect(isValidNewTerm('board', '')).toBe(false);
    expect(isValidNewTerm('board', '   ')).toBe(false);
    expect(isValidNewTerm('', '')).toBe(false);
    expect(isValidNewTerm(null, undefined)).toBe(false);
  });

  it('accepts when both are non-empty once trimmed', () => {
    expect(isValidNewTerm('board', 'consiglio')).toBe(true);
    expect(isValidNewTerm('  board  ', '  consiglio  ')).toBe(true);
  });
});

describe('toAddArgs', () => {
  const form: AddTermFormValues = {
    sourceTerm: '  board  ',
    translation: '  consiglio di amministrazione  ',
    termType: '  tecnico  ',
    note: '  vincolante  ',
    locked: true
  };

  it('maps form fields to the add_glossary_term invoke args, trimming text', () => {
    expect(toAddArgs(42, form)).toEqual({
      documentId: 42,
      sourceTerm: 'board',
      translation: 'consiglio di amministrazione',
      termType: 'tecnico',
      note: 'vincolante',
      locked: true
    });
  });

  it('carries exactly the fields the command expects', () => {
    expect(Object.keys(toAddArgs(42, form)).sort()).toEqual([
      'documentId',
      'locked',
      'note',
      'sourceTerm',
      'termType',
      'translation'
    ]);
  });

  it('preserves the locked flag when false', () => {
    expect(toAddArgs(1, { ...form, locked: false }).locked).toBe(false);
  });
});

describe('AddTermResult', () => {
  it('accepts the two documented shapes returned by add_glossary_term', () => {
    const inserted: AddTermResult = { status: 'inserted', id: 42 };
    const duplicate: AddTermResult = { status: 'duplicate', id: 7 };
    expect(inserted.status).toBe('inserted');
    expect(duplicate.status).toBe('duplicate');
    expect([inserted, duplicate].map((r) => r.id)).toEqual([42, 7]);
  });
});

describe('pageLabel', () => {
  it('shows "manuale" for the sentinel page 0 (manually added term)', () => {
    expect(pageLabel(0)).toBe('manuale');
  });

  it('shows the page number for perceptor-collected terms', () => {
    expect(pageLabel(3)).toBe('3');
    expect(pageLabel(12)).toBe('12');
  });
});

import { describe, it, expect } from 'vitest';
import {
  restoreDecision,
  fileName,
  clampPage,
  type LastSession
} from './session';

const session: LastSession = {
  session_id: 1,
  document_id: 2,
  target_language: 'en',
  current_page: 3,
  file_path: '/docs/book.pdf',
  file_hash: 'abc',
  title: 'book',
  total_pages: 10
};

describe('restoreDecision', () => {
  it('returns none when there is no last session', () => {
    expect(restoreDecision(null, true)).toBe('none');
    expect(restoreDecision(undefined, false)).toBe('none');
  });

  it('restores when the file still exists', () => {
    expect(restoreDecision(session, true)).toBe('restore');
  });

  it('flags missing when the file is gone (EC06)', () => {
    expect(restoreDecision(session, false)).toBe('missing');
  });
});

describe('fileName', () => {
  it('extracts the base name across slash styles', () => {
    expect(fileName('C:\\Users\\me\\report.pdf')).toBe('report.pdf');
    expect(fileName('/home/me/report.pdf')).toBe('report.pdf');
    expect(fileName('report.pdf')).toBe('report.pdf');
  });
});

describe('clampPage', () => {
  it('keeps a valid page unchanged', () => {
    expect(clampPage(3, 10)).toBe(3);
  });

  it('clamps below 1 and above total', () => {
    expect(clampPage(0, 10)).toBe(1);
    expect(clampPage(-5, 10)).toBe(1);
    expect(clampPage(99, 10)).toBe(10);
  });

  it('defaults to page 1 for an empty document', () => {
    expect(clampPage(4, 0)).toBe(1);
  });
});

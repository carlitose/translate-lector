import { describe, it, expect } from 'vitest';
import {
  isMissingKeyError,
  isOfflineError,
  isRateLimitError,
  isOutputBudgetError,
  translationErrorMessage,
  MISSING_KEY_HINT,
  OFFLINE_HINT,
  RATE_LIMIT_HINT,
  OUTPUT_BUDGET_HINT,
  pageStatusLabel,
  resultStatus,
  requestKey,
  isCurrentRequest,
  shouldTranslate,
  isLatestNav
} from './translation';

describe('isMissingKeyError', () => {
  it('detects the EC03 marker in string errors', () => {
    expect(isMissingKeyError('EC03: API key OpenRouter mancante o non valida. …')).toBe(true);
  });

  it('detects EC03 when wrapped in a non-string error', () => {
    expect(isMissingKeyError(new Error('EC03: no key'))).toBe(true);
  });

  it('returns false for unrelated errors', () => {
    expect(isMissingKeyError('Errore di rete/OpenRouter: 500')).toBe(false);
    expect(isMissingKeyError(null)).toBe(false);
  });
});

describe('isOfflineError / isRateLimitError', () => {
  it('detects the EC02 offline marker', () => {
    expect(isOfflineError('EC02: nessuna connessione')).toBe(true);
    expect(isOfflineError('EC03: no key')).toBe(false);
  });

  it('detects the EC07 rate-limit marker', () => {
    expect(isRateLimitError('EC07: rate limit')).toBe(true);
    expect(isRateLimitError('boom')).toBe(false);
  });

  it('detects the EC08 output-budget marker', () => {
    expect(isOutputBudgetError('EC08: il modello locale ha esaurito il budget')).toBe(true);
    expect(isOutputBudgetError('EC02: nessuna connessione')).toBe(false);
  });
});

describe('translationErrorMessage', () => {
  it('maps EC03 to the ⚙️ hint', () => {
    expect(translationErrorMessage('EC03: whatever')).toBe(MISSING_KEY_HINT);
  });

  it('maps EC02 to the offline hint', () => {
    expect(translationErrorMessage('EC02: nessuna connessione')).toBe(OFFLINE_HINT);
  });

  it('maps EC07 to the rate-limit hint', () => {
    expect(translationErrorMessage('EC07: 429')).toBe(RATE_LIMIT_HINT);
  });

  it('maps EC08 to the output-budget hint', () => {
    expect(translationErrorMessage('EC08: budget esaurito')).toBe(OUTPUT_BUDGET_HINT);
  });

  it('passes through other error text unchanged', () => {
    expect(translationErrorMessage('boom')).toBe('boom');
  });
});

describe('pageStatusLabel', () => {
  it('shows the §3.1 cache label', () => {
    expect(pageStatusLabel('cached')).toBe('● Tradotto (cache)');
  });

  it('labels loading, translated and error, and stays empty when idle', () => {
    expect(pageStatusLabel('loading')).toContain('in corso');
    expect(pageStatusLabel('translated')).toBe('● Tradotto');
    expect(pageStatusLabel('error')).toContain('Errore');
    expect(pageStatusLabel('idle')).toBe('');
  });
});

describe('resultStatus', () => {
  it('is cached for a cache hit and translated otherwise', () => {
    expect(resultStatus(true)).toBe('cached');
    expect(resultStatus(false)).toBe('translated');
  });
});

describe('requestKey / isCurrentRequest', () => {
  const req = { documentId: 1, pageNumber: 12, targetLanguage: 'it' };

  it('builds a stable identity from document, page and language', () => {
    expect(requestKey(req)).toBe(requestKey({ ...req }));
  });

  it('treats a result as current only when page and language still match', () => {
    expect(isCurrentRequest(req, { ...req })).toBe(true);
    // Navigated to another page — stale result must be dropped.
    expect(isCurrentRequest(req, { ...req, pageNumber: 13 })).toBe(false);
    // Language changed — also stale.
    expect(isCurrentRequest(req, { ...req, targetLanguage: 'en' })).toBe(false);
    // Different document — stale.
    expect(isCurrentRequest(req, { ...req, documentId: 2 })).toBe(false);
  });
});

describe('shouldTranslate', () => {
  it('translates only when the extracted text belongs to the current page', () => {
    // Page↔text invariant holds: the reconstructed text is page 10's text.
    expect(shouldTranslate(10, 10, 'testo pag. 10')).toBe(true);
  });

  it('does NOT translate while the extracted text is still the previous page', () => {
    // The reactive race: currentPage advanced to 10 but the text is still page 9.
    expect(shouldTranslate(9, 10, 'testo pag. 9')).toBe(false);
  });

  it('does NOT translate when there is no extractable text', () => {
    expect(shouldTranslate(10, 10, '')).toBe(false);
    expect(shouldTranslate(10, 10, '   ')).toBe(false);
  });
});

describe('isLatestNav', () => {
  it('commits when the render is still the latest navigation', () => {
    expect(isLatestNav(3, 3)).toBe(true);
  });

  it('does NOT commit a superseded render (a newer navigation started)', () => {
    // myToken=2 but a newer navigation bumped the counter to 3 meanwhile.
    expect(isLatestNav(2, 3)).toBe(false);
  });
});

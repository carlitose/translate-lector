import { describe, expect, it, vi } from 'vitest';
import { extractPageText, type PdfTextPage } from './pdfPageText';

describe('extractPageText', () => {
  it('uses reader-based extraction for the production page-text path', async () => {
    const releaseLock = vi.fn();
    const read = vi
      .fn()
      .mockResolvedValueOnce({
        done: false,
        value: {
          items: [
            {
              str: 'WKWebView text',
              transform: [1, 0, 0, 10, 10, 100],
              width: 80,
              height: 10,
              hasEOL: true
            }
          ]
        }
      })
      .mockResolvedValueOnce({ done: true });
    const stream = {
      getReader: () => ({ read, releaseLock })
    };
    const getTextContent = vi.fn(async () => {
      throw new Error('Ordinary pages must not call getTextContent()');
    });
    const page: PdfTextPage = {
      isPureXfa: false,
      getTextContent,
      streamTextContent: () => stream,
      getViewport: vi.fn(() => ({ width: 600 }))
    };

    expect((stream as unknown as Record<symbol, unknown>)[Symbol.asyncIterator]).toBeUndefined();
    await expect(extractPageText(page)).resolves.toBe('WKWebView text');
    expect(getTextContent).not.toHaveBeenCalled();
    expect(releaseLock).toHaveBeenCalledOnce();
  });
});

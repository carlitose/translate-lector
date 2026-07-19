import { describe, expect, it, vi } from 'vitest';
import { collectTextContentItems, type TextContentPage } from './pdfTextStream';

function pageWithReads(
  reads: Array<
    | { done: false; value: { items: string[] } }
    | { done: true; value?: undefined }
    | Error
  >
): {
  page: TextContentPage<string>;
  getTextContent: ReturnType<typeof vi.fn>;
  releaseLock: ReturnType<typeof vi.fn>;
} {
  const releaseLock = vi.fn();
  const read = vi.fn(async (): Promise<ReadableStreamReadResult<{ items: string[] }>> => {
    const next = reads.shift();
    if (next instanceof Error) throw next;
    if (!next) throw new Error('Unexpected read');
    return next;
  });
  const getTextContent = vi.fn(async () => ({ items: [] }));

  return {
    page: {
      isPureXfa: false,
      getTextContent,
      streamTextContent: () => ({
        getReader: () => ({ read, releaseLock })
      })
    },
    getTextContent,
    releaseLock
  };
}

describe('collectTextContentItems', () => {
  it('preserves every item in chunk order and releases the reader lock', async () => {
    const { page, getTextContent, releaseLock } = pageWithReads([
      { done: false, value: { items: ['first', 'second'] } },
      { done: false, value: { items: ['third'] } },
      { done: true }
    ]);

    await expect(collectTextContentItems(page)).resolves.toEqual(['first', 'second', 'third']);
    expect(getTextContent).not.toHaveBeenCalled();
    expect(releaseLock).toHaveBeenCalledOnce();
  });

  it('preserves PDF.js pure-XFA extraction without opening a text-content stream', async () => {
    const getTextContent = vi.fn(async () => ({ items: ['xfa text'] }));
    const streamTextContent = vi.fn(() => {
      throw new Error('Pure XFA must not open a text-content stream');
    });
    const page: TextContentPage<string> = {
      isPureXfa: true,
      getTextContent,
      streamTextContent
    };

    await expect(collectTextContentItems(page)).resolves.toEqual(['xfa text']);
    expect(getTextContent).toHaveBeenCalledOnce();
    expect(streamTextContent).not.toHaveBeenCalled();
  });

  it('returns an empty array for an immediately completed stream', async () => {
    const { page, releaseLock } = pageWithReads([{ done: true }]);

    await expect(collectTextContentItems(page)).resolves.toEqual([]);
    expect(releaseLock).toHaveBeenCalledOnce();
  });

  it('releases the reader lock and propagates the original read error', async () => {
    const readError = new Error('stream failed');
    const { page, releaseLock } = pageWithReads([readError]);

    await expect(collectTextContentItems(page)).rejects.toBe(readError);
    expect(releaseLock).toHaveBeenCalledOnce();
  });
});

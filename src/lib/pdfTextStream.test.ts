import { describe, expect, it, vi } from 'vitest';
import { collectTextContentItems, type TextContentPage } from './pdfTextStream';

type TextContentRead<T> =
  | { done: false; value: { items: T[] } }
  | { done: true; value?: undefined };

function pageWithReads<T>(reads: Array<TextContentRead<T> | Error>): {
  page: TextContentPage<T>;
  getTextContent: ReturnType<typeof vi.fn>;
  releaseLock: ReturnType<typeof vi.fn>;
  stream: ReturnType<TextContentPage<T>['streamTextContent']>;
} {
  const releaseLock = vi.fn();
  const read = vi.fn(async (): Promise<ReadableStreamReadResult<{ items: T[] }>> => {
    const next = reads.shift();
    if (next instanceof Error) throw next;
    if (!next) throw new Error('Unexpected read');
    return next;
  });
  const getTextContent = vi.fn(async () => ({ items: [] as T[] }));
  const stream = {
    getReader: () => ({ read, releaseLock })
  };

  return {
    page: {
      isPureXfa: false,
      getTextContent,
      streamTextContent: () => stream
    },
    getTextContent,
    releaseLock,
    stream
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
    const { page, releaseLock } = pageWithReads<string>([{ done: true }]);

    await expect(collectTextContentItems(page)).resolves.toEqual([]);
    expect(releaseLock).toHaveBeenCalledOnce();
  });

  it('releases the reader lock and propagates the original read error', async () => {
    const readError = new Error('stream failed');
    const { page, releaseLock } = pageWithReads<string>([readError]);

    await expect(collectTextContentItems(page)).rejects.toBe(readError);
    expect(releaseLock).toHaveBeenCalledOnce();
  });

  it('works when getReader exists but async iteration is unavailable', async () => {
    const { page, getTextContent, releaseLock, stream } = pageWithReads([
      { done: false, value: { items: ['WKWebView text'] } },
      { done: true }
    ]);

    expect(stream.getReader).toBeTypeOf('function');
    expect((stream as unknown as Record<symbol, unknown>)[Symbol.asyncIterator]).toBeUndefined();
    await expect(collectTextContentItems(page)).resolves.toEqual(['WKWebView text']);
    expect(getTextContent).not.toHaveBeenCalled();
    expect(releaseLock).toHaveBeenCalledOnce();
  });
});

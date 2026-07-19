interface TextContentChunk<T> {
  items: T[];
}

interface TextContentReader<T> {
  read(): Promise<ReadableStreamReadResult<TextContentChunk<T>>>;
  releaseLock(): void;
}

interface TextContentStream<T> {
  getReader(): TextContentReader<T>;
}

/** The minimum PDF.js page contract needed to collect text items. */
export interface TextContentPage<T = unknown> {
  readonly isPureXfa: boolean;
  getTextContent(): Promise<TextContentChunk<T>>;
  streamTextContent(): TextContentStream<T>;
}

/**
 * Collect PDF.js text items without async stream iteration for ordinary PDFs;
 * pure-XFA pages retain PDF.js's non-streaming `getTextContent()` branch.
 */
export async function collectTextContentItems<T>(page: TextContentPage<T>): Promise<T[]> {
  if (page.isPureXfa) {
    const content = await page.getTextContent();
    return content.items;
  }

  const reader = page.streamTextContent().getReader();
  const items: T[] = [];

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) return items;
      items.push(...value.items);
    }
  } finally {
    reader.releaseLock();
  }
}

export interface RegisteredPdfDocument {
  document_id: number;
  title: string;
  total_pages: number;
}

export interface PdfDocumentSession {
  document_id: number;
  target_language: string;
  current_page: number;
}

/** Public PDF.js loading-task lifecycle used by this module. */
export interface PdfDocumentLoadingTask<Document> {
  readonly promise: Promise<Document>;
  destroy(): Promise<void>;
}

export interface PreparedPdfDocument<Document, Session> {
  document: Document;
  session: Session;
  title: string;
  totalPages: number;
  currentPage: number;
  targetLanguage: string;
}

/** Stable identity captured by page navigation and prefetch continuations. */
export interface ActivePdfDocument<Document, Session>
  extends PreparedPdfDocument<Document, Session> {
  readonly generation: number;
}

export type PdfDocumentLoadPhase =
  | 'idle'
  | 'loading'
  | 'activating'
  | 'ready'
  | 'unsupported'
  | 'error';

export interface PdfDocumentLoadState<Document, Session> {
  readonly phase: PdfDocumentLoadPhase;
  readonly generation: number;
  readonly publication: ActivePdfDocument<Document, Session> | null;
  readonly error?: unknown;
}

export type PdfDocumentLoadResult<Document, Session> =
  | { status: 'ready'; document: ActivePdfDocument<Document, Session> }
  | { status: 'unsupported' }
  | { status: 'stale' }
  | { status: 'error'; error: unknown };

interface PdfDocumentLoadDependencies<
  Bytes,
  Document extends { readonly numPages: number },
  Session extends PdfDocumentSession
> {
  readBytes(path: string): Promise<Bytes>;
  openDocument(bytes: Bytes): PdfDocumentLoadingTask<Document>;
  hasExtractableText(document: Document, pageCount: number): Promise<boolean>;
  registerDocument(input: {
    path: string;
    totalPages: number;
    title: string;
  }): Promise<RegisteredPdfDocument>;
  openSession(documentId: number): Promise<Session>;
  titleFromPath(path: string): string;
  clampPage(page: number, totalPages: number): number;
  /** Wait for cancelled rendering to settle before destroying an opened task. */
  beforeDestroy(document: Document): Promise<void>;
  onStateChange(state: PdfDocumentLoadState<Document, Session>): void;
}

export interface PdfDocumentLoadRequest<Document, Session> {
  hasExtractableText?(document: Document, pageCount: number): Promise<boolean>;
  activate?(document: ActivePdfDocument<Document, Session>): Promise<void>;
}

class OwnedLoadingTask<Document> {
  document: Document | null = null;
  private destruction: Promise<void> | null = null;

  constructor(readonly task: PdfDocumentLoadingTask<Document>) {}

  destroy(beforeDestroy: (document: Document) => Promise<void>): Promise<void> {
    if (!this.destruction) {
      this.destruction = (async () => {
        let coordinationError: unknown;
        if (this.document) {
          try {
            await beforeDestroy(this.document);
          } catch (error) {
            coordinationError = error;
          }
        }

        await this.task.destroy();
        if (coordinationError !== undefined) throw coordinationError;
      })();
    }
    return this.destruction;
  }
}

/**
 * Owns PDF loading tasks from creation through replacement/teardown.
 *
 * Each state transition publishes either no document or one complete,
 * generation-bound document/session bundle. Superseded continuations can only
 * destroy the task they created; they cannot clear or dispose a newer active
 * document.
 */
export class PdfDocumentLoadController<
  Bytes,
  Document extends { readonly numPages: number },
  Session extends PdfDocumentSession
> {
  private generation = 0;
  private resources = new Set<OwnedLoadingTask<Document>>();
  private active: {
    identity: ActivePdfDocument<Document, Session>;
    resource: OwnedLoadingTask<Document>;
  } | null = null;

  constructor(
    private readonly dependencies: PdfDocumentLoadDependencies<Bytes, Document, Session>
  ) {}

  captureActive(): ActivePdfDocument<Document, Session> | null {
    return this.active?.identity ?? null;
  }

  isActive(identity: ActivePdfDocument<Document, Session>): boolean {
    return this.active?.identity === identity;
  }

  async load(
    path: string,
    request: PdfDocumentLoadRequest<Document, Session> = {}
  ): Promise<PdfDocumentLoadResult<Document, Session>> {
    const generation = ++this.generation;
    const supersededResources = [...this.resources];
    this.active = null;
    this.transition({ phase: 'loading', generation, publication: null });

    let resource: OwnedLoadingTask<Document> | null = null;

    try {
      await Promise.all(supersededResources.map((owned) => this.destroyResource(owned)));
      if (!this.isCurrent(generation)) return { status: 'stale' };

      const bytes = await this.dependencies.readBytes(path);
      if (!this.isCurrent(generation)) return { status: 'stale' };

      resource = new OwnedLoadingTask(this.dependencies.openDocument(bytes));
      this.resources.add(resource);
      const document = await resource.task.promise;
      resource.document = document;
      if (!this.isCurrent(generation)) return await this.stale(resource);

      const pageCount = document.numPages;
      const hasExtractableText =
        request.hasExtractableText ?? this.dependencies.hasExtractableText;
      const extractable = await hasExtractableText(document, pageCount);
      if (!this.isCurrent(generation)) return await this.stale(resource);
      if (!extractable) {
        await this.destroyResource(resource);
        this.transition({ phase: 'unsupported', generation, publication: null });
        return { status: 'unsupported' };
      }

      const registered = await this.dependencies.registerDocument({
        path,
        totalPages: pageCount,
        title: this.dependencies.titleFromPath(path)
      });
      if (!this.isCurrent(generation)) return await this.stale(resource);

      const session = await this.dependencies.openSession(registered.document_id);
      if (!this.isCurrent(generation)) return await this.stale(resource);

      const identity: ActivePdfDocument<Document, Session> = {
        generation,
        document,
        session,
        title: registered.title,
        totalPages: registered.total_pages,
        currentPage: this.dependencies.clampPage(session.current_page, pageCount),
        targetLanguage: session.target_language
      };
      this.active = { identity, resource };
      this.transition({ phase: 'activating', generation, publication: identity });

      await request.activate?.(identity);
      if (!this.isCurrent(generation) || !this.isActive(identity)) {
        return await this.stale(resource);
      }

      this.transition({ phase: 'ready', generation, publication: identity });
      return { status: 'ready', document: identity };
    } catch (error) {
      if (resource) {
        try {
          await this.destroyResource(resource);
        } catch (cleanupError) {
          error = cleanupError;
        }
      }
      if (!this.isCurrent(generation)) return { status: 'stale' };

      this.active = null;
      this.transition({ phase: 'error', generation, publication: null, error });
      return { status: 'error', error };
    }
  }

  async dispose(): Promise<void> {
    const generation = ++this.generation;
    const resources = [...this.resources];
    this.active = null;
    this.transition({ phase: 'idle', generation, publication: null });
    await Promise.all(resources.map((resource) => this.destroyResource(resource)));
  }

  private isCurrent(generation: number): boolean {
    return generation === this.generation;
  }

  private async stale(
    resource: OwnedLoadingTask<Document>
  ): Promise<{ status: 'stale' }> {
    await this.destroyResource(resource);
    return { status: 'stale' };
  }

  private async destroyResource(resource: OwnedLoadingTask<Document>): Promise<void> {
    try {
      await resource.destroy((document) => this.dependencies.beforeDestroy(document));
    } finally {
      this.resources.delete(resource);
      if (this.active?.resource === resource) this.active = null;
    }
  }

  private transition(state: PdfDocumentLoadState<Document, Session>): void {
    this.dependencies.onStateChange(state);
  }
}

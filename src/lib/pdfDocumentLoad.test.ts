import { describe, expect, it, vi } from 'vitest';
import {
  PdfDocumentLoadController,
  type PdfDocumentLoadingTask,
  type PdfDocumentLoadState
} from './pdfDocumentLoad';

interface TestDocument {
  readonly name: string;
  readonly numPages: number;
}

interface TestSession {
  readonly document_id: number;
  readonly session_id: number;
  readonly target_language: string;
  readonly current_page: number;
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function loadingTask(document: TestDocument) {
  return {
    promise: Promise.resolve(document),
    destroy: vi.fn(async () => undefined)
  };
}

function createHarness(overrides: {
  openDocument?: (bytes: Uint8Array) => PdfDocumentLoadingTask<TestDocument>;
  hasExtractableText?: (document: TestDocument) => Promise<boolean>;
  registerDocument?: (input: {
    path: string;
    totalPages: number;
    title: string;
  }) => Promise<{ document_id: number; title: string; total_pages: number }>;
  openSession?: (documentId: number) => Promise<TestSession>;
} = {}) {
  const states: PdfDocumentLoadState<TestDocument, TestSession>[] = [];
  const beforeDestroy = vi.fn(async () => undefined);
  let nextDocumentId = 1;

  const controller = new PdfDocumentLoadController<Uint8Array, TestDocument, TestSession>({
    readBytes: async () => new Uint8Array([1, 2, 3]),
    openDocument:
      overrides.openDocument ??
      (() => loadingTask({ name: 'default', numPages: 3 })),
    hasExtractableText:
      overrides.hasExtractableText ??
      (async () => true),
    registerDocument:
      overrides.registerDocument ??
      (async ({ title, totalPages }) => ({
        document_id: nextDocumentId++,
        title,
        total_pages: totalPages
      })),
    openSession:
      overrides.openSession ??
      (async (documentId) => ({
        document_id: documentId,
        session_id: documentId * 10,
        target_language: 'it',
        current_page: 1
      })),
    titleFromPath: (path) => path.split('/').pop()?.replace(/\.pdf$/, '') ?? path,
    clampPage: (page, totalPages) => Math.min(Math.max(page, 1), totalPages),
    beforeDestroy,
    onStateChange: (state) => states.push(state)
  });

  return { controller, states, beforeDestroy };
}

describe('PdfDocumentLoadController', () => {
  it('atomically publishes a complete bundle and destroys only the replaced document', async () => {
    const firstTask = loadingTask({ name: 'first', numPages: 3 });
    const secondTask = loadingTask({ name: 'second', numPages: 7 });
    const tasks = [firstTask, secondTask];
    const { controller, states, beforeDestroy } = createHarness({
      openDocument: () => tasks.shift()!
    });

    const first = await controller.load('/books/first.pdf');
    expect(first.status).toBe('ready');
    const firstIdentity = controller.captureActive();
    expect(firstIdentity).not.toBeNull();

    const second = await controller.load('/books/second.pdf');
    expect(second.status).toBe('ready');
    const secondIdentity = controller.captureActive();

    expect(firstTask.destroy).toHaveBeenCalledOnce();
    expect(secondTask.destroy).not.toHaveBeenCalled();
    expect(beforeDestroy).toHaveBeenCalledOnce();
    expect(beforeDestroy).toHaveBeenCalledWith(firstIdentity?.document);
    expect(controller.isActive(firstIdentity!)).toBe(false);
    expect(controller.isActive(secondIdentity!)).toBe(true);

    const activating = states.find(
      (state) =>
        state.phase === 'activating' && state.publication?.document.name === 'second'
    );
    expect(activating?.publication).toMatchObject({
      document: { name: 'second', numPages: 7 },
      session: { document_id: 2, session_id: 20 },
      title: 'second',
      totalPages: 7,
      currentPage: 1,
      targetLanguage: 'it'
    });
    expect(
      states.some(
        (state) => state.phase === 'loading' && state.publication !== null
      )
    ).toBe(false);

    await controller.dispose();
    await controller.dispose();
    expect(firstTask.destroy).toHaveBeenCalledOnce();
    expect(secondTask.destroy).toHaveBeenCalledOnce();
  });

  it('destroys a stale opened task exactly once without overwriting the newer final state', async () => {
    const firstTask = loadingTask({ name: 'first', numPages: 4 });
    const secondTask = loadingTask({ name: 'second', numPages: 5 });
    const firstRegistration = deferred<{
      document_id: number;
      title: string;
      total_pages: number;
    }>();
    let registrationCount = 0;
    const { controller, states } = createHarness({
      openDocument: (() => {
        const tasks = [firstTask, secondTask];
        return () => tasks.shift()!;
      })(),
      registerDocument: async ({ title, totalPages }) => {
        registrationCount++;
        if (registrationCount === 1) return firstRegistration.promise;
        return { document_id: 2, title, total_pages: totalPages };
      }
    });

    const firstLoad = controller.load('/books/first.pdf');
    await vi.waitFor(() => expect(registrationCount).toBe(1));

    const secondLoad = controller.load('/books/second.pdf');
    await expect(secondLoad).resolves.toMatchObject({ status: 'ready' });
    const stateAfterSecond = states.at(-1);

    firstRegistration.resolve({
      document_id: 1,
      title: 'first',
      total_pages: 4
    });
    await expect(firstLoad).resolves.toEqual({ status: 'stale' });

    expect(firstTask.destroy).toHaveBeenCalledOnce();
    expect(secondTask.destroy).not.toHaveBeenCalled();
    expect(states.at(-1)).toBe(stateAfterSecond);
    expect(states.at(-1)).toMatchObject({
      phase: 'ready',
      publication: { document: { name: 'second' } }
    });
  });

  it('destroys an unsupported no-text document exactly once', async () => {
    const task = loadingTask({ name: 'scan', numPages: 2 });
    const { controller, states } = createHarness({
      openDocument: () => task,
      hasExtractableText: async () => false
    });

    await expect(controller.load('/books/scan.pdf')).resolves.toEqual({
      status: 'unsupported'
    });

    expect(task.destroy).toHaveBeenCalledOnce();
    expect(states.at(-1)).toMatchObject({
      phase: 'unsupported',
      publication: null
    });
  });

  it('destroys exactly once when an opened task or a later preparation step rejects', async () => {
    const openFailure = deferred<TestDocument>();
    const rejectedTask = {
      promise: openFailure.promise,
      destroy: vi.fn(async () => undefined)
    };
    const preparationTask = loadingTask({ name: 'later-failure', numPages: 6 });
    const tasks = [rejectedTask, preparationTask];
    const { controller, states } = createHarness({
      openDocument: () => tasks.shift()!,
      registerDocument: async () => {
        throw new Error('database unavailable');
      }
    });

    const rejectedLoad = controller.load('/books/rejected.pdf');
    openFailure.reject(new Error('invalid PDF'));
    await expect(rejectedLoad).resolves.toMatchObject({
      status: 'error',
      error: expect.objectContaining({ message: 'invalid PDF' })
    });
    expect(rejectedTask.destroy).toHaveBeenCalledOnce();

    await expect(controller.load('/books/later.pdf')).resolves.toMatchObject({
      status: 'error',
      error: expect.objectContaining({ message: 'database unavailable' })
    });
    expect(preparationTask.destroy).toHaveBeenCalledOnce();
    expect(states.at(-1)).toMatchObject({ phase: 'error', publication: null });
  });

  it('awaits teardown of the active resource and makes repeated teardown idempotent', async () => {
    const destroyGate = deferred<void>();
    const task = {
      promise: Promise.resolve({ name: 'active', numPages: 2 }),
      destroy: vi.fn(() => destroyGate.promise)
    };
    const { controller, states } = createHarness({ openDocument: () => task });
    await controller.load('/books/active.pdf');

    let teardownFinished = false;
    const teardown = controller.dispose().then(() => {
      teardownFinished = true;
    });

    await vi.waitFor(() => expect(task.destroy).toHaveBeenCalledOnce());
    expect(teardownFinished).toBe(false);
    destroyGate.resolve();
    await teardown;
    await controller.dispose();

    expect(task.destroy).toHaveBeenCalledOnce();
    expect(states.at(-1)).toMatchObject({ phase: 'idle', publication: null });
  });
});

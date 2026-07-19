/** The subset of a PDF.js RenderTask needed to coordinate canvas rendering. */
export interface CancellableRenderTask {
  promise: Promise<unknown>;
  cancel(): void;
}

interface ActiveRender {
  task: CancellableRenderTask;
  cancellationRequested: boolean;
  settled: Promise<void>;
}

/**
 * Coordinates renders that share a canvas.
 *
 * A new navigation cancels the active PDF.js task. Its replacement waits for
 * that task to settle before it is allowed to resize or render into the canvas.
 * Requests superseded while fetching their page never start rendering at all.
 */
export class LatestRenderCoordinator {
  private latestRequest = 0;
  private latestDocument = 0;
  private active: ActiveRender | null = null;

  /** Invalidates all work for the prior document and returns the new identity. */
  beginDocument(): number {
    const document = ++this.latestDocument;
    this.beginNavigation();
    return document;
  }

  isCurrentDocument(document: number): boolean {
    return document === this.latestDocument;
  }

  beginNavigation(): number {
    const request = ++this.latestRequest;
    if (this.active && !this.active.cancellationRequested) {
      this.active.cancellationRequested = true;
      this.active.task.cancel();
    }
    return request;
  }

  isLatest(request: number): boolean {
    return request === this.latestRequest;
  }

  /** Waits until the task cancelled by `beginDocument` has released the canvas. */
  async waitForIdle(): Promise<void> {
    const active = this.active;
    if (active) await active.settled;
  }

  /**
   * Starts the task only when `request` is still current and no earlier task is
   * using the canvas. Returns true only when the latest render completed.
   */
  async render(
    request: number,
    start: () => CancellableRenderTask
  ): Promise<boolean> {
    if (!this.isLatest(request)) return false;

    const previous = this.active;
    if (previous) await previous.settled;
    if (!this.isLatest(request)) return false;

    const task = start();
    const active: ActiveRender = {
      task,
      cancellationRequested: false,
      // Always observe task rejection here as well as below, so a replacement
      // can safely wait for cleanup without inheriting the previous error.
      settled: task.promise.then(
        () => undefined,
        () => undefined
      )
    };
    this.active = active;

    try {
      await task.promise;
      return this.isLatest(request);
    } catch (error) {
      if (active.cancellationRequested || isRenderingCancelled(error)) return false;
      throw error;
    } finally {
      if (this.active === active) this.active = null;
    }
  }
}

/** PDF.js rejects a cancelled RenderTask with RenderingCancelledException. */
function isRenderingCancelled(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'name' in error &&
    error.name === 'RenderingCancelledException'
  );
}

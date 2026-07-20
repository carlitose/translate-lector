import { describe, expect, it } from 'vitest';
import { LatestRenderCoordinator, type CancellableRenderTask } from './pdfRender';

function deferredTask(onCancel: () => void): {
  task: CancellableRenderTask;
  resolve: () => void;
} {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return {
    task: {
      promise,
      cancel() {
        onCancel();
        const error = new Error('Rendering cancelled');
        error.name = 'RenderingCancelledException';
        reject(error);
      }
    },
    resolve
  };
}

describe('LatestRenderCoordinator', () => {
  it('cancels an overlapping render and starts only the latest task after it settles', async () => {
    const coordinator = new LatestRenderCoordinator();
    const started: number[] = [];
    let cancellations = 0;

    const first = deferredTask(() => cancellations++);
    const firstRequest = coordinator.beginNavigation();
    const firstResult = coordinator.render(firstRequest, () => {
      started.push(1);
      return first.task;
    });

    const second = deferredTask(() => cancellations++);
    const secondRequest = coordinator.beginNavigation();
    const secondResult = coordinator.render(secondRequest, () => {
      started.push(2);
      return second.task;
    });

    expect(cancellations).toBe(1);
    expect(started).toEqual([1]);
    await expect(firstResult).resolves.toBe(false);
    await Promise.resolve();
    expect(started).toEqual([1, 2]);

    second.resolve();
    await expect(secondResult).resolves.toBe(true);
  });

  it('does not start a request superseded while its page was loading', async () => {
    const coordinator = new LatestRenderCoordinator();
    const staleRequest = coordinator.beginNavigation();
    coordinator.beginNavigation();

    let started = false;
    await expect(
      coordinator.render(staleRequest, () => {
        started = true;
        return deferredTask(() => undefined).task;
      })
    ).resolves.toBe(false);
    expect(started).toBe(false);
  });

  it('invalidates and cancels deferred work when a new document load begins', async () => {
    const coordinator = new LatestRenderCoordinator();
    let cancellations = 0;

    const firstDocument = coordinator.beginDocument();
    const firstTask = deferredTask(() => cancellations++);
    const firstRender = coordinator.render(coordinator.beginNavigation(), () => firstTask.task);

    const secondDocument = coordinator.beginDocument();

    expect(cancellations).toBe(1);
    expect(coordinator.isCurrentDocument(firstDocument)).toBe(false);
    expect(coordinator.isCurrentDocument(secondDocument)).toBe(true);
    await expect(firstRender).resolves.toBe(false);
  });

  it('waits for a cancelled render to settle before document disposal continues', async () => {
    const coordinator = new LatestRenderCoordinator();
    let rejectRender!: (reason: unknown) => void;
    const task: CancellableRenderTask = {
      promise: new Promise((_, reject) => {
        rejectRender = reject;
      }),
      cancel() {
        queueMicrotask(() => {
          const error = new Error('Rendering cancelled');
          error.name = 'RenderingCancelledException';
          rejectRender(error);
        });
      }
    };
    const rendering = coordinator.render(coordinator.beginNavigation(), () => task);

    coordinator.beginDocument();
    let settled = false;
    const idle = coordinator.waitForIdle().then(() => {
      settled = true;
    });
    expect(settled).toBe(false);

    await idle;
    await expect(rendering).resolves.toBe(false);
    expect(settled).toBe(true);
  });
});

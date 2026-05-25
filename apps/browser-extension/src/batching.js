export function createEventBatcher({ delayMs = 750, maxSize = 10, send }) {
  let queue = [];
  let timer = null;
  let pendingResolvers = [];

  function resolvePending(value) {
    const resolvers = pendingResolvers;
    pendingResolvers = [];
    resolvers.forEach((resolve) => resolve(value));
  }

  async function flush() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (queue.length === 0) {
      const result = { ok: true, skipped: true, count: 0 };
      resolvePending(result);
      return result;
    }

    const events = queue;
    queue = [];
    const result = await send({
      type: "worktrace.browser_tab_batch",
      schemaVersion: 1,
      events,
    });
    resolvePending(result);
    return result;
  }

  function enqueue(event) {
    queue.push(event);
    const promise = new Promise((resolve) => {
      pendingResolvers.push(resolve);
    });

    if (queue.length >= maxSize) {
      void flush();
    } else if (!timer) {
      timer = setTimeout(() => {
        void flush();
      }, delayMs);
    }

    return promise;
  }

  return {
    enqueue,
    flush,
    size: () => queue.length,
  };
}

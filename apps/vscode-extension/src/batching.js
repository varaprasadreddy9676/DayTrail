"use strict";

const { toBridgeMessage } = require("./bridge");

function createEventBatcher(options = {}) {
  const delayMs = clampInteger(options.delayMs ?? 750, 0, 60_000);
  const maxBatchSize = clampInteger(options.maxBatchSize ?? options.maxSize ?? 10, 1, 50);
  const maxQueueSize = clampInteger(options.maxQueueSize ?? 100, 1, 500);
  const source = options.source ?? "vscode-extension";
  const now = typeof options.now === "function" ? options.now : () => new Date().toISOString();
  const send = options.send;

  if (typeof send !== "function") {
    throw new TypeError("createEventBatcher requires a send function");
  }

  let queue = [];
  let timer = null;
  let inFlight = null;

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function scheduleFlush() {
    if (!timer && queue.length > 0) {
      timer = setTimeout(() => {
        void flush();
      }, delayMs);
    }
  }

  function enqueue(event) {
    const promise = new Promise((resolve) => {
      queue.push({ event, resolve });
    });

    while (queue.length > maxQueueSize) {
      const dropped = queue.shift();
      dropped?.resolve({ ok: false, dropped: true, reason: "queue_limit" });
    }

    if (queue.length >= maxBatchSize) {
      void flush();
    } else {
      scheduleFlush();
    }

    return promise;
  }

  async function flush() {
    clearTimer();
    if (inFlight) {
      await inFlight;
    }
    if (queue.length === 0) {
      return { ok: true, skipped: true, count: 0 };
    }

    const batch = queue.splice(0, maxBatchSize);
    const payload = toBridgeMessage(
      batch.map((entry) => entry.event),
      {
        source,
        capturedAt: now(),
      },
    );

    inFlight = Promise.resolve()
      .then(() => send(payload))
      .then((result) => result ?? { ok: true })
      .catch((error) => ({ ok: false, error: error?.message ?? String(error) }))
      .then((result) => {
        batch.forEach((entry) => entry.resolve(result));
        return result;
      })
      .finally(() => {
        inFlight = null;
      });

    const result = await inFlight;
    if (queue.length >= maxBatchSize) {
      void flush();
    } else {
      scheduleFlush();
    }
    return result;
  }

  return {
    enqueue,
    flush,
    size: () => queue.length,
  };
}

function clampInteger(value, min, max) {
  const number = Number.isFinite(value) ? Math.trunc(value) : min;
  return Math.min(max, Math.max(min, number));
}

module.exports = {
  createEventBatcher,
};

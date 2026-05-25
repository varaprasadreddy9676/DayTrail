import assert from 'node:assert/strict';
import test from 'node:test';

import { createEventBatcher } from '../apps/browser-extension/src/batching.js';

test('browser extension batcher sends bounded native batches', async () => {
  const sent = [];
  const batcher = createEventBatcher({
    delayMs: 10_000,
    maxSize: 2,
    send: async (payload) => {
      sent.push(payload);
      return { ok: true, stored: payload.events.length };
    },
  });

  const first = batcher.enqueue({ url: 'https://example.com/a' });
  const second = batcher.enqueue({ url: 'https://example.com/b' });
  const result = await second;

  assert.equal((await first).ok, true);
  assert.equal(result.stored, 2);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].type, 'worktrace.browser_tab_batch');
  assert.equal(sent[0].schemaVersion, 1);
  assert.deepEqual(sent[0].events.map((event) => event.url), [
    'https://example.com/a',
    'https://example.com/b',
  ]);
});

test('browser extension batcher supports explicit flush', async () => {
  const sent = [];
  const batcher = createEventBatcher({
    delayMs: 10_000,
    send: async (payload) => {
      sent.push(payload);
      return { ok: true };
    },
  });

  batcher.enqueue({ url: 'https://example.com/only' });
  await batcher.flush();

  assert.equal(sent.length, 1);
  assert.equal(sent[0].events.length, 1);
  assert.equal(batcher.size(), 0);
});

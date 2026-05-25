import assert from 'node:assert/strict';
import test from 'node:test';

import contextHelpers from '../apps/vscode-extension/src/context.js';
import bridgeHelpers from '../apps/vscode-extension/src/bridge.js';
import batchingHelpers from '../apps/vscode-extension/src/batching.js';

const {
  collectActiveEditorContext,
  normalizeEditorContext,
  redactEditorString,
} = contextHelpers;
const { stableStringify, toBridgeMessage } = bridgeHelpers;
const { createEventBatcher } = batchingHelpers;

test('VS Code editor context helper captures metadata and avoids file contents by default', () => {
  const captured = collectActiveEditorContext({
    now: () => '2026-05-23T08:00:00.000Z',
    appName: 'Cursor',
    workspaceName: 'Payments',
    workspaceFolders: [
      {
        name: 'Payments',
        uri: { fsPath: '/Users/alice/work/payments-api' },
      },
    ],
    activeTextEditor: {
      document: {
        uri: {
          scheme: 'file',
          fsPath: '/Users/alice/work/payments-api/.env',
        },
        fileName: '/Users/alice/work/payments-api/.env',
        languageId: 'dotenv',
        isUntitled: false,
        lineCount: 11,
        getText() {
          throw new Error('file contents must not be read by default');
        },
      },
      selection: {
        active: { line: 7, character: 14 },
        anchor: { line: 7, character: 3 },
      },
    },
  });

  assert.equal(captured.type, 'worktrace.editor_context');
  assert.equal(captured.schemaVersion, 1);
  assert.equal(captured.source, 'vscode-extension');
  assert.equal(captured.app, 'Cursor');
  assert.equal(captured.eventType, 'active_editor_changed');
  assert.equal(captured.capturedAt, '2026-05-23T08:00:00.000Z');
  assert.equal(captured.document.contentCaptured, false);
  assert.equal(captured.document.contentHash, null);
  assert.equal(captured.document.languageId, 'dotenv');
  assert.equal(captured.document.cursor.line, 7);
  assert.equal(captured.document.selection.start.line, 7);
  assert.equal(captured.document.selection.end.character, 14);
  assert.equal(captured.sensitivity, 'sensitive');

  const serialized = JSON.stringify(captured);
  assert.equal(serialized.includes('file contents'), false);
  assert.equal(serialized.includes('.env'), false);
  assert.equal(serialized.includes('payments-api'), true);
});

test('VS Code editor normalization redacts query parameters and secret-like strings', () => {
  const normalized = normalizeEditorContext({
    source: 'unit-test',
    capturedAt: '2026-05-23T08:00:00.000Z',
    app: 'Visual Studio Code',
    document: {
      uri: 'vscode-remote://ssh-remote+prod/home/me/app/config.js?token=super-secret#frag',
      filePath: '/home/me/app/config.js',
      fileName: 'config.js',
      languageId: 'javascript',
      lineCount: 23,
      cursor: { line: 2, character: 4 },
      selection: {
        start: { line: 2, character: 4 },
        end: { line: 2, character: 9 },
      },
    },
    workspace: {
      name: 'API',
      folders: ['/home/me/app'],
    },
    metadata: {
      title: 'Bearer secret-token-1234567890abcdef1234567890abcdef',
      note: 'plain context',
    },
  });

  assert.equal(normalized.document.uri, 'vscode-remote://ssh-remote+prod/home/me/app/config.js');
  assert.equal(normalized.metadata.title, 'Bearer [redacted-secret]');
  assert.equal(normalized.metadata.note, 'plain context');
  assert.equal(JSON.stringify(normalized).includes('super-secret'), false);
  assert.equal(JSON.stringify(normalized).includes('token='), false);
});

test('VS Code bridge message uses deterministic JSON without secret fields', () => {
  const event = normalizeEditorContext({
    source: 'vscode-extension',
    capturedAt: '2026-05-23T08:00:00.000Z',
    app: 'Visual Studio Code',
    document: {
      uri: 'file:///Users/alice/work/app/src/index.ts?password=hunter2',
      filePath: '/Users/alice/work/app/src/index.ts',
      fileName: 'index.ts',
      languageId: 'typescript',
      cursor: { line: 12, character: 2 },
    },
    workspace: {
      name: 'app',
      folders: ['/Users/alice/work/app'],
    },
  });
  const message = toBridgeMessage([event], {
    source: 'vscode-extension',
    capturedAt: '2026-05-23T08:00:01.000Z',
  });

  assert.deepEqual(Object.keys(message), [
    'type',
    'schemaVersion',
    'source',
    'capturedAt',
    'events',
  ]);
  assert.equal(message.type, 'worktrace.editor_context_batch');
  assert.equal(message.schemaVersion, 1);
  assert.equal(message.events.length, 1);

  const first = stableStringify(message);
  const second = stableStringify(message);
  assert.equal(first, second);
  assert.equal(first.includes('hunter2'), false);
  assert.equal(first.includes('password='), false);
});

test('VS Code event batcher sends bounded native bridge batches', async () => {
  const sent = [];
  const batcher = createEventBatcher({
    delayMs: 10_000,
    maxBatchSize: 2,
    maxQueueSize: 3,
    source: 'vscode-extension',
    now: () => '2026-05-23T08:00:01.000Z',
    send: async (payload) => {
      sent.push(payload);
      return { ok: true, stored: payload.events.length };
    },
  });

  const eventA = normalizeEditorContext({
    capturedAt: '2026-05-23T08:00:00.000Z',
    document: { fileName: 'a.ts' },
  });
  const eventB = normalizeEditorContext({
    capturedAt: '2026-05-23T08:00:00.100Z',
    document: { fileName: 'b.ts' },
  });

  const first = batcher.enqueue(eventA);
  const second = batcher.enqueue(eventB);
  const result = await second;

  assert.equal((await first).ok, true);
  assert.equal(result.stored, 2);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].type, 'worktrace.editor_context_batch');
  assert.equal(sent[0].schemaVersion, 1);
  assert.deepEqual(
    sent[0].events.map((event) => event.document.fileName),
    ['a.ts', 'b.ts'],
  );
  assert.equal(batcher.size(), 0);
});

test('VS Code event batcher drops oldest events when queue is full', async () => {
  const batcher = createEventBatcher({
    delayMs: 10_000,
    maxBatchSize: 10,
    maxQueueSize: 2,
    send: async (payload) => ({ ok: true, stored: payload.events.length }),
  });

  const first = batcher.enqueue(normalizeEditorContext({ document: { fileName: 'old.ts' } }));
  batcher.enqueue(normalizeEditorContext({ document: { fileName: 'middle.ts' } }));
  batcher.enqueue(normalizeEditorContext({ document: { fileName: 'new.ts' } }));

  assert.deepEqual(await first, {
    ok: false,
    dropped: true,
    reason: 'queue_limit',
  });
  assert.equal(batcher.size(), 2);
  await batcher.flush();
  assert.equal(batcher.size(), 0);
});

test('VS Code redaction helper handles common credential patterns', () => {
  const redacted = redactEditorString(
    'password=hunter2 token=abc123 Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature secret-token-1234567890abcdef1234567890abcdef',
  );

  assert.equal(redacted.includes('hunter2'), false);
  assert.equal(redacted.includes('abc123'), false);
  assert.equal(redacted.includes('eyJhbGci'), false);
  assert.equal(redacted.includes('secret-token'), false);
});

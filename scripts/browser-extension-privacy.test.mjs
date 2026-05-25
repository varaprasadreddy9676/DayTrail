import assert from 'node:assert/strict';
import test from 'node:test';

import {
  domainFromUrl,
  redactUrl,
  toBridgePayload,
} from '../apps/browser-extension/src/privacy.js';

test('browser extension privacy helpers redact URL query and fragment before payload storage', () => {
  const redacted = redactUrl('https://chatgpt.com/c/abc?token=secret#frag');
  assert.equal(redacted, 'https://chatgpt.com/c/abc');
  assert.equal(domainFromUrl(redacted), 'chatgpt.com');

  const payload = toBridgePayload(
    {
      id: 42,
      windowId: 7,
      title: 'ChatGPT thread',
      url: 'https://chatgpt.com/c/abc?token=secret#frag',
      incognito: false,
    },
    'unit-test',
  );

  assert.equal(payload.url, 'https://chatgpt.com/c/abc');
  assert.equal(payload.domain, 'chatgpt.com');
  assert.equal(JSON.stringify(payload).includes('token=secret'), false);
});

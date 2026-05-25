import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const script = fileURLToPath(new URL('./write-native-host-manifest.mjs', import.meta.url));

function runManifest(browser, extensionId = 'abcdefghijklmnopabcdefghijklmnop') {
  const dir = mkdtempSync(join(tmpdir(), 'worktrace-host-'));
  const outFile = join(dir, `${browser}.json`);
  const result = spawnSync(process.execPath, [
    script,
    browser,
    '/tmp/worktrace-native-host',
    extensionId,
    outFile,
  ]);
  return { result, outFile };
}

test('native host manifest writer supports Chrome and Edge origins', () => {
  for (const browser of ['chrome', 'edge']) {
    const { result, outFile } = runManifest(browser);
    assert.equal(result.status, 0, result.stderr.toString());

    const manifest = JSON.parse(readFileSync(outFile, 'utf8'));
    assert.equal(manifest.name, 'ai.daytrail.desktop');
    assert.equal(manifest.type, 'stdio');
    assert.deepEqual(manifest.allowed_origins, [
      'chrome-extension://abcdefghijklmnopabcdefghijklmnop/',
    ]);
  }
});

test('native host manifest writer rejects placeholder extension ids', () => {
  const { result } = runManifest('chrome', '__EXTENSION_ID__');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr.toString(), /extension id/i);
});

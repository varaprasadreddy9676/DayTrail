import { readFileSync } from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';

test('DMG packaging script uses hdiutil and avoids Finder AppleScript', () => {
  const script = readFileSync(new URL('./package-macos-dmg.sh', import.meta.url), 'utf8');

  assert.match(script, /hdiutil\s+create/);
  assert.doesNotMatch(script, /osascript/i);
  assert.doesNotMatch(script, /Finder/i);
});

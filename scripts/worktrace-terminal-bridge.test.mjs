import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

test('terminal bridge writes redacted command metadata', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'worktrace-terminal-'));
  const outFile = join(dir, 'terminal-bridge.json');

  await execFileAsync('./scripts/worktrace-terminal-bridge.sh', [], {
    env: {
      ...process.env,
      WORKTRACE_TERMINAL_BRIDGE: outFile,
      WORKTRACE_TERMINAL_EVENT: 'command',
      WORKTRACE_TERMINAL_COMMAND: 'curl https://api.example.test --api-key secret-token',
    },
  });

  const metadata = JSON.parse(await readFile(outFile, 'utf8'));
  assert.equal(metadata.eventType, 'command');
  assert.equal(metadata.lastCommand, 'curl https://api.example.test --api-key [redacted]');
  assert.equal(metadata.cwd, process.cwd());
});

test('terminal bridge prints installable shell hooks with absolute script path', async () => {
  const { stdout: zshHook } = await execFileAsync('./scripts/worktrace-terminal-bridge.sh', [
    '--print-zsh-hook',
  ]);
  const { stdout: bashHook } = await execFileAsync('./scripts/worktrace-terminal-bridge.sh', [
    '--print-bash-hook',
  ]);

  assert.match(zshHook, /add-zsh-hook precmd worktrace_terminal_bridge_precmd/);
  assert.match(zshHook, /WORKTRACE_TERMINAL_EVENT=command/);
  assert.match(zshHook, /\/scripts\/worktrace-terminal-bridge\.sh/);
  assert.match(bashHook, /trap worktrace_terminal_bridge_debug DEBUG/);
  assert.match(bashHook, /PROMPT_COMMAND=/);
  assert.match(bashHook, /\/scripts\/worktrace-terminal-bridge\.sh/);
});

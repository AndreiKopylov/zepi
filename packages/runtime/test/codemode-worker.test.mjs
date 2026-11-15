// Reproduces the compiled-agent codemode sandbox failure: the QuickJS worker must be
// resolvable inside the bun binary ("Cannot find module './src/.../worker.ts'").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';

const exe = process.env.ZEPI_AGENT_EXE || 'D:/AgentSandbox/zepi-build/release-fast/agent.exe';

test('compiled agent codemode sandbox loads its worker and runs a script', { timeout: 60_000, skip: !existsSync(exe) }, () => {
  const r = spawnSync(exe, ['codemode-selftest'], { encoding: 'utf8', timeout: 45_000 });
  const line = (r.stdout || '').trim().split('\n').filter(Boolean).pop();
  assert.ok(line, `no selftest output (stderr: ${(r.stderr || '').slice(0, 300)})`);
  const out = JSON.parse(line);
  assert.equal(out.ok, true, `codemode sandbox failed: ${out.error}`);
});
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { resolveBrowserExecutable } from '../../src/browser/config.mjs';

const exeAt = async (root, rel) => {
  const path = join(root, rel);
  await mkdir(join(path, '..'), { recursive: true });
  await writeFile(path, 'x');
  return path;
};

test('browser ladder: setting wins, then system Chrome, then Edge, then none', async (t) => {
  const local = await mkdtemp(join(tmpdir(), 'bl-'));
  const pf = await mkdtemp(join(tmpdir(), 'bp-'));
  const empty = await mkdtemp(join(tmpdir(), 'be-'));
  t.after(() => Promise.all([local, pf, empty].map((d) => rm(d, { recursive: true, force: true }))));
  const env = { LOCALAPPDATA: local, ProgramFiles: pf, 'ProgramFiles(x86)': empty };

  const setting = join(empty, 'my-chrome.exe');
  await writeFile(setting, 'x');
  assert.deepEqual(resolveBrowserExecutable(setting, env), { path: setting, source: 'setting' });
  assert.equal(resolveBrowserExecutable(join(empty, 'ghost.exe'), env).source, 'missing');

  const edge = await exeAt(pf, 'Microsoft/Edge/Application/msedge.exe');
  assert.deepEqual(resolveBrowserExecutable('', env), { path: edge, source: 'system' });

  const chrome = await exeAt(pf, 'Google/Chrome/Application/chrome.exe');
  assert.deepEqual(resolveBrowserExecutable('', env), { path: chrome, source: 'system' });

  assert.deepEqual(resolveBrowserExecutable('', { LOCALAPPDATA: empty, ProgramFiles: empty, 'ProgramFiles(x86)': empty }), { path: undefined, source: 'none' });
});

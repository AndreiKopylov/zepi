import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { resolveBrowserExecutable } from '../../src/browser/config.mjs';

const chromeAt = async (root, rel) => {
  const path = join(root, rel);
  await mkdir(join(root, rel, '..'), { recursive: true }).catch(() => {});
  await mkdir(path.replace(/[/\\][^/\\]+$/, ''), { recursive: true });
  await writeFile(path, 'x');
  return path;
};

test('browser ladder: setting wins, then zepi dir, playwright cache, system, none', async () => {
  const zepi = await mkdtemp(join(tmpdir(), 'bz-'));
  const local = await mkdtemp(join(tmpdir(), 'bl-'));
  const pf = await mkdtemp(join(tmpdir(), 'bp-'));
  const empty = await mkdtemp(join(tmpdir(), 'be-'));
  const bundled = await chromeAt(zepi, 'chromium-1243/chrome-win/chrome.exe');
  const cached = await chromeAt(local, 'ms-playwright/chromium-9/chrome-win/chrome.exe');
  const edge = await chromeAt(pf, 'Microsoft/Edge/Application/msedge.exe');
  const setting = join(empty, 'my-chrome.exe');
  await writeFile(setting, 'x');
  const env = { PLAYWRIGHT_BROWSERS_PATH: zepi, LOCALAPPDATA: local, ProgramFiles: pf, 'ProgramFiles(x86)': empty };
  assert.deepEqual(resolveBrowserExecutable('', env), { path: bundled, source: 'zepi' });
  assert.deepEqual(resolveBrowserExecutable(setting, env), { path: setting, source: 'setting' });
  assert.equal(resolveBrowserExecutable(join(empty, 'ghost.exe'), env).source, 'missing');
  assert.deepEqual(resolveBrowserExecutable('', { ...env, PLAYWRIGHT_BROWSERS_PATH: empty }), { path: cached, source: 'playwright' });
  assert.deepEqual(resolveBrowserExecutable('', { PLAYWRIGHT_BROWSERS_PATH: empty, LOCALAPPDATA: empty, ProgramFiles: pf, 'ProgramFiles(x86)': empty }), { path: edge, source: 'system' });
  const chrome = await chromeAt(pf, 'Google/Chrome/Application/chrome.exe');
  assert.deepEqual(resolveBrowserExecutable('', { PLAYWRIGHT_BROWSERS_PATH: empty, LOCALAPPDATA: empty, ProgramFiles: pf, 'ProgramFiles(x86)': empty }), { path: chrome, source: 'system' });
  assert.deepEqual(resolveBrowserExecutable('', { PLAYWRIGHT_BROWSERS_PATH: empty, LOCALAPPDATA: empty, ProgramFiles: empty, 'ProgramFiles(x86)': empty }), { path: undefined, source: 'none' });
});

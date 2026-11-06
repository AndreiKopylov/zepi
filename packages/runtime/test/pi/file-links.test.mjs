import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtemp, writeFile, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createFileLinker, linkifyText } from '../../src/pi/file-links.mjs';

test('code spans naming real files become links, identifiers stay code', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'file-links-'));
  await mkdir(join(dir, 'src'), { recursive: true });
  await writeFile(join(dir, 'src', 'foo.ts'), 'x');
  await writeFile(join(dir, 'notes.md'), 'x');
  const text = 'Edited `src/foo.ts` and `notes.md`; symbol `PollingInterval` and value `425` stay code.';
  const out = linkifyText(text, dir);
  assert.match(out, /\[src\/foo\.ts\]\(src\/foo\.ts\)/);
  assert.match(out, /\[notes\.md\]\(notes\.md\)/);
  assert.match(out, /`PollingInterval`/);
  assert.match(out, /`425`/);
});

test('fenced blocks and already linked spans are untouched', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'file-links-'));
  await writeFile(join(dir, 'a.txt'), 'x');
  const text = 'See [a.txt](a.txt) and:\n\n```ts\n// `a.txt` inside fence\n```\nDone `a.txt`.';
  const out = linkifyText(text, dir);
  assert.match(out, /\[a\.txt\]\(a\.txt\) and:/);
  assert.match(out, /`a\.txt` inside fence/);
  assert.equal(out.split('[a.txt](a.txt)').length - 1, 2);
});

test('streaming holds a span split across chunks and flushes the rest', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'file-links-'));
  await writeFile(join(dir, 'b.txt'), 'x');
  const linker = createFileLinker(dir);
  const a = linker.push('see `b.');
  const b = linker.push('txt` now');
  assert.equal(a, 'see ');
  assert.equal(b + linker.flush(), '[b.txt](b.txt) now');
});

test('unfinished span at turn end flushes as literal code', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'file-links-'));
  const linker = createFileLinker(dir);
  const a = linker.push('tail `b.txt');
  assert.equal(a, 'tail ');
  assert.equal(linker.flush(), '`b.txt');
});

test('missing files and urls stay code', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'file-links-'));
  const out = linkifyText('No `src/ghost.ts` and `https://example.com/a.ts` here.', dir);
  assert.match(out, /`src\/ghost\.ts`/);
  assert.match(out, /`https:\/\/example\.com\/a\.ts`/);
});

test('subfolder-relative spans link through a unique project index suffix', () => {
  const dir = tmpdir();
  const index = () => ['integration-sign/src/Application/Signing/Client/ClientSigningResultFactory.cs'];
  const out = linkifyText('See `Application/Signing/Client/ClientSigningResultFactory.cs` now.', dir, index);
  assert.match(out, /\[Application\/Signing\/Client\/ClientSigningResultFactory\.cs\]\(integration-sign\/src\/Application\/Signing\/Client\/ClientSigningResultFactory\.cs\)/);
  const amb = linkifyText('`Application/Dup.cs`', dir, () => ['a/Application/Dup.cs', 'b/Application/Dup.cs']);
  assert.match(amb, /`Application\/Dup\.cs`/);
});

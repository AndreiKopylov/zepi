import assert from 'node:assert/strict';
import {test} from 'node:test';
import {mkdtemp, mkdir, readdir, realpath, readFile, utimes, writeFile, rm, access} from 'node:fs/promises';
import {readdirSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {client, methods} from '@agentclientprotocol/sdk';
import {startZepiAgent} from '../src/server.ts';
import {EDITOR_QUERY} from '../src/editor-query.ts';
import {sweepStaleRuns} from '../src/capture.ts';
import {sweepDeadRuntimes} from '../src/server.ts';
import {fixtureModel, call, lastToolText, type FixtureStep} from './fixture-model.ts';

// Binary input above the old 1 MiB text bound: capture must accept it.
const xlsx = Buffer.concat([Buffer.from([0x50, 0x4b, 0x03, 0x04]), Buffer.alloc(1_500_000)]);
const xlsxModified = Buffer.from(xlsx); xlsxModified[3] = 88;
const bin = Buffer.from([1, 0, 3, 4]);
const encode = (data: Buffer | string) => Buffer.from(data).toString('base64');
const settings = JSON.stringify({defaultProvider: 'zepi-fixture', defaultModel: 'scripted', defaultTools: ['powershell'], compaction: {enabled: false}});

type Snapshot = { content: string; disk: string | null; buffer: string | null };
type Write = { path: string; content: string; meta: Record<string, unknown> };

// agentDir (settings, journals, run storage) is kept outside the session workspace,
// as in production, so the outside-capture scan never sees agent runtime files.
async function shellFixture(agentDir: string, steps: FixtureStep[], snapshots: Map<string, Snapshot>,
  published: {reviews: any[]}[] = [], textPublished: {reviews: any[]; pending?: string[]}[] = []) {
  const fixture = await fixtureModel(agentDir, steps);
  const toAgent = new TransformStream(), toClient = new TransformStream();
  const server = await startZepiAgent(agentDir, join(agentDir, 'sessions'), {readable: toAgent.readable, writable: toClient.writable}, fixture.runtime);
  const writes: Write[] = [];
  const conn = client({name: 'shell-apply-test'})
    .onNotification(methods.client.session.update, () => {})
    .onRequest(methods.client.session.requestPermission, () => ({outcome: {outcome: 'selected', optionId: 'allow_once'}}))
    .onRequest('_zepi/fs/snapshot', {parse: raw => raw}, ({params}: any) => {
      const snapshot = snapshots.get(params.path);
      assert.ok(snapshot, `Unexpected snapshot request: ${params.path}`);
      return snapshot;
    })
    .onRequest(EDITOR_QUERY, {parse: raw => raw}, ({params}: any) => {
      assert.equal(params.operation, 'find', 'Unexpected editor query operation');
      const found: string[] = [];
      const walk = (local: string) => {
        for (const entry of readdirSync(join(params.path, local), {withFileTypes: true})) {
          if (entry.isDirectory()) { if (entry.name === '.git' || entry.name === 'node_modules') continue; walk(local ? `${local}/${entry.name}` : entry.name); continue; }
          if (entry.isFile()) found.push(local ? `${local}/${entry.name}` : entry.name);
        }
      };
      walk('');
      const limit = params.limit ?? 512;
      return {output: found.length ? found.slice(0, limit).join('\n') : 'No files found matching pattern', truncated: found.length > limit};
    })
    .onRequest('_zepi/binaryReview', {parse: raw => raw}, ({params}: any) => {published.push(params); return {handled: true};})
    .onRequest('_zepi/textReview', {parse: raw => raw}, ({params}: any) => {textPublished.push(params); return {handled: true, restored: params.reviews.length};})
    .onRequest(methods.client.fs.readTextFile, () => {throw new Error('Captured copies must not use text file reads');})
    .onRequest(methods.client.fs.writeTextFile, ({params}: any) => {writes.push({path: params.path, content: params.content, meta: params._meta ?? {}}); return {};})
    .connect({readable: toClient.readable, writable: toAgent.writable});
  return {server, conn, writes, published, textPublished};
}

const agentSetup = async (prefix: string) => {
  const agentDir = await realpath(await mkdtemp(join(tmpdir(), `${prefix}-agent-`)));
  await writeFile(join(agentDir, 'settings.json'), settings);
  const dir = await realpath(await mkdtemp(join(tmpdir(), `${prefix}-ws-`)));
  return {agentDir, dir, cleanup: async () => {await rm(dir, {recursive: true, force: true}); await rm(agentDir, {recursive: true, force: true});}};
};

const toolResult = (context: Parameters<FixtureStep>[0], name: string) => {
  const message = context.messages.findLast(m => m.role === 'toolResult' && m.toolName === name);
  assert.ok(message && message.role === 'toolResult', `Missing tool result: ${name}`);
  return message;
};

test('shell applies binary changes with a backup and merges text through buffers', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-apply');
  await writeFile(join(dir, 'book.xlsx'), xlsx);
  await writeFile(join(dir, 'notes.txt'), 'original notes');
  const snapshots = new Map<string, Snapshot>([
    [join(dir, 'book.xlsx'), {content: encode(xlsx), disk: encode(xlsx), buffer: null}],
    [join(dir, 'notes.txt'), {content: encode('original notes'), disk: encode('original notes'), buffer: 'v-notes'}],
  ]);
  const command = "$b=[IO.File]::ReadAllBytes('book.xlsx');$b[3]=88;[IO.File]::WriteAllBytes('book.xlsx',$b);"
    + "[IO.File]::WriteAllText('notes.txt','updated text');[IO.File]::WriteAllBytes('out.bin',[byte[]](1,0,3,4))";
  let backupPath: string | undefined;
  const steps: FixtureStep[] = [
    () => call('powershell', {command, description: 'Update report files', files: ['book.xlsx', 'notes.txt']}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, false);
      const value = JSON.parse(result.content.filter(part => part.type === 'text').map(part => (part as any).text).join('\n'));
      assert.equal(value.cancelled, false);
      assert.equal(value.processCleanup, 'confirmed');
      assert.equal(value.recovery, undefined);
      const byPath = new Map<string, any>(value.changes.map((change: any) => [change.path, change]));
      const modified = byPath.get('book.xlsx');
      assert.equal(modified.status, 'applied'); assert.equal(modified.binary, true); assert.ok(modified.backup);
      backupPath = modified.backup;
      const created = byPath.get('out.bin');
      assert.equal(created.status, 'applied'); assert.equal(created.binary, true); assert.equal(created.backup, undefined);
      const notes = byPath.get('notes.txt');
      assert.equal(notes.status, 'applied'); assert.ok(!notes.binary);
      assert.equal(value.changes.length, 3);
      return 'Report verified.';
    },
  ];
  const textPublished: {reviews: any[]; pending?: string[]}[] = [];
  const {server, conn, writes, published} = await shellFixture(agentDir, steps, snapshots, [], textPublished);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiBinaryReview: 1, zepiTextReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Apply the report through PowerShell.'}]});
    // The live edit is already in the editor; only the pending path list travels now.
    assert.ok(textPublished.some(({pending}) => (pending as string[] | undefined)?.includes('notes.txt')),
      'the pending text review is announced to Zed');
    const byPath = new Map(writes.map(write => [write.path, write]));
    assert.deepEqual(byPath.get(join(dir, 'notes.txt')), {path: join(dir, 'notes.txt'), content: 'updated text', meta: {'zepi.dev/expectedBuffer': 'v-notes'}});
    assert.equal(writes.length, 1);
    assert.deepEqual(await readFile(join(dir, 'book.xlsx')), xlsxModified);
    assert.deepEqual(await readFile(join(dir, 'out.bin')), bin);
    // The buffer holds the update until the user saves; the disk text is untouched.
    assert.equal(await readFile(join(dir, 'notes.txt'), 'utf8'), 'original notes');
    // The backup restores the pre-command binary.
    assert.ok(backupPath);
    assert.deepEqual(await readFile(backupPath!), xlsx);
    // Binary changes are published for the native keep/reject panel.
    const first = published.at(-1);
    assert.ok(first, 'binary changes must be published to Zed');
    const publishedXlsx = first.reviews.find((review: any) => review.path === 'book.xlsx');
    assert.equal(publishedXlsx.kind, 'modified'); assert.equal(publishedXlsx.state, 'pending'); assert.equal(publishedXlsx.backup, backupPath);
    const publishedBin = first.reviews.find((review: any) => review.path === 'out.bin');
    assert.equal(publishedBin.kind, 'created'); assert.equal(publishedBin.state, 'pending'); assert.equal(publishedBin.backup, undefined);
    // Reject restores the modified file from its backup.
    const reject = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'book.xlsx', state: 'rejected'} as any);
    assert.deepEqual(reject, {handled: true, path: 'book.xlsx', state: 'rejected'});
    assert.deepEqual(await readFile(join(dir, 'book.xlsx')), xlsx);
    await assert.rejects(access(backupPath!), 'a settled binary review deletes its backup');
    // The pending review survives a reload: the journal is replayed into a new session.
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    const deadline = Date.now() + 5000;
    let restored: {reviews: any[]} | undefined;
    while (Date.now() < deadline) {
      const last = published.at(-1);
      if (last && last.reviews.length === 1 && last.reviews[0].path === 'out.bin' && last.reviews[0].state === 'pending') { restored = last; break; }
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.ok(restored, 'pending binary reviews are re-published after a reload');
    let textRestored: {reviews: any[]; pending?: string[]} | undefined;
    const textDeadline = Date.now() + 5000;
    while (Date.now() < textDeadline) {
      const last = textPublished.at(-1);
      if (last?.reviews.some((review: any) => review.path === 'notes.txt' && review.base === 'v-notes')) { textRestored = last; break; }
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.ok(textRestored, `pending text reviews are re-published after a reload: ${JSON.stringify(textPublished)}`);
    // Settling a review stops the replay: the journal must not offer it again.
    const settled: any = await conn.agent.request('_zepi/textReview/resolve', {sessionId: session.sessionId, path: 'notes.txt', state: 'kept'});
    assert.deepEqual(settled, {handled: true});
    const settledAgain: any = await conn.agent.request('_zepi/textReview/resolve', {sessionId: session.sessionId, path: 'notes.txt', state: 'rejected'});
    assert.equal(settledAgain.handled, false);
    const beforeSettledReload = textPublished.length;
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    await new Promise(resolve => setTimeout(resolve, 500));
    assert.ok(!textPublished.slice(beforeSettledReload).some(({reviews}) => reviews.some((review: any) => review.path === 'notes.txt')),
      'a settled text review is never replayed again');
    const leftover: string[] = [];
    for (const session of await readdir(join(agentDir, 'text-reviews')).catch(() => [])) {
      for (const snapshot of await readdir(join(agentDir, 'text-reviews', session)).catch(() => [])) {
        leftover.push(...await readdir(join(agentDir, 'text-reviews', session, snapshot)).catch(() => []));
      }
    }
    assert.equal(leftover.length, 0, 'settled text reviews leave no snapshot files behind');
    const keep: any = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'out.bin', state: 'kept'});
    assert.deepEqual(keep, {handled: true, path: 'out.bin', state: 'kept'});
    assert.deepEqual(await readFile(join(dir, 'out.bin')), bin);
    const stale: any = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'out.bin', state: 'rejected'});
    assert.equal(stale.handled, false);
    assert.deepEqual(await readFile(join(dir, 'out.bin')), bin);
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});

test('shell keeps deletes in the copy and refuses oversized text inputs', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-keep');
  await writeFile(join(dir, 'gone.txt'), 'gone content');
  const snapshots = new Map<string, Snapshot>([
    [join(dir, 'gone.txt'), {content: encode('gone content'), disk: encode('gone content'), buffer: 'v-gone'}],
    [join(dir, 'big.txt'), {content: encode('a'.repeat(2 * 1024 * 1024)), disk: null, buffer: 'v-big'}],
  ]);
  const steps: FixtureStep[] = [
    () => call('powershell', {command: 'Remove-Item gone.txt', description: 'Remove the stale marker', files: ['gone.txt']}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, true);
      assert.match(result.content.map((part: any) => part.text ?? '').join('\n'), /Command output is preserved at/);
      return call('powershell', {command: 'Write-Output ok', description: 'Probe the oversized input', files: ['big.txt']});
    },
    context => {
      assert.throws(() => lastToolText(context, 'powershell'), /Inputs exceed the supported size/);
      return 'Shell keep semantics verified.';
    },
  ];
  const {server, conn} = await shellFixture(agentDir, steps, snapshots);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}}});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Verify deletion and limits.'}]});
    assert.equal(await readFile(join(dir, 'gone.txt'), 'utf8'), 'gone content');
    await assert.rejects(access(join(dir, 'big.txt')));
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});



test('write tool edits are replayed into new buffers after a reload', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-write-replay');
  await writeFile(join(agentDir, 'settings.json'), JSON.stringify({defaultProvider: 'zepi-fixture', defaultModel: 'scripted', defaultTools: ['write'], compaction: {enabled: false}}));
  const steps: FixtureStep[] = [
    context => {
      assert.equal(context.messages.filter(message => message.role === 'toolResult').length, 0);
      return call('write', {path: 'note.txt', content: 'written by the tool'});
    },
    () => 'Write verified.',
  ];
  const {server, conn, writes} = await shellFixture(agentDir, steps, new Map());
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiTextReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Write a note.'}]});
    assert.equal(writes.length, 1);
    assert.equal(writes[0]!.meta['zepi.dev/createFile'], true);
    // The review is journaled by the tool itself, so a reload brings the buffer back.
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    const before = writes.length;
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline && writes.length === before) {
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.equal(writes.length, before + 1, 'a write tool review survives a reload');
    assert.equal(writes.at(-1)!.path, join(dir, 'note.txt'), 'the replay resolves an absolute path');
    assert.equal(writes.at(-1)!.content, 'written by the tool');
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});

test('created text reviews are replayed into new buffers after a reload', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-created');
  const steps: FixtureStep[] = [
    () => call('powershell', {command: "Set-Content poem.txt 'poem text'", description: 'Create a poem', files: []}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, false);
      const value = JSON.parse(result.content.filter(part => part.type === 'text').map(part => (part as any).text).join('\n'));
      const created = value.changes.find((change: any) => change.path === 'poem.txt');
      assert.equal(created.kind, 'created');
      assert.equal(created.status, 'applied');
      assert.ok(!created.binary);
      return 'Created file review verified.';
    },
  ];
  const {server, conn, writes} = await shellFixture(agentDir, steps, new Map());
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiTextReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Create a poem through PowerShell.'}]});
    assert.equal(writes.length, 1);
    const applied = writes[0]!;
    assert.equal(applied.meta['zepi.dev/createFile'], true);
    assert.match(applied.content, /poem text/);
    // Buffers do not survive a reload: the replay re-creates the buffer with its text.
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    const before = writes.length;
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    const deadline = Date.now() + 5000;
    while (Date.now() < deadline && writes.length === before) {
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.equal(writes.length, before + 1, 'the created text review is replayed after a reload');
    const replay = writes.at(-1)!;
    assert.equal(replay.path, join(dir, 'poem.txt'), 'the replay resolves an absolute path');
    assert.equal(replay.meta['zepi.dev/createFile'], true);
    assert.match(replay.content, /poem text/);
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});


test('an autosaved intermediate agent version still replays toward the final one', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-autosave');
  const textPublished: {reviews: any[]; pending?: string[]}[] = [];
  const steps: FixtureStep[] = [
    () => call('powershell', {command: "[IO.File]::WriteAllText('script.py','v1')", description: 'Write the first version', files: []}),
    () => call('powershell', {command: "[IO.File]::WriteAllText('script.py','v2 final')", description: 'Write the final version', files: []}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, false);
      return 'Two versions verified.';
    },
  ];
  const {server, conn} = await shellFixture(agentDir, steps, new Map(), [], textPublished);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiTextReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Write a script twice.'}]});
    // Autosave caught the first version; the final one lived only in the buffer.
    await writeFile(join(dir, 'script.py'), 'v1');
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    const deadline = Date.now() + 5000;
    let replay: {reviews: any[]} | undefined;
    while (Date.now() < deadline) {
      const last = textPublished.at(-1);
      if (last?.reviews.some((review: any) => review.path === 'script.py')) { replay = last; break; }
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.ok(replay, `the script review is replayed: ${JSON.stringify(textPublished)}`);
    const script = replay!.reviews.find((review: any) => review.path === 'script.py')!;
    // The review goes from the autosaved intermediate toward the agent's final text.
    assert.equal(script.base, 'v1');
    assert.equal(script.current, 'v2 final');
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});


test('a CRLF autosave of an LF buffer still replays its review', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-eol');
  await writeFile(join(agentDir, 'settings.json'), JSON.stringify({defaultProvider: 'zepi-fixture', defaultModel: 'scripted', defaultTools: ['write'], compaction: {enabled: false}}));
  const textPublished: {reviews: any[]; pending?: string[]}[] = [];
  const steps: FixtureStep[] = [
    () => call('write', {path: 'eol.txt', content: 'line one\nline two\nline three'}),
    () => 'EOL write verified.',
  ];
  const {server, conn} = await shellFixture(agentDir, steps, new Map(), [], textPublished);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiTextReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Write with LF.'}]});
    // PowerShell-flavoured autosave: the same text with CRLF endings.
    await writeFile(join(dir, 'eol.txt'), 'line one\r\nline two\r\nline three');
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    const deadline = Date.now() + 5000;
    let replay: {reviews: any[]} | undefined;
    while (Date.now() < deadline) {
      const last = textPublished.at(-1);
      if (last?.reviews.some((review: any) => review.path === 'eol.txt')) { replay = last; break; }
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    assert.ok(replay, `the CRLF-saved review is replayed: ${JSON.stringify(textPublished)}`);
    const review = replay!.reviews.find((item: any) => item.path === 'eol.txt')!;
    assert.equal(review.base, '');
    assert.equal(review.current, 'line one\nline two\nline three');
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});



test('dead runtime leases are swept', async () => {
  const root = await realpath(await mkdtemp(join(tmpdir(), 'zepi-sweeps-')));
  try {
    const runtimes = join(root, 'runtimes');
    await mkdir(runtimes, {recursive: true});
    await writeFile(join(runtimes, `${process.pid}.json`), '{}');
    await writeFile(join(runtimes, '99999999.json'), '{}');
    await writeFile(join(runtimes, '99999999.mcp.json'), '{}');
    await sweepDeadRuntimes(runtimes);
    await assert.doesNotReject(access(join(runtimes, `${process.pid}.json`)), 'a live process keeps its lease');
    await assert.rejects(access(join(runtimes, '99999999.json')), 'a dead pid lease is deleted');
    await assert.rejects(access(join(runtimes, '99999999.mcp.json')), 'a dead pid status is deleted');
  } finally {await rm(root, {recursive: true, force: true});}
});

test('run retention sweeps stale shell directories only', async () => {
  const storage = await realpath(await mkdtemp(join(tmpdir(), 'zepi-run-retention-')));
  try {
    const stale = join(storage, 'hash', 'old-run');
    await mkdir(stale, {recursive: true});
    await writeFile(join(stale, 'manifest.json'), '{}');
    const fresh = join(storage, 'hash', 'new-run');
    await mkdir(fresh, {recursive: true});
    await writeFile(join(fresh, 'manifest.json'), '{}');
    const old = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000);
    await utimes(join(stale, 'manifest.json'), old, old);
    await utimes(stale, old, old);
    await sweepStaleRuns(storage);
    await assert.rejects(access(stale), 'a run older than the retention window is deleted');
    await assert.doesNotReject(access(fresh), 'a fresh run survives the sweep');
  } finally {await rm(storage, {recursive: true, force: true});}
});

test('shell registers absolute-path binary writes that bypass the capture', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-outside');
  const command = `[IO.File]::WriteAllBytes('${dir}\\abs-out.bin',[byte[]](9,0,8));`
    + `[IO.File]::WriteAllText('${dir}\\abs-note.txt','outside write')`;
  const steps: FixtureStep[] = [
    () => call('powershell', {command, description: 'Write through absolute paths', files: []}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, false);
      const value = JSON.parse(result.content.filter(part => part.type === 'text').map(part => (part as any).text).join('\n'));
      const byPath = new Map<string, any>(value.changes.map((change: any) => [change.path, change]));
      assert.equal(value.changes.length, 1);
      const binary = byPath.get('abs-out.bin');
      assert.equal(binary.kind, 'created'); assert.equal(binary.status, 'applied'); assert.equal(binary.binary, true);
      assert.equal(binary.backup, undefined);
      const note = value.outside.find((entry: any) => entry.path === 'abs-note.txt');
      assert.equal(note.kind, 'created');
      assert.match(note.note, /outside the captured copy/);
      return 'Outside writes reported.';
    },
  ];
  const {server, conn, published} = await shellFixture(agentDir, steps, new Map());
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiBinaryReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Write files outside the capture.'}]});
    const first = published.at(-1);
    assert.ok(first, 'the created binary must be published for keep/reject');
    const review = first.reviews.find((item: any) => item.path === 'abs-out.bin');
    assert.equal(review.kind, 'created'); assert.equal(review.state, 'pending');
    assert.deepEqual(await readFile(join(dir, 'abs-out.bin')), Buffer.from([9, 0, 8]));
    // Reject removes the created file: nothing existed to back up.
    const reject: any = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'abs-out.bin', state: 'rejected'});
    assert.deepEqual(reject, {handled: true, path: 'abs-out.bin', state: 'rejected'});
    await assert.rejects(access(join(dir, 'abs-out.bin')));
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});

test('a binary review whose target was deleted drops out on reload', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-vanished');
  await writeFile(join(dir, 'deck.pptx'), xlsx);
  const snapshots = new Map<string, Snapshot>([
    [join(dir, 'deck.pptx'), {content: encode(xlsx), disk: encode(xlsx), buffer: null}],
  ]);
  const steps: FixtureStep[] = [
    () => call('powershell', {command: "$b=[IO.File]::ReadAllBytes('deck.pptx');$b[3]=88;[IO.File]::WriteAllBytes('deck.pptx',$b)", description: 'Modify the deck', files: ['deck.pptx']}),
    () => 'Deck modified.',
  ];
  const published: {reviews: any[]}[] = [];
  const {server, conn} = await shellFixture(agentDir, steps, snapshots, published);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiBinaryReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Modify the deck.'}]});
    assert.ok(published.at(-1)?.reviews.some((review: any) => review.path === 'deck.pptx'), 'the live review is published');
    await conn.agent.request(methods.agent.session.close, {sessionId: session.sessionId});
    await rm(join(dir, 'deck.pptx'));
    const before = published.length;
    await conn.agent.request(methods.agent.session.load, {sessionId: session.sessionId, cwd: dir, mcpServers: []});
    await new Promise(resolve => setTimeout(resolve, 500));
    assert.ok(!published.slice(before).some(({reviews}) => reviews.some((review: any) => review.path === 'deck.pptx')),
      'a deleted target is not re-published after a reload');
    const settle: any = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'deck.pptx', state: 'kept'});
    assert.equal(settle.handled, false, 'the vanished review is no longer pending');
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});

test('an empty files list still captures pre-state for absolute-path binary writes', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-empty-files');
  await writeFile(join(dir, 'deck.pptx'), xlsx);
  const snapshots = new Map<string, Snapshot>([
    [join(dir, 'deck.pptx'), {content: encode(xlsx), disk: encode(xlsx), buffer: null}],
  ]);
  const command = `$b=[IO.File]::ReadAllBytes('${dir}\\deck.pptx');$b[3]=88;[IO.File]::WriteAllBytes('${dir}\\deck.pptx',$b)`;
  const steps: FixtureStep[] = [
    () => call('powershell', {command, description: 'Modify the deck through an absolute path', files: []}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, false);
      const value = JSON.parse(result.content.filter(part => part.type === 'text').map(part => (part as any).text).join('\n'));
      const modified = value.changes.find((change: any) => change.path === 'deck.pptx');
      assert.equal(modified.kind, 'modified');
      assert.equal(modified.status, 'applied');
      assert.ok(modified.backup, 'an empty files list captures the pre-command state for Reject');
      return 'Backup verified.';
    },
  ];
  const {server, conn} = await shellFixture(agentDir, steps, snapshots);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiBinaryReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Modify the deck.'}]});
    const reject: any = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'deck.pptx', state: 'rejected'});
    assert.deepEqual(reject, {handled: true, path: 'deck.pptx', state: 'rejected'});
    assert.deepEqual(await readFile(join(dir, 'deck.pptx')), xlsx);
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});

test('a files subset still backs up binaries written through absolute paths', {timeout: 30_000}, async () => {
  const {agentDir, dir, cleanup} = await agentSetup('zepi-shell-subset-binary');
  await writeFile(join(dir, 'deck.pptx'), xlsx);
  await writeFile(join(dir, 'script.py'), 'print(1)');
  const snapshots = new Map<string, Snapshot>([
    [join(dir, 'deck.pptx'), {content: encode(xlsx), disk: encode(xlsx), buffer: null}],
    [join(dir, 'script.py'), {content: encode('print(1)'), disk: encode('print(1)'), buffer: 'print(1)'}],
  ]);
  const command = `$b=[IO.File]::ReadAllBytes('${dir}\\deck.pptx');$b[3]=88;[IO.File]::WriteAllBytes('${dir}\\deck.pptx',$b)`;
  const steps: FixtureStep[] = [
    () => call('powershell', {command, description: 'Run the paint script', files: ['script.py']}),
    context => {
      const result = toolResult(context, 'powershell');
      assert.equal(result.isError, false);
      const value = JSON.parse(result.content.filter(part => part.type === 'text').map(part => (part as any).text).join('\n'));
      const modified = value.changes.find((change: any) => change.path === 'deck.pptx');
      assert.equal(modified.kind, 'modified');
      assert.equal(modified.status, 'applied');
      assert.ok(modified.backup, 'workspace binaries are captured even outside the files subset');
      return 'Backup verified.';
    },
  ];
  const {server, conn} = await shellFixture(agentDir, steps, snapshots);
  try {
    await conn.agent.request(methods.agent.initialize, {protocolVersion: 1, clientCapabilities: {fs: {readTextFile: true, writeTextFile: true}, _meta: {zepiBinaryReview: 1}} as any});
    const session = await conn.agent.request(methods.agent.session.new, {cwd: dir, mcpServers: []});
    await conn.agent.request(methods.agent.session.prompt, {sessionId: session.sessionId, prompt: [{type: 'text', text: 'Paint the deck.'}]});
    const reject: any = await conn.agent.request('_zepi/binaryReview/resolve', {sessionId: session.sessionId, path: 'deck.pptx', state: 'rejected'});
    assert.deepEqual(reject, {handled: true, path: 'deck.pptx', state: 'rejected'});
    assert.deepEqual(await readFile(join(dir, 'deck.pptx')), xlsx);
  } finally {await server.agent.dispose(); conn.close(); server.connection.close(); await cleanup();}
});

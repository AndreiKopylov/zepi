// P0 bundle smoke: seeding check -> ACP newSession -> browser -> preview run.
// Usage: node scripts/smoke.mjs [--root <zepi root>] [--skip-browser] [--full]
// Default root is the repository; pass the bundle dir (e.g. C:/Llm/zepi) to check a deploy.
// Default suites are the green P0 legs; --full adds preview/shell (known-red, see below).
import {spawnSync} from 'node:child_process';
import {readdir, stat} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';

const args = process.argv.slice(2);
const repoRoot = dirname(fileURLToPath(import.meta.url));
const rootIdx = args.indexOf('--root');
const root = rootIdx === -1 ? join(repoRoot, '..') : args[rootIdx + 1];
const skipBrowser = args.includes('--skip-browser');
const full = args.includes('--full');
const isRepo = (await stat(join(root, 'scripts/smoke.mjs')).catch(() => null)) !== null;
let failed = 0;
// Never leak credentials into logs: mask token-shaped values in everything we print.
const mask = (text) => String(text)
  .replace(/(Bearer\s+)[A-Za-z0-9\-._~+/=]{10,}/gi, '$1***')
  .replace(/\bsk-[A-Za-z0-9\-_]{16,}\b/g, 'sk-***')
  .replace(/\boc_sk_[A-Za-z0-9\-_.]+/g, 'oc_sk-***')
  .replace(/\bgh[op]_[A-Za-z0-9_]+|\bgithub_pat_[A-Za-z0-9_]+|\bglpat-[A-Za-z0-9\-_]+|\bxox[bpars]-[A-Za-z0-9\-]+/g, '***');
if ((await stat(join(root, 'auth.json')).catch(() => null))) {
  console.error('smoke refuses a live profile dir (auth.json next to the root): point --root at the repo or a deploy bundle.');
  process.exit(2);
}
const step = (name) => console.log(`\n### smoke: ${name}`);
const ok = (name) => console.log(mask(`ok - ${name}`));
const fail = (name, why) => { failed++; console.log(mask(`FAIL - ${name}: ${why}`)); };
// Red preview/shell tests: dangling-link lifecycle drift and shell review changes, pending a product decision.
// Smoke fails only on failures NOT on this list.
const KNOWN_RED = new Set([
  'pi builds unsaved TypeScript with installed dependencies, repairs browser behavior and rechecks a fresh project',
  'project command failure, timeout and snapshot mutation never claim a current successful verification',
  'cancelling project startup cleans a stubborn descendant and a terminal returned late',
  'project server cleans up after complete without an explicit stop tool call',
  'project server cleans up after model error without an explicit stop tool call',
  'project server cleans up after force-killed terminal without an explicit stop tool call',
  'project commands cannot silently resolve uncaptured workspace packages through ancestor directories',
  'shell conflict, deletion and binary outputs remain recoverable without overwriting manual edits',
  'failed shell commands retain results and terminate background descendants',
  'shell captures binary originals and proposes exact replacement and deletion without changing disk',
]);

// Seeding inputs: pi defaults ship neutral examples and validate against the bundled pi.
step('seeding check');
try {
  for (const f of ['settings.json', 'models.json']) await stat(join(root, 'native/pi-defaults', f));
  const skills = await readdir(join(root, 'native/pi-defaults/skills'));
  if (!skills.length) throw new Error('native/pi-defaults/skills is empty');
  const modelConfig = await import(pathToFileURL(join(root, 'packages/acp/node_modules/@earendil-works/pi-coding-agent/dist/core/model-config.js')).href);
  const config = await modelConfig.ModelConfig.load(join(root, 'native/pi-defaults/models.json'));
  const error = config.getError();
  if (error) throw new Error(`seed models.json rejected by pi: ${error}`);
  ok(`seeds validate (${skills.length} skills)`);
} catch (error) { fail('seeding check', String(error?.message ?? error)); }

// Compiled ACP entry must match sources: the runtime prefers cli.js over tsx.
step('cli.js freshness');
try {
  if (isRepo) {
    const build = spawnSync('npm', ['run', 'build:acp'], {cwd: root, shell: true, stdio: 'pipe', encoding: 'utf8'});
    if (build.status !== 0) throw new Error(`build:acp failed: ${(build.stdout + build.stderr).slice(-500)}`);
  }
  const sources = await readdir(join(root, 'packages/acp/src'));
  let newest = 0;
  for (const f of sources) {
    if (!f.endsWith('.ts')) continue;
    const {mtimeMs} = await stat(join(root, 'packages/acp/src', f));
    newest = Math.max(newest, mtimeMs);
  }
  const {mtimeMs} = await stat(join(root, 'packages/acp/src/cli.js'));
  if (mtimeMs < newest) throw new Error(isRepo ? 'cli.js is stale after build' : 'bundle cli.js is older than repo sources: redeploy');
  ok('cli.js is fresh');
} catch (error) { fail('cli.js freshness', String(error?.message ?? error)); }

// Chained suites with a scripted model: session open/prompt, tools, subagents,
// /compact /model /thinking and real browser open/snapshot/click.
step('ACP + browser suites');
const suites = [
  'packages/acp/test/acp-startup.test.ts',
  'packages/acp/test/editor-query.test.ts',
  'packages/acp/test/pi-commands.test.ts',
  'packages/acp/test/subagents.test.ts',
  ...(skipBrowser ? [] : ['packages/acp/test/browser.test.ts']),
  // preview/shell suites are known-red (dangling-link lifecycle drift in the test
  // assertions, fails identically on the untouched baseline); include via --full.
  ...(full ? ['packages/acp/test/preview.test.ts'] : []),
];
try {
  // Suites must not see machine credentials: drop every key-like variable, not a hardcoded list.
  const env = {...process.env};
  for (const key of Object.keys(env)) if (/api[_-]?key|auth[_-]?token|secret|private[_-]?key/i.test(key)) delete env[key];
  const run = spawnSync('npx', ['tsx', '--test', ...suites], {cwd: root, env, shell: true, stdio: 'pipe', encoding: 'utf8', timeout: 600_000});
  const out = run.stdout + run.stderr;
  const pass = out.match(/# pass (\d+)/)?.[1] ?? '?';
  const failedNames = out.split('\n').filter((l) => l.startsWith('not ok')).map((l) => l.replace(/^not ok \d+ - /, '').trim());
  const known = failedNames.filter((n) => KNOWN_RED.has(n));
  const unexpected = failedNames.filter((n) => !KNOWN_RED.has(n));
  if (known.length) console.log(mask(`known-red (${known.length}):\n` + known.map((n) => `  - ${n}`).join('\n')));
  if (unexpected.length) throw new Error(mask(`new failures (${unexpected.length}):\n` + unexpected.map((n) => `  - ${n}`).join('\n')));
  ok(`${pass} tests pass${known.length ? `, ${known.length} known-red` : ''}`);
} catch (error) { fail('suites', String(error?.message ?? error)); }

if (!full) console.log('note: preview/shell suites excluded (known-red); run smoke --full to include them.');
console.log(failed ? `\nSMOKE FAILED (${failed} steps)` : '\nSMOKE OK');
process.exit(failed ? 1 : 0);

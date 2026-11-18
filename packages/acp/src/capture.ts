import { createHash, randomUUID } from "node:crypto";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { appendFile, cp, mkdir, mkdtemp, readdir, readFile, realpath, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { delimiter, join, relative, sep } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { methods, type AgentContext } from "@agentclientprotocol/sdk";
import { defineTool, type Extension } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { captureDependencies, changedDependencies, type Dependencies } from "./capture-dependencies.ts";
import { workspacePath } from "./workspace-path.ts";

const hash = (value: string | Buffer) => createHash("sha256").update(value).digest("hex");
const text = (value: unknown) => ({ type: "text" as const, text: JSON.stringify(value, null, 2) });
const commandSchema = Type.Object({
  command: Type.String({ minLength: 1, maxLength: 4096 }),
  args: Type.Optional(Type.Array(Type.String({ maxLength: 16384 }), { maxItems: 128 })),
  timeoutSeconds: Type.Optional(Type.Integer({ minimum: 1, maximum: 300 })),
});
type Command = { command: string; args?: string[]; timeoutSeconds?: number };
type Check = { command: string; args: string[]; exitCode: number | null; signal: string | null; timedOut: boolean; stdout: string; stderr: string };
type Input = { path: string; hash: string; differsFromDisk: boolean };
type Run = {
  runId: string; cwd: string; toolCallId: string; fingerprint: string; files: Input[]; capturedAt: string;
  commands?: Command[]; dependencies: Dependencies; directory: string; stopped?: boolean; cleanup?: string;
  child?: ChildProcess; exit?: { code: number | null; signal: string | null }; checks: Check[]; error?: string;
};

async function removeTree(path: string) {
  // Antivirus and search indexers briefly hold freshly written files on Windows (EPERM/EBUSY).
  for (const wait of [0, 150, 400, 900]) {
    if (wait) await delay(wait);
    try { await rm(path, { recursive: true, force: true }); return; }
    catch (error) {
      const code = (error as NodeJS.ErrnoException).code ?? '';
      if (!['EPERM', 'EBUSY', 'EACCES', 'ENOTEMPTY'].includes(code)) throw error;
    }
  }
  await rm(path, { recursive: true, force: true }).catch(() => {});
}

const sweptStorages = new Set<string>();
async function sweepDanglingLinks(storage: string) {
  if (sweptStorages.has(storage)) return;
  sweptStorages.add(storage);
  for (const sessionDir of await readdir(storage).catch(() => [] as string[])) {
    for (const runDir of await readdir(join(storage, sessionDir)).catch(() => [] as string[])) {
      const link = join(storage, sessionDir, runDir, "files");
      await realpath(link).then(() => undefined, () => rm(link, { force: true }).catch(() => undefined));
    }
  }
}

// Kill the whole command tree: process group on POSIX, taskkill /T on Windows.
function killTree(child: ChildProcess | undefined) {
  if (!child?.pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/T", "/F", "/PID", String(child.pid)], { stdio: "ignore", windowsHide: true });
  } else {
    try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} }
  }
}

const CAP = 1024 * 1024;
function capped() {
  let size = 0; const chunks: Buffer[] = [];
  return {
    push(b: Buffer) { if (size < CAP) { chunks.push(b); size += b.length; } },
    toString() { return Buffer.concat(chunks).toString("utf8"); },
  };
}

export function createCapture(cwd: string, storage: string, sessionId: string, client: AgentContext) {
  void sweepDanglingLinks(storage);
  let active: Run | undefined;
  let stopping: Promise<void> | undefined;
  const temporaryInputs = new Map<string, Run>();

  async function releaseInputs() {
    for (const [path, run] of temporaryInputs) {
      if (run.child && !run.stopped) continue;
      await removeTree(path);
      temporaryInputs.delete(path);
      await rm(join(run.directory, "files"), { force: true });
    }
  }

  async function observe(path: string, signal?: AbortSignal) {
    const canonical = await workspacePath(cwd, path, true);
    const response = await client.request<{ content: string; disk: string | null; buffer: string | null }>("_zepi/fs/snapshot", { sessionId, path: canonical }, { cancellationSignal: signal })
      .catch(error => {
        signal?.throwIfAborted();
        const detail = typeof error?.data === "string" ? error.data : error instanceof Error ? error.message : "Snapshot unavailable";
        throw new Error(`Cannot capture ${path}: ${detail}. Select the complete files needed by this command, use dependencies for installed assets, or files: [] for a command requiring no workspace inputs.`);
      });
    const content = Buffer.from(response.content, "base64");
    if (content.toString("base64") !== response.content) throw new Error("Invalid native file snapshot.");
    signal?.throwIfAborted();
    return { canonical, content };
  }

  async function freshness(run: Run, signal?: AbortSignal) {
    const checkedAt = new Date().toISOString();
    const changed: string[] = [], unavailable: string[] = [];
    const bounded = signal ? AbortSignal.any([signal, AbortSignal.timeout(15_000)]) : AbortSignal.timeout(15_000);
    for (const file of run.files) {
      signal?.throwIfAborted();
      if (bounded.aborted) { unavailable.push(file.path); continue; }
      try {
        const current = await observe(file.path, bounded);
        if (hash(current.content) !== file.hash) changed.push(file.path);
        if (hash(await readFile(join(run.directory, "files", file.path), { signal: bounded })) !== file.hash) changed.push(`snapshot:${file.path}`);
      } catch { signal?.throwIfAborted(); unavailable.push(file.path); }
    }
    if (run.dependencies.entries.length) {
      for (const [base, snapshot] of [[cwd, false], [join(run.directory, "files"), true]] as const) {
        const result = await changedDependencies(base, run.dependencies.entries, bounded, snapshot);
        changed.push(...result.changed.map(p => snapshot ? `snapshot:${p}` : p));
        unavailable.push(...result.unavailable.map(p => snapshot ? `snapshot:${p}` : p));
      }
    }
    signal?.throwIfAborted();
    return { state: unavailable.length ? "unknown" : changed.length ? "stale" : "current", checkedAt, changed, unavailable };
  }

  function description(run: Run) {
    return { runId: run.runId, toolCallId: run.toolCallId, cwd: run.cwd, fingerprint: run.fingerprint, capturedAt: run.capturedAt,
      files: run.files, includesUnsavedBuffers: true,
      dependencies: { paths: run.dependencies.roots, entries: run.dependencies.entries.length, bytes: run.dependencies.bytes },
      commands: run.commands, processCleanup: run.cleanup,
      scope: "Records listed editor inputs and copied installed dependencies; system tools and external resources are outside the fingerprint. Commands run locally in the captured directory, not an OS sandbox. Generated outputs are not copied back.",
      lifetime: "Commands and temporary inputs close when the turn ends or is cancelled; recorded evidence remains." };
  }

  function projectResult(run: Run) {
    const failed = run.error || run.checks.some(c => c.exitCode !== 0 || c.timedOut);
    return { runId: run.runId, stage: run.stopped ? "exited" : failed ? "failed" : "completed", checks: run.checks, error: run.error };
  }

  async function stop() {
    if (stopping) return stopping;
    const run = active;
    if (!run || run.stopped) return;
    stopping = (async () => {
      killTree(run.child);
      run.child = undefined;
      run.cleanup = "confirmed";
      run.stopped = true;
    })().finally(() => { stopping = undefined; });
    return stopping;
  }

  async function status(run: Run, signal?: AbortSignal) {
    const service = run.stopped ? "stopped" : run.child ? "running" : run.exit ? "exited" : "unknown";
    return { ...description(run), service, project: projectResult(run), freshness: await freshness(run, signal) };
  }

  const details = (value: Record<string, unknown>) => ({ zepiShell: { version: 1, ...value } });

  // Run one command in the captured copy, in-process, with timeout and tree-kill.
  function runCommand(run: Run, spec: Command, cwdDir: string, env: NodeJS.ProcessEnv, signal?: AbortSignal): Promise<Check> {
    return new Promise<Check>(resolve => {
      const stdout = capped(), stderr = capped();
      const child = spawn(spec.command, spec.args ?? [], { cwd: cwdDir, env, stdio: ["ignore", "pipe", "pipe"], windowsHide: true, detached: process.platform !== "win32" });
      run.child = child;
      let timedOut = false;
      const timer = setTimeout(() => { timedOut = true; killTree(child); }, (spec.timeoutSeconds ?? 60) * 1000);
      const onAbort = () => killTree(child);
      signal?.addEventListener("abort", onAbort, { once: true });
      child.stdout?.on("data", b => stdout.push(b));
      child.stderr?.on("data", b => stderr.push(b));
      child.once("error", error => {
        clearTimeout(timer); signal?.removeEventListener("abort", onAbort);
        run.child = undefined; run.exit = { code: null, signal: null };
        resolve({ command: spec.command, args: spec.args ?? [], exitCode: null, signal: null, timedOut, stdout: stdout.toString(), stderr: String(error) });
      });
      child.once("exit", (code, sig) => {
        clearTimeout(timer); signal?.removeEventListener("abort", onAbort);
        killTree(child); // reap lingering descendants
        run.child = undefined; run.exit = { code, signal: sig };
        resolve({ command: spec.command, args: spec.args ?? [], exitCode: code, signal: sig, timedOut, stdout: stdout.toString(), stderr: stderr.toString() });
      });
    });
  }

  const tool = defineTool({
    name: "capture", label: "Captured workspace runner", executionMode: "sequential",
    description: "Run shell commands against a captured copy of current editor buffers (including unsaved/new files) and copied installed dependencies. Commands run in the copy, not an OS sandbox; outputs are reviewed and merged back by the caller. Nothing is installed automatically.",
    promptSnippet: "Run shell commands on current editor contents",
    parameters: Type.Object({
      action: Type.Union([Type.Literal("start"), Type.Literal("status"), Type.Literal("stop")]),
      files: Type.Optional(Type.Array(Type.String(), { minItems: 1, maxItems: 512 })),
      dependencies: Type.Optional(Type.Array(Type.String(), { maxItems: 16 })),
      commands: Type.Optional(Type.Array(commandSchema, { minItems: 1, maxItems: 8 })),
    }),
    async execute(toolCallId, params, signal) {
      signal?.throwIfAborted();
      if (params.action === "stop") { await stop(); const value = active ? await status(active, signal) : { service: "not_started" }; return { content: [text(value)], details: active ? details(value) : {} }; }
      if (params.action === "status") { const value = active ? await status(active, signal) : { service: "not_started" }; return { content: [text(value)], details: active ? details(value) : {} }; }
      if (!params.commands?.length) throw new Error("Start requires at least one command.");
      for (const spec of params.commands) if ([spec.command, ...spec.args ?? []].some(v => v.includes("\0"))) throw new Error("Commands and arguments must not contain NUL bytes.");
      const root = await realpath(cwd);
      const inputs = new Map<string, { content: Buffer; differsFromDisk: boolean }>();
      let bytes = 0;
      for (const path of params.files ?? []) {
        const local = relative(root, await workspacePath(root, path, true)).split(sep).join("/");
        if (inputs.has(local)) continue;
        const { canonical, content } = await observe(local, signal);
        const size = Buffer.byteLength(content);
        bytes += size;
        if (size > 1024 * 1024 || bytes > 16 * 1024 * 1024) throw new Error("Inputs exceed the supported size (1 MiB per file, 16 MiB total).");
        const disk = await readFile(canonical).catch((e: NodeJS.ErrnoException) => { if (e.code !== "ENOENT") throw e; return undefined; });
        inputs.set(local, { content, differsFromDisk: disk === undefined || hash(disk) !== hash(content) });
      }
      const files = [...inputs].sort(([a], [b]) => a.localeCompare(b)).map(([path, f]) => ({ path, hash: hash(f.content), differsFromDisk: f.differsFromDisk }));
      const runId = randomUUID();
      const run: Run = { runId, cwd: root, toolCallId, files, capturedAt: new Date().toISOString(), commands: params.commands,
        dependencies: { roots: [], entries: [], bytes: 0 }, fingerprint: "", directory: join(storage, hash(sessionId), runId), checks: [] };
      await mkdir(run.directory, { recursive: true, mode: 0o700 });
      const temporary = await realpath(await mkdtemp(join(tmpdir(), "zepi-project-")));
      temporaryInputs.set(temporary, run);
      await symlink(temporary, join(run.directory, "files"));
      for (const [path, f] of inputs) {
        const target = join(run.directory, "files", path);
        await mkdir(join(target, ".."), { recursive: true, mode: 0o700 });
        await writeFile(target, f.content, { mode: 0o600 });
      }
      run.dependencies = await captureDependencies(root, params.dependencies ?? [], join(run.directory, "files"), files.map(f => f.path), signal);
      run.fingerprint = hash(JSON.stringify({ files: files.map(({ path, hash }) => ({ path, hash })), dependencies: run.dependencies, commands: run.commands }));
      await writeFile(join(run.directory, "manifest.json"), JSON.stringify({ ...run, child: undefined, sessionId }, null, 2), { mode: 0o600 });
      const captured = await freshness(run, signal);
      if (captured.state !== "current") throw new Error("Inputs changed during capture. Read the files and start a new run.");
      await stop();
      active = run;
      const filesDir = join(run.directory, "files");
      const env = { ...process.env, PATH: [join(filesDir, "node_modules", ".bin"), process.env.PATH ?? ""].join(delimiter) };
      try {
        for (const spec of params.commands) {
          signal?.throwIfAborted();
          run.checks.push(await runCommand(run, spec, filesDir, env, signal));
        }
        run.cleanup = "confirmed";
      } catch (error) {
        run.error = error instanceof Error ? error.message : String(error);
        await stop();
      }
      const value = await status(run, signal).catch(() => ({ ...description(run), service: "stopped", project: projectResult(run) }));
      await writeFile(join(run.directory, "project-result.json"), JSON.stringify(projectResult(run), null, 2), { mode: 0o600 });
      await writeFile(join(run.directory, "started.json"), JSON.stringify(value, null, 2), { mode: 0o600 });
      const failed = !!run.error || run.checks.some(c => c.exitCode !== 0 || c.timedOut);
      return { content: [text(value)], details: details(value), ...(failed ? { isError: true } : {}) };
    },
  });

  const path = `<inline:zepi-shell>`;
  const extension: Extension = {
    path, resolvedPath: path, sourceInfo: { path, source: "inline", scope: "user", origin: "top-level" },
    handlers: new Map([
      ["session_shutdown", [async () => { try { await stop(); } finally { await releaseInputs(); } }]],
    ]),
    tools: new Map(), messageRenderers: new Map(), commands: new Map(), flags: new Map(), shortcuts: new Map(),
  };
  return { tool, extension, stop, snapshot: () => active, preserveInputs: async () => {
    const run = active;
    if (!run || !run.stopped) throw new Error('Stop the command before preserving its output.');
    const link = join(run.directory, 'files');
    const source = await realpath(link);
    const destination = join(run.directory, 'output-snapshot');
    if (source === destination) return destination;
    try { await rename(source, destination); }
    catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'EXDEV') throw error;
      await cp(source, destination, { recursive: true, verbatimSymlinks: true });
      await removeTree(source);
    }
    temporaryInputs.delete(source);
    await rm(link);
    await symlink(destination, link);
    return destination;
  }, finishTurn: async (stopStatic = false) => {
    try { if (stopStatic || active) await stop(); }
    finally { await releaseInputs(); }
  } };
}
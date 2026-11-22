import { mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";

// Bun entry setup: embeds the codemode QuickJS wasm and registers bun OAuth/bedrock,
// mirroring pi's own compiled binary (runtime resolution of the wasm fails otherwise).
if (process.versions.bun) {
  // pi assets live in subfolders next to the exe to keep the bundle root clean.
  const execDir = resolve(process.execPath, "..");
  if (!process.env.PI_PACKAGE_DIR && existsSync(join(execDir, "pi", "package.json"))) {
    process.env.PI_PACKAGE_DIR = join(execDir, "pi");
  }
  await import("../node_modules/@earendil-works/pi-coding-agent/dist/bun/runtime-setup.js");
}

// Side tools (settings/extensions UI) run as subcommands of the compiled agent so
// they use the embedded pi instead of a Node runtime plus on-disk node_modules.
const subcommand = process.argv[2];
if (subcommand === "mcp-auth") {
  await import("../../runtime/bin/mcp-auth.mjs");
  process.exit(0);
}
if (subcommand === "codemode-selftest") {
  const { CodemodeSandbox } = await import("../node_modules/@earendil-works/pi-codemode/dist/runtime/host.js");
  const { loadQuickJSWasm } = await import("../node_modules/@earendil-works/pi-codemode/dist/wasm.js");
  const { getCodemodeWorkerSpecifier, getQuickJSWasmPath } = await import("../node_modules/@earendil-works/pi-coding-agent/dist/config.js");
  try {
    const sandbox = new CodemodeSandbox({ workerUrl: getCodemodeWorkerSpecifier(), wasm: loadQuickJSWasm(getQuickJSWasmPath()) });
    const result = await sandbox.execute("1+1", {});
    console.log(JSON.stringify({ ok: true, result }));
  } catch (error) {
    console.log(JSON.stringify({ ok: false, error: String((error as Error)?.message ?? error) }));
  }
  process.exit(0);
}
if (subcommand === "pi-settings" || subcommand === "pi-extensions") {
  if (subcommand === "pi-settings") {
    const { createPiSettings, PiConfigError } = await import("../../runtime/src/pi/settings.mjs");
    let input = "";
    try {
      for await (const chunk of process.stdin) { input += chunk; if (input.length > 1_000_000) throw new PiConfigError("The request is too large."); }
      const result = await createPiSettings().execute(input.trim() ? JSON.parse(input) : {});
      process.stdout.write(JSON.stringify({ ok: true, data: result }));
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      const message = error instanceof PiConfigError ? error.message
        : `Unable to update pi settings: ${detail || "check file formats and permissions"}`;
      process.stdout.write(JSON.stringify({ ok: false, error: message }));
      process.exitCode = 1;
    }
  } else {
    const { createExtensionCenter } = await import("../../runtime/src/pi/extensions.mjs");
    const { takeOverStdout, writeRawStdout, flushRawStdout } = await import("../../runtime/src/pi/embed.mjs");
    takeOverStdout();
    try {
      let text = ""; for await (const part of process.stdin) { text += part; if (text.length > 1_000_000) throw new Error("Request is too large."); }
      const data = await createExtensionCenter().execute(text ? JSON.parse(text) : {});
      writeRawStdout(JSON.stringify({ ok: true, data }));
    } catch (error) { writeRawStdout(JSON.stringify({ ok: false, error: (error as Error).message })); process.exitCode = 1; }
    await flushRawStdout();
  }
  process.exit(process.exitCode ?? 0);
}

// Standalone (no Zepi): behave like the pi CLI, sharing Zepi's pi config when present.
if (!process.env.ZEPI_ROOT) {
  if (!process.env.PI_CODING_AGENT_DIR && process.env.LOCALAPPDATA) {
    const zepiPi = join(process.env.LOCALAPPDATA, "Zepi", "pi");
    if (existsSync(zepiPi)) process.env.PI_CODING_AGENT_DIR = zepiPi;
  }
  await import("../node_modules/@earendil-works/pi-coding-agent/dist/cli.js");
  await new Promise(() => {});
}

// Reserve stdout for the existing adapter's ACP transport before loading pi.
console.log = console.error;
console.info = console.error;
console.debug = console.error;
const root = process.env.ZEPI_ROOT;
if (!root) throw new Error("Launch this agent from Zepi's configured workspace.");
const agentDir = resolve(process.env.ZEPI_PI_CONFIG_DIR || resolve(root, ".local/zepi"));
const sessionDir = resolve(agentDir, "acp-sessions");
process.env.PI_CODING_AGENT_DIR = agentDir;
// Windows has no process-tree kill: an ACP server orphaned by a force-killed Zepi
// would otherwise hold session ownership leases until its pid gets reused.
const zepiParent = process.ppid;
const parentWatch = setInterval(() => {
  try { process.kill(zepiParent, 0); }
  catch { clearInterval(parentWatch); process.exit(0); }
}, 2000);
parentWatch.unref();
await mkdir(sessionDir, { recursive: true, mode: 0o700 });
const {claimBundledRuntime}=await import('../../runtime/src/lifecycle.mjs');
const releaseBundle=process.env.ZEPI_INSTALLED_RUNTIME==='1'?async()=>{}:await claimBundledRuntime(root);
process.once('exit',()=>{void releaseBundle();});
const { createExtensionCenter } = await import("../../runtime/src/pi/extensions.mjs");
const { takeOverStdout, restoreStdout } = await import("../../runtime/src/pi/embed.mjs");
takeOverStdout();
const releaseRuntime = await createExtensionCenter(agentDir).acquireRuntime();
restoreStdout();
const { initializePiNetwork } = await import('./pi-network.ts');
try { initializePiNetwork(agentDir); }
catch { await releaseRuntime(); await releaseBundle(); throw new Error('Unable to initialize pi networking. Check global HTTP proxy and timeout settings.'); }
const { startZepiAgent } = await import("./server.ts");
const { agent, connection } = await startZepiAgent(agentDir, sessionDir).catch(async error => {await releaseRuntime(); await releaseBundle(); throw error;});
let shuttingDown: Promise<void> | undefined;
const shutdown = (code: number) => {
  shuttingDown ??= (async () => {
    // Exit promptly so the editor never blocks on a hung MCP/worker dispose at close.
    const deadline = setTimeout(() => process.exit(code), 2_000);
    try { await agent.dispose(); await releaseRuntime(); await releaseBundle(); clearTimeout(deadline); process.exit(code); }
    catch { clearTimeout(deadline); console.error("Zepi ACP cleanup failed."); process.exit(1); }
  })();
  return shuttingDown;
};
connection.closed.then(() => shutdown(0), () => shutdown(1));
process.stdin.on("end", () => { void shutdown(0); });
process.on("SIGTERM", () => { void shutdown(0); });
process.on("SIGINT", () => { void shutdown(0); });

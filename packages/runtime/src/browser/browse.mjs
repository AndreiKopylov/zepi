import { execFile, spawn } from "node:child_process";
import { promisify } from "node:util";
import { readFile, writeFile, mkdir, rename, rm, copyFile, access } from "node:fs/promises";
import { join } from "node:path";

const exec = promisify(execFile);
const BUN_VERSION = "1.4.4";
const EXTENSION_URL = "https://chromewebstore.google.com/detail/mfnicocicmmlkpjnaffgihfjhdgjkdjg";

const npmLatest = async name => {
  const res = await fetch(`https://registry.npmjs.org/${name}/latest`);
  if (!res.ok) throw new Error(`npm registry unreachable for ${name}.`);
  const d = await res.json();
  return { version: d.version, tarball: d.dist.tarball };
};
const download = (url, dest) => exec("curl", ["-fsSL", "--retry", "2", "-o", dest, url], { timeout: 600000 });
const systemTar = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
const extractTgz = async (tgz, dest) => { await mkdir(dest, { recursive: true }); await exec(systemTar, ["-xzf", tgz, "-C", dest]); };
const extractZip = async (zip, dest) => { await mkdir(dest, { recursive: true }); await exec("powershell", ["-NoProfile", "-Command", `Expand-Archive -LiteralPath '${zip}' -DestinationPath '${dest}' -Force`], { timeout: 300000 }); };

const readManifest = dir => readFile(join(dir, "browse.json"), "utf8").then(t => JSON.parse(t)).catch(() => null);

async function stopBrowse(dir) {
  // Windows locks running exe files; stop this folder's relay/cli before overwriting.
  const win = dir.replace(/\//g, "\\").replace(/'/g, "''");
  const ps = `Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith('${win}', [System.StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force -ErrorAction SilentlyContinue`;
  await exec("powershell", ["-NoProfile", "-Command", ps]).catch(() => {});
  await exec("taskkill", ["/F", "/IM", "browse.exe"]).catch(() => {});
}

async function fetchParts(dir, want) {
  if (want.bun || want.relay || want.cli) await stopBrowse(dir);
  const tmp = join(dir, ".download");
  await mkdir(tmp, { recursive: true });
  const manifest = (await readManifest(dir)) ?? {};
  try {
    if (want.bun) {
      const zip = join(tmp, "bun.zip");
      await download(`https://github.com/oven-sh/bun/releases/download/bun-v${BUN_VERSION}/bun-windows-x64.zip`, zip);
      await extractZip(zip, tmp);
      await copyFile(join(tmp, "bun-windows-x64", "bun.exe"), join(dir, "bun.exe"));
      manifest.bun = BUN_VERSION;
    }
    if (want.relay) {
      const meta = await npmLatest("@opencode/browse-windows-x64");
      const tgz = join(tmp, "relay.tgz");
      await download(meta.tarball, tgz);
      await extractTgz(tgz, join(tmp, "relay"));
      const dest = join(dir, "node_modules", "@opencode", "browse-windows-x64");
      await mkdir(join(dest, "bin"), { recursive: true });
      await copyFile(join(tmp, "relay", "package", "bin", "browse.exe"), join(dest, "bin", "browse.exe"));
      await copyFile(join(tmp, "relay", "package", "package.json"), join(dest, "package.json"));
      manifest.relay = meta.version;
    }
    if (want.cli) {
      const meta = await npmLatest("@opencode/browse");
      const tgz = join(tmp, "cli.tgz");
      await download(meta.tarball, tgz);
      await extractTgz(tgz, join(tmp, "cli"));
      await copyFile(join(tmp, "cli", "package", "dist", "cli.mjs"), join(dir, "cli.mjs"));
      manifest.cli = meta.version;
    }
    manifest.installedAt = new Date().toISOString();
    await writeFile(join(dir, "browse.json"), JSON.stringify(manifest, null, 2) + "\n");
    return manifest;
  } finally {
    await rm(tmp, { recursive: true, force: true });
  }
}

export async function installBrowse(dir) {
  await mkdir(dir, { recursive: true });
  return fetchParts(dir, { bun: true, relay: true, cli: true });
}

export async function checkBrowseUpdate(dir) {
  const manifest = await readManifest(dir);
  if (!manifest) throw new Error("browse is not installed in this folder.");
  const [cli, relay] = await Promise.all([npmLatest("@opencode/browse"), npmLatest("@opencode/browse-windows-x64")]);
  return { current: manifest.cli, latest: cli.version, updateAvailable: cli.version !== manifest.cli || relay.version !== manifest.relay };
}

export async function updateBrowse(dir) {
  const manifest = await readManifest(dir);
  if (!manifest) throw new Error("browse is not installed in this folder.");
  const [cli, relay] = await Promise.all([npmLatest("@opencode/browse"), npmLatest("@opencode/browse-windows-x64")]);
  return fetchParts(dir, { bun: false, cli: cli.version !== manifest.cli, relay: relay.version !== manifest.relay });
}

export async function restartBrowseRelay(dir) {
  await stopBrowse(dir);
}

// The relay bakes OPENCODE_BROWSER_TOOLS at startup; `browse restart` re-reads it from env.
// Also revives a dead relay (endpoint unreachable) on session start, with a restart backoff.
let lastRelayRestart = 0;
async function relayAlive() {
  try {
    await fetch("http://127.0.0.1:19988/", { signal: AbortSignal.timeout(1500) });
    return true; // any HTTP response means the relay is up
  } catch { return false; }
}
export async function ensureRelayMode(dir, tools) {
  const manifest = await readManifest(dir);
  if (!manifest) return;
  const modeChanged = manifest.relayTools !== tools;
  const alive = await relayAlive();
  if (!modeChanged && alive) return;
  if (!alive && Date.now() - lastRelayRestart < 30_000) return; // crash-loop guard
  lastRelayRestart = Date.now();
  const relay = join(dir, "node_modules", "@opencode", "browse-windows-x64", "bin", "browse.exe");
  try { spawn(relay, [modeChanged ? "restart" : "serve"], { env: { ...process.env, OPENCODE_BROWSER_TOOLS: tools }, detached: true, stdio: "ignore" }).unref(); } catch { /* relay absent */ }
  if (modeChanged) {
    manifest.relayTools = tools;
    await writeFile(join(dir, "browse.json"), JSON.stringify(manifest, null, 2) + "\n");
  }
}

export async function browseStatus(dir, tools = "plain") {
  const manifest = await readManifest(dir);
  if (!manifest) return { installed: false, extensionUrl: EXTENSION_URL };
  const env = { ...process.env, OPENCODE_BROWSER_TOOLS: tools };
  // Keep a persistent relay (detached) so the extension can connect beyond this call;
  // a relay tied to a short-lived cli process exits before the extension reconnects.
  try {
    spawn(join(dir, "node_modules", "@opencode", "browse-windows-x64", "bin", "browse.exe"), ["serve"], { detached: true, stdio: "ignore", env }).unref();
  } catch { /* already running */ }
  const read = async () => {
    const { stdout } = await exec(join(dir, "bun.exe"), [join(dir, "cli.mjs"), "status", "--json"], { timeout: 20000, env });
    return JSON.parse(stdout);
  };
  // The paired extension's service worker needs a few seconds to reconnect to a fresh
  // relay; wait (bounded) for it instead of reporting a stale "not connected".
  let live = null;
  for (let i = 0; i < 8; i++) {
    try { live = await read(); } catch { /* relay starting */ }
    if (live?.["extension"]?.["connected"] === true) break;
    await new Promise(resolve => setTimeout(resolve, 1000));
  }
  return { installed: true, dir, versions: manifest, live, extensionUrl: EXTENSION_URL };
}

export function pairBrowse(dir) {
  return new Promise((resolve, reject) => {
    const child = spawn(join(dir, "bun.exe"), [join(dir, "cli.mjs"), "pair"], { stdio: ["ignore", "pipe", "pipe"] });
    let out = "";
    const timer = setTimeout(() => { child.kill(); reject(new Error("Pairing did not produce a code in time.")); }, 20000);
    child.stdout.on("data", chunk => {
      out += chunk;
      const m = out.match(/Pairing code:\s*([A-Z0-9-]+)/);
      if (m) { clearTimeout(timer); child.stdout.removeAllListeners("data"); child.unref(); resolve(m[1]); }
    });
    child.on("error", e => { clearTimeout(timer); reject(e); });
    child.on("exit", code => { if (code) { clearTimeout(timer); reject(new Error("browse pair exited: " + code)); } });
  });
}

export async function setBrowseSettings(dir, changes) {
  const { stdout } = await exec(join(dir, "bun.exe"), [join(dir, "cli.mjs"), "settings", "--set", JSON.stringify(changes)], { timeout: 30000 });
  return JSON.parse(stdout);
}

export async function writeBrowseMcp(directory, dir, tools) {
  await mkdir(directory, { recursive: true });
  const path = join(directory, "mcp.json");
  const value = await readFile(path, "utf8").then(t => JSON.parse(t)).catch(() => ({ mcpServers: {} }));
  value.mcpServers = value.mcpServers ?? {};
  value.mcpServers.browse = {
    command: join(dir, "bun.exe"),
    args: [join(dir, "cli.mjs"), "mcp"],
    env: { OPENCODE_BROWSER_TOOLS: tools === "execute" ? "execute" : "plain" },
    exposure: "direct",
  };
  const tmp = path + ".tmp";
  await writeFile(tmp, JSON.stringify(value, null, 2) + "\n");
  await rename(tmp, path);
}

export async function ensureBrowseMcp(directory, dir, tools) {
  try { await access(join(dir, "cli.mjs")); } catch { return false; }
  const value = await readFile(join(directory, "mcp.json"), "utf8").then(t => JSON.parse(t)).catch(() => ({ mcpServers: {} }));
  if (value?.mcpServers?.browse) return false;
  await writeBrowseMcp(directory, dir, tools);
  return true;
}

export async function removeBrowseMcp(directory) {
  const path = join(directory, "mcp.json");
  const value = await readFile(path, "utf8").then(t => JSON.parse(t)).catch(() => null);
  if (!value?.mcpServers?.browse) return;
  delete value.mcpServers.browse;
  const tmp = path + ".tmp";
  await writeFile(tmp, JSON.stringify(value, null, 2) + "\n");
  await rename(tmp, path);
}
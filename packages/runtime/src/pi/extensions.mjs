import {piDirectory, runtimeRoot as root} from '../paths.mjs';
import {DefaultPackageManager, SettingsManager, parseFrontmatter, FileSettingsStorage} from './embed.mjs';
import {validateMcp, credentials, usesOAuth} from '../mcp/config.mjs';
import {readMcpStatus, mcpRevision} from '../mcp/status.mjs';
export {validateMcp, configuredMcp} from '../mcp/config.mjs';
import { mkdir, readFile, writeFile, rm, readdir } from "node:fs/promises";
import { renameWithRetry } from "../fs/rename.mjs";
import { createHash } from "node:crypto";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";

import lockfile from 'proper-lockfile';
const revisionOf = value => createHash("sha256").update(JSON.stringify(value)).digest("hex");
const sourceOf = value => typeof value === "string" ? value : value.source;
const RUNTIME_LEASE_TTL_MS = 6 * 60 * 60 * 1000;
const LEASE_RENEWAL_MS = 15 * 60 * 1000;
const readJson = async (path, fallback) => {
  try { return JSON.parse(await readFile(path, "utf8")); }
  catch (error) { if (error.code === "ENOENT") return fallback; throw new Error(`Invalid ${basename(path)}. It was not changed.`); }
};
const atomic = async (path, data) => {
  await mkdir(dirname(path), {recursive: true, mode: 0o700});
  const temp = `${path}.${process.pid}.${crypto.randomUUID()}.tmp`;
  try { await writeFile(temp, JSON.stringify(data, null, 2) + "\n", {mode: 0o600}); await renameWithRetry(temp, path); }
  finally { await rm(temp, {force: true}); }
};
// An inspection deletes a lease older than its TTL, and with it the MCP status bound to that
// id, so a runtime that outlives the TTL rewrites the lease keeping the same id.
export const renewLease = (path, id) => atomic(path, {id, startedAt: new Date().toISOString()});
const npmName = source => {
  const name = source.replace(/^npm:/, "");
  if (!/^(@[a-z0-9._-]+\/)?[a-z0-9][a-z0-9._-]*(?:@[a-zA-Z0-9.*^~<>=| -]+)?$/.test(name)) throw new Error("Enter an npm package name or an absolute local package path.");
  return name;
};

export function createExtensionCenter(directory = piDirectory(root), fetcher = fetch) {
  const queuePath = join(directory, "package-changes.json"), leases = join(directory, "runtimes");
  const settings = () => {
    const manager = SettingsManager.create(directory, directory, {projectTrusted: false});
    if (manager.drainErrors().length) throw new Error("Fix pi settings.json before managing extensions.");
    return manager;
  };
  const manager = s => new DefaultPackageManager({cwd: directory, agentDir: directory, settingsManager: s});
  const configured = () => readJson(join(directory, "settings.json"), {});
  const mutateSettings = fn => new FileSettingsStorage(directory, directory).withLock("global", raw => JSON.stringify(fn(raw ? JSON.parse(raw) : {}), null, 2) + "\n");
  async function locked(fn, wait = false) {
    await mkdir(directory, {recursive: true, mode: 0o700});
    const lockPath = join(directory, ".extensions.lock");
    // pi's lock implementation refreshes live operations and recovers crashed owners.
    const release = await lockfile.lock(lockPath, {realpath: false, stale: 10_000, retries: wait ? {retries: 60, minTimeout: 250, maxTimeout: 250, factor: 1} : 0})
      .catch(() => {throw new Error("Another extension operation is running. Try again when it finishes.");});
    try { return await fn(); } finally {await release();}
  }
  async function activeRuntimes() {
    const names = await readdir(leases).catch(e => {if (e.code === "ENOENT") return []; throw e;});
    const active = [];
    for (const name of names) {
      if (!/^\d+\.json$/.test(name)) continue;
      const pid = Number(name.slice(0, -5)), path = join(leases, name);
      if (await leaseExpired(path) || !isRunning(pid)) { await rm(path, {force: true}); continue; }
      active.push(pid);
    }
    // MCP snapshots outlive crashed processes; a lease above is the only authority on liveness.
    for (const name of names) {
      if (/^\d+\.mcp\.json$/.test(name) && !active.includes(Number(name.slice(0, -9)))) await rm(join(leases, name), {force: true});
    }
    return active;
  }
  // Windows answers a missing pid with EPERM, not ESRCH, so a permission error means the pid
  // was reused by a foreign process. Our runtimes always run as the current user.
  function isRunning(pid) {
    try { process.kill(pid, 0); return true; }
    catch (error) { return error.code !== "ESRCH" && error.code !== "EPERM"; }
  }
  // An abandoned lease must not block package changes forever.
  async function leaseExpired(path) {
    const started = Date.parse((await readJson(path, {}).catch(() => ({}))).startedAt);
    return !Number.isFinite(started) || Date.now() - started > RUNTIME_LEASE_TTL_MS;
  }
  async function status() {
    const s = settings(), pm = manager(s), config = await configured();
    // Inspection never installs missing packages or contacts git remotes.
    const paths = await pm.resolve(async () => "skip");
    const disabled = config.zepi?.disabledPackages ?? {};
    const packages = s.getPackages().map(pkg => {
      const source = sourceOf(pkg), installedPath = pm.getInstalledPath(source, "user");
      return {source, name: source.replace(/^npm:/, ""), enabled: !(source in disabled), installed: !!installedPath, path: installedPath ?? ""};
    });
    for (const pkg of packages) {
      if (pkg.path) {const manifest = await readJson(join(pkg.path, "package.json"), {}); pkg.name = manifest.name ?? pkg.name; pkg.version = manifest.version ?? "local"; pkg.description = manifest.description ?? "";}
    }
    const resources = async kind => Promise.all(paths[kind].filter(p => p.metadata.scope === "user").map(async p => {
      let name = basename(p.path), description = "";
      if (kind === "skills") {try {const {frontmatter} = parseFrontmatter(await readFile(p.path, "utf8"));
        name = (frontmatter.name || basename(dirname(p.path))).trim();
        // Frontmatter values may carry trailing newlines; one line keeps rows the same height.
        description = String(frontmatter.description ?? "").replace(/\s+/g, " ").trim();} catch {description = "Unable to read skill metadata.";}}
      return {...p, name, description};
    }));
    const mcp = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
    const active = await activeRuntimes();
    const running = await readMcpStatus(directory, active);
    const servers = [...Object.entries(mcp.mcpServers).map(([name, s]) => {
      const signedIn = usesOAuth(s) && !!credentials(directory).tokens(name, s.url);
      const hasAuthHeader = Object.keys(s.headers ?? {}).some(key => key.toLowerCase() === "authorization");
      // `auth` describes the configuration only. Whether a server asks for a sign-in is
      // known from the runtime state (`needs-auth`), not from a missing header.
      const auth = s.auth?.provider ? `token:${s.auth.provider}`
        : hasAuthHeader ? "header"
        : signedIn ? "signed-in"
        : s.oauth ? "oauth-configured"
        : s.url ? "unknown" : "none";
      return {name, enabled: s.enabled !== false, builtin: false, signedIn, auth,
        transport: s.command ? "stdio" : s.type === "sse" ? "sse" : "http",
        exposure: s.exposure ?? "codemode",
        toolExposure: Object.keys(s.toolExposure ?? {}).length,
        detail: s.command ? "Local process" : s.type === "sse" ? "SSE server" : "HTTP server"};
    })];
    for (const state of running) if (!servers.some(server => server.name === state.name)) {
      servers.push({name:state.name, enabled:true, builtin:false, runtimeOnly:true,
        detail:state.origin === 'plugin' ? `Registered by pi plugin${state.plugin ? ` · ${state.plugin}` : ''}` : 'Retained by an open task'});
    }
    for (const server of servers) server.connections = running.filter(state => state.name === server.name).map(state => ({...state,
      configurationChanged: server.builtin ? false : Object.hasOwn(mcp.mcpServers, server.name)
        ? state.origin !== 'global' || state.revision !== mcpRevision(mcp.mcpServers[server.name], mcp.autoEnableCodemode)
        : state.origin === 'global'}));
    return {packages, extensions: await resources("extensions"), skills: await resources("skills"), mcp:servers,
      mcpCodemode: mcp.autoEnableCodemode !== false,
      mcpObservedAt:new Date().toISOString(), pending: await readJson(queuePath, []), activeRuntimes:active.length};
  }
  async function applyPending() {
    if ((await activeRuntimes()).length) return;
    const queue = await readJson(queuePath, []);
    while (queue.length) {
      const request = queue[0], s = settings(), pm = manager(s);
      try {
        if (request.operation === "install") await pm.installAndPersist(request.source);
        else if (request.operation === "update") await pm.update(request.source);
        else if (request.operation === "remove") {
          await pm.removeAndPersist(request.source);
          mutateSettings(c => {if (c.zepi?.disabledPackages) delete c.zepi.disabledPackages[request.source]; return c;});
        }
        await s.flush();
        if (s.drainErrors().length) throw new Error("pi could not save the package configuration.");
        queue.shift(); await atomic(queuePath, queue);
      } catch {request.error = "Package operation failed. Check the source and network, then retry."; await atomic(queuePath, queue); break;}
    }
  }
  async function search(query) {
    if (typeof query !== "string" || query.trim().length > 160) throw new Error("Enter a package search of at most 160 characters.");
    const url = new URL("https://registry.npmjs.org/-/v1/search");
    url.searchParams.set("text", `keywords:pi-package ${query.trim()}`); url.searchParams.set("size", "30");
    const response = await fetcher(url, {signal: AbortSignal.timeout(15_000)});
    if (!response.ok) throw new Error("Package search is unavailable. Try again later.");
    const result = await response.json();
    return {results: (result.objects ?? []).map(o => o.package).filter(p => p.keywords?.includes("pi-package")).map(p => ({name: p.name, source: `npm:${p.name}`, version: p.version, description: p.description ?? ""}))};
  }
  async function execute(request = {}) {
    const operation = request.operation ?? "status";
    if (operation === "status") return status();
    // Network check for newer package versions; an unreachable registry means "nothing known".
    if (operation === "updates") return {updates: await manager(settings()).checkForAvailableUpdates().then(list => list.map(item => item.source), () => [])};
    if (operation === "search") return search(request.query ?? "");
    if (operation === "mcp-entry") {
      const name = String(request.name ?? "");
      const value = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
      const server = value.mcpServers[name];
      if (!server) throw new Error(`${name || "This server"} is not configured. Refresh the page.`);
      return {name, text: JSON.stringify(server, null, 2)};
    }
    if (operation === "mcp-read") {
      const value = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
      return {text: JSON.stringify(value, null, 2), revision: revisionOf(value)};
    }
    return locked(async () => {
      if (["install", "update", "remove"].includes(operation)) {
        let source = request.source;
        if (typeof source !== "string" || source.length > 4096) throw new Error("Choose a package source.");
        if (operation !== "install") {
          const s = settings(), pm = manager(s);
          const existing = s.getPackages().find(p => sourceOf(p) === source || (isAbsolute(source) && pm.getInstalledPath(sourceOf(p), "user") === source));
          if (!existing) throw new Error("This package is not configured.");
          source = sourceOf(existing);
          if (operation === "update" && /^(git:|https?:|git@)/.test(source)) throw new Error("Update GitHub packages manually, then restart Zepi.");
        } else if (isAbsolute(source)) {
          const manifest = await readJson(join(source, "package.json"), {});
          if (!manifest.pi) throw new Error("The local folder must contain a pi package manifest.");
          source = resolve(source);
        } else source = `npm:${npmName(source)}`;
        const queue = await readJson(queuePath, []);
        if (!queue.some(item => item.source === source && item.operation === operation)) queue.push({operation, source});
        await atomic(queuePath, queue); await applyPending();
      } else if (operation === "retry") await applyPending();
      else if (operation === "cancel-pending") {
        const queue = await readJson(queuePath, []); await atomic(queuePath, queue.filter(item => item.source !== request.source));
      } else if (operation === "toggle-package") {
        if (typeof request.enabled !== "boolean") throw new Error("Choose enabled or disabled.");
        mutateSettings(c => {
          const i = (c.packages ?? []).findIndex(p => sourceOf(p) === request.source);
          if (i < 0) throw new Error("This package is not configured.");
          c.zepi ??= {}; c.zepi.disabledPackages ??= {};
          if (request.enabled) {
            if (request.source in c.zepi.disabledPackages) {c.packages[i] = c.zepi.disabledPackages[request.source]; delete c.zepi.disabledPackages[request.source];}
          } else if (!(request.source in c.zepi.disabledPackages)) {
            c.zepi.disabledPackages[request.source] = c.packages[i];
            c.packages[i] = {source: request.source, autoload: false};
          }
          return c;
        });
      } else if (operation === "toggle-resource") {
        const kind = request.kind;
        if (!["skills", "extensions"].includes(kind) || typeof request.enabled !== "boolean") throw new Error("Choose an installed resource.");
        const s = settings(), pm = manager(s), paths = await pm.resolve(async () => "skip");
        const resource = paths[kind].find(p => p.path === request.path && p.metadata.scope === "user");
        if (!resource) throw new Error("This resource is no longer installed.");
        mutateSettings(c => {
          if (resource.metadata.origin === "package") {
            const i = (c.packages ?? []).findIndex(p => sourceOf(p) === resource.metadata.source);
            if (i < 0 || c.zepi?.disabledPackages?.[resource.metadata.source]) throw new Error("Enable the owning package first.");
            const p = typeof c.packages[i] === "string" ? {source: c.packages[i]} : c.packages[i];
            const file = relative(resource.metadata.packageRoot, resource.path).split("\\").join("/");
            // Package lists may be include-allowlists (plain entries): keep those intact and
            // normalize only this file's +/-/! markers, so disabling one skill never enables the rest.
            const entries = (p[kind] ?? []).filter(v => v !== `+${file}` && v !== `-${file}` && v !== `!${file}`);
            p[kind] = [...entries, `${request.enabled ? "+" : "-"}${file}`];
            c.packages[i] = p;
          } else {
            const entries = (c[kind] ?? []).filter(v => ![request.path, `+${request.path}`, `-${request.path}`, `!${request.path}`].includes(v));
            c[kind] = request.enabled ? entries : [...entries, `-${request.path}`];
          }
          return c;
        });
      } else if (operation === "mcp-save") {
        const value = validateMcp(JSON.parse(request.text));
        const current = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
        if (request.revision !== undefined && request.revision !== revisionOf(current)) throw new Error("MCP configuration changed. Reopen the editor before saving again.");
        await atomic(join(directory, "mcp.json"), value);
      } else if (operation === "mcp-move") {
        const value = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
        const names = Object.keys(value.mcpServers);
        const index = names.indexOf(String(request.name ?? ""));
        if (index < 0) throw new Error("This MCP server is not configured.");
        const target = request.direction === "up" ? index - 1 : index + 1;
        if (target < 0 || target >= names.length) return;
        const ordered = [...names];
        [ordered[index], ordered[target]] = [ordered[target], ordered[index]];
        const servers = Object.create(null);
        for (const name of ordered) servers[name] = value.mcpServers[name];
        await atomic(join(directory, "mcp.json"), {...value, mcpServers: servers});
      } else if (["mcp-toggle", "mcp-remove", "mcp-signout"].includes(operation)) {
        const value = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
        if (!value.mcpServers[request.name]) throw new Error("This MCP server is not configured.");
        if (operation === "mcp-signout" || operation === "mcp-remove") {
          const server = value.mcpServers[request.name];
          if (server.url) {
            const store = credentials(directory);
            await store.forServer(request.name,server.url).withRefreshLock(async()=>store.remove(request.name,server.url));
          }
          if (operation === "mcp-remove") delete value.mcpServers[request.name];
        }
        else {if (typeof request.enabled !== "boolean") throw new Error("Choose enabled or disabled."); value.mcpServers[request.name].enabled = request.enabled;}
        await atomic(join(directory, "mcp.json"), value);
      } else if (operation === "mcp-set") {
        const name = String(request.name ?? "").trim();
        if (!/^[a-zA-Z0-9_-]{1,64}$/.test(name)) throw new Error("Keep the server name. Refresh the page.");
        let body;
        try { body = JSON.parse(String(request.text ?? "").trim()); }
        catch { throw new Error("Enter the server settings as a JSON object. Nothing was changed."); }
        if (!body || typeof body !== "object" || Array.isArray(body)) throw new Error("Enter the server settings as a JSON object. Nothing was changed.");
        const part = validateMcp({mcpServers: {[name]: body}});
        const current = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
        if (request.revision !== undefined && request.revision !== revisionOf(current)) throw new Error("MCP configuration changed. Reopen the page before saving again.");
        if (!current.mcpServers[name]) throw new Error(`${name} is no longer configured. Refresh the page.`);
        const servers = {};
        for (const [existing, config] of Object.entries(current.mcpServers)) {
          servers[existing] = existing === name ? part.mcpServers[name] : config;
        }
        await atomic(join(directory, "mcp.json"), {...current, mcpServers: servers});
      } else if (operation === "mcp-add") {
        // A pasted entry may arrive without the surrounding braces, with trailing commas
        // or with the comments people keep in mcp.json.
        const relax = text => {
          // Pass 1: drop // line comments, staying aware of quoted strings.
          let stripped = "", inString = false, escaped = false;
          for (let i = 0; i < text.length; i++) {
            const ch = text[i];
            if (inString) {
              stripped += ch;
              if (escaped) escaped = false;
              else if (ch === "\\") escaped = true;
              else if (ch === '"') inString = false;
              continue;
            }
            if (ch === '"') { inString = true; stripped += ch; continue; }
            if (ch === "/" && text[i + 1] === "/") { while (i < text.length && text[i] !== "\n") i++; stripped += "\n"; continue; }
            stripped += ch;
          }
          // Pass 2: drop trailing commas (comments are gone now), still string-aware.
          let out = ""; inString = false; escaped = false;
          for (let i = 0; i < stripped.length; i++) {
            const ch = stripped[i];
            if (inString) {
              out += ch;
              if (escaped) escaped = false;
              else if (ch === "\\") escaped = true;
              else if (ch === '"') inString = false;
              continue;
            }
            if (ch === '"') { inString = true; out += ch; continue; }
            if (ch === ",") {
              let j = i + 1;
              while (j < stripped.length && /\s/.test(stripped[j])) j++;
              // A fragment may end with a comma that only becomes trailing once braces wrap it.
              if (stripped[j] === undefined || stripped[j] === "}" || stripped[j] === "]") continue;
            }
            out += ch;
          }
          return out;
        };
        const parse = text => {
          const relaxed = relax(text);
          for (const candidate of [text, relaxed, `{${text}}`, `{${relaxed}}`]) {
            try { return JSON.parse(candidate); } catch {}
          }
          throw new Error("Enter MCP servers as a JSON object. Nothing was changed.");
        };
        const parsed = parse(String(request.text ?? "").trim());
        const part = validateMcp({mcpServers: parsed?.mcpServers ?? parsed});
        const names = Object.keys(part.mcpServers);
        if (!names.length) throw new Error("Enter MCP servers as a JSON object. Nothing was changed.");
        const current = validateMcp(await readJson(join(directory, "mcp.json"), {mcpServers: {}}));
        if (request.revision !== undefined && request.revision !== revisionOf(current)) throw new Error("MCP configuration changed. Reopen the page before saving again.");
        const clash = names.filter(name => current.mcpServers[name]);
        if (clash.length) throw new Error(`${clash.join(", ")} is already configured. Use the edit action to change it.`);
        // New servers land on top; the page shows mcp.json order. Keep top-level flags (autoEnableCodemode).
        await atomic(join(directory, "mcp.json"), {...current, mcpServers: {...part.mcpServers, ...current.mcpServers}});
      } else throw new Error("Unsupported extension operation.");
      return status();
    });
  }
  return {execute, async acquireRuntime() {
    return locked(async () => {
      await applyPending(); await mkdir(leases, {recursive: true, mode: 0o700});
      const path = join(leases, `${process.pid}.json`), id = crypto.randomUUID();
      await renewLease(path, id);
      const renewal = setInterval(() => { renewLease(path, id).catch(() => {}); }, LEASE_RENEWAL_MS);
      renewal.unref();
      return async () => {
        clearInterval(renewal);
        await rm(path, {force:true}); await rm(join(leases, `${process.pid}.mcp.json`), {force:true});
        // The last runtime to close is the moment queued package changes can be applied.
        await applyPending().catch(() => {});
      };
    }, true);
  }};
}

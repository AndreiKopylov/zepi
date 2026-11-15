// Smoke test for the native Rust/CDP browser (zepi_browser serve) over MCP stdio.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const exeName = process.platform === "win32" ? "zepi_browser.exe" : "zepi_browser";
const exe = [
  process.env.ZEPI_BROWSER_EXE,
  join(repoRoot, "..", "zepi-build", "release-fast", exeName),
  join(repoRoot, "..", "zepi-build", "debug", exeName),
  "C:/Llm/zepi/" + exeName,
].filter(Boolean).find((p) => existsSync(p));

function client(child) {
  const rl = createInterface({ input: child.stdout });
  const pending = new Map();
  let next = 1;
  rl.on("line", (line) => {
    if (!line.trim()) return;
    let msg; try { msg = JSON.parse(line); } catch { return; }
    if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
  });
  const send = (obj) => child.stdin.write(JSON.stringify(obj) + "\n");
  const request = (method, params, timeout = 30000) => new Promise((resolve, reject) => {
    const id = next++;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, timeout);
    pending.set(id, (msg) => { clearTimeout(timer); resolve(msg); });
    send({ jsonrpc: "2.0", id, method, params });
  });
  return { send, request };
}

test("rust browser MCP smoke: open/snapshot/act/screenshot/close", { timeout: 90000, skip: !exe && "zepi_browser binary not built" }, async () => {
  const child = spawn(exe, ["serve"], { stdio: ["pipe", "pipe", "ignore"] });
  const c = client(child);
  try {
    const init = await c.request("initialize", { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "smoke", version: "0" } });
    assert.ok(init.result.serverInfo);
    c.send({ jsonrpc: "2.0", method: "notifications/initialized" });

    const list = await c.request("tools/list");
    const names = list.result.tools.map((t) => t.name);
    for (const tool of ["browser_open", "browser_snapshot", "browser_act", "browser_screenshot", "browser_status", "browser_close"])
      assert.ok(names.includes(tool), `missing tool ${tool}`);

    const open = await c.request("tools/call", { name: "browser_open", arguments: { url: "https://example.com" } });
    assert.ok(!open.result.isError);
    const state = JSON.parse(open.result.content[0].text);
    assert.equal(state.title, "Example Domain");

    const snap = await c.request("tools/call", { name: "browser_snapshot", arguments: {} });
    assert.ok(!snap.result.isError);
    assert.ok(Array.isArray(JSON.parse(snap.result.content[0].text).actions));

    const act = await c.request("tools/call", { name: "browser_act", arguments: { kind: "press_key", key: "Tab" } });
    assert.ok(!act.result.isError);

    const shot = await c.request("tools/call", { name: "browser_screenshot", arguments: {} });
    assert.ok(!shot.result.isError);
    const path = JSON.parse(shot.result.content[0].text).path;
    assert.ok(existsSync(path), "screenshot file exists");
    assert.equal(readFileSync(path).subarray(0, 2).toString("hex"), "ffd8", "screenshot is a JPEG");

    const close = await c.request("tools/call", { name: "browser_close", arguments: {} });
    assert.ok(!close.result.isError);
  } finally {
    child.stdin.end();
    child.kill();
  }
});
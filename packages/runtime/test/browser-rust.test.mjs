// MCP-level smoke on a complex form using Playwright-style tool surface.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";

const repoRoot = fileURLToPath(new URL("../../..", import.meta.url));
const exeName = process.platform === "win32" ? "zepi_browser.exe" : "zepi_browser";
const exe = [
  process.env.ZEPI_BROWSER_EXE,
  join(repoRoot, "..", "zepi-build", "release-fast", exeName),
  "C:/Llm/zepi/" + exeName,
].filter(Boolean).find((p) => existsSync(p));
const formUrl = "file:///" + fileURLToPath(new URL("./fixtures/complex-form.html", import.meta.url)).replace(/\\/g, "/");

function client(child) {
  const rl = createInterface({ input: child.stdout });
  const pending = new Map(); let id = 0;
  rl.on("line", (l) => { const m = JSON.parse(l); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } });
  return {
    request(method, params) { const i = ++id; const p = new Promise((r) => pending.set(i, r)); child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: i, method, params }) + "\n"); return p; },
    send(o) { child.stdin.write(JSON.stringify(o) + "\n"); },
  };
}
const find = (s, role, name) => (s.elements || []).find((n) => n.role === role && (n.name || "").includes(name));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("rust browser MCP smoke (Playwright-style): navigate/snapshot/type/screenshot/close", { timeout: 90000, skip: !exe && "zepi_browser binary not built" }, async () => {
  const child = spawn(exe, ["serve"], { stdio: ["pipe", "pipe", "ignore"] });
  const c = client(child);
  const call = async (name, args) => { const r = await c.request("tools/call", { name, arguments: args }); assert.ok(r.result && !r.result.isError, `${name}: ${r.result?.content?.[0]?.text}`); return r.result; };
  try {
    const init = await c.request("initialize", { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "smoke", version: "0" } });
    assert.ok(init.result.serverInfo);
    c.send({ jsonrpc: "2.0", method: "notifications/initialized" });

    const list = await c.request("tools/list");
    const names = list.result.tools.map((t) => t.name);
    for (const tool of ["browser_navigate", "browser_snapshot", "browser_click", "browser_type", "browser_select_option", "browser_press_key", "browser_wait_for", "browser_take_screenshot", "browser_close"])
      assert.ok(names.includes(tool), `missing tool ${tool}`);

    const open = await call("browser_navigate", { url: formUrl });
    const state = JSON.parse(open.content[0].text);
    assert.equal(state.title, "Complex form");
    assert.ok(Array.isArray(state.elements), "interactive elements");

    const snap = await call("browser_snapshot", {});
    const s = JSON.parse(snap.content[0].text);
    const name = find(s, "textbox", "Name");
    assert.ok(name, "form field indexed");

    const typed = await call("browser_type", { node: name.index, text: "Alice" });
    const after = JSON.parse(typed.content[0].text);
    assert.equal(find(after, "textbox", "Name").value, "Alice");

    const shot = await call("browser_take_screenshot", {});
    const path = JSON.parse(shot.content[0].text).path;
    assert.ok(existsSync(path), "screenshot file exists");
    assert.equal(readFileSync(path).subarray(0, 2).toString("hex"), "ffd8", "screenshot is a JPEG");

    const close = await call("browser_close", {});
    assert.equal(JSON.parse(close.content[0].text).closed, true);
  } finally {
    await Promise.race([c.request("tools/call", { name: "browser_close", arguments: {} }).catch(() => {}), sleep(3000)]);
    child.stdin.end(); child.kill();
  }
});

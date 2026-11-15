// Complex-form regression smoke using Playwright-style tools:
// 3 tabs, varied inputs, long scrollable select, modal dialog.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
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
const value = (n) => (n && n.value !== undefined ? n.value : "");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("rust browser complex form (Playwright-style tools): tabs, inputs, selects, modal", { timeout: 120000, skip: !exe && "zepi_browser binary not built" }, async () => {
  const child = spawn(exe, ["serve"], { stdio: ["pipe", "pipe", "ignore"] });
  const c = client(child);
  const call = async (name, args) => { const r = await c.request("tools/call", { name, arguments: args }); assert.ok(r.result && !r.result.isError, `${name} ${JSON.stringify(args)}: ${r.result?.content?.[0]?.text}`); return JSON.parse(r.result.content[0].text); };
  const type = (n, t) => call("browser_type", { node: n, text: t });
  const click = (n) => call("browser_click", { node: n });
  const select = (n, v) => call("browser_select_option", { node: n, value: v });
  try {
    await c.request("initialize", { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "form", version: "0" } });
    c.send({ jsonrpc: "2.0", method: "notifications/initialized" });
    let s = await call("browser_navigate", { url: formUrl });

    for (const [label, text, role] of [["Name", "Alice", "textbox"], ["Email", "a@b.c", "textbox"], ["Secret", "pw", "textbox"], ["Age", "30", "spinbutton"], ["Notes", "note", "textbox"], ["Date", "2026-01-15", "textbox"], ["Time", "10:30", "textbox"]]) {
      s = await type(find(s, role, label).index, text);
      if (label !== "Secret") assert.equal(value(find(s, role, label)), text, label);
    }

    s = await click(find(s, "tab", "Choices").index);
    s = await select(find(s, "combobox", "Region").index, "b");
    assert.equal(value(find(s, "combobox", "Region")), "B");
    s = await click(find(s, "checkbox", "Agree").index);
    assert.equal(find(s, "checkbox", "Agree").checked, true);
    s = await click(find(s, "radio", "R2").index);
    assert.equal(find(s, "radio", "R2").checked, true);

    s = await select(find(s, "combobox", "City").index, "Moscow");
    assert.equal(value(find(s, "combobox", "City")), "Moscow");
    s = await select(find(s, "combobox", "Oblast").index, "Oblast-37");
    assert.equal(value(find(s, "combobox", "Oblast")), "Oblast-37");

    s = await click(find(s, "switch", "Notify").index);
    assert.equal(find(s, "switch", "Notify").checked, true);
    s = await type(find(s, "searchbox", "Search").index, "q");
    assert.equal(value(find(s, "searchbox", "Search")), "q");

    s = await click(find(s, "button", "Open modal").index);
    assert.ok(find(s, "textbox", "ModalName"), "dialog open");
    s = await type(find(s, "textbox", "ModalName").index, "mn");
    s = await select(find(s, "combobox", "ModalSel").index, "y");
    s = await click(find(s, "button", "Confirm").index);
    assert.ok(!find(s, "textbox", "ModalName"), "dialog closed");

    s = await click(find(s, "tab", "Extra").index);
    s = await type(find(s, "textbox", "Rich").index, "rich");
    s = await click(find(s, "button", "Submit").index);
    assert.ok(find(s, "button", "Done"), "submit ran");
  } finally {
    await Promise.race([c.request("tools/call", { name: "browser_close", arguments: {} }).catch(() => {}), sleep(3000)]);
    child.stdin.end(); child.kill();
  }
});

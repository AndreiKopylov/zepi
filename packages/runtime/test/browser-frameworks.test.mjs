// Framework + component-library regression smoke (Playwright-style tools):
// React/MUI, Vue/Quasar, Angular. Selects re-render on scroll (stale-node re-resolution).
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
const fixture = (n) => fileURLToPath(new URL(`./fixtures/${n}.html`, import.meta.url));

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
const regionSelected = (s) => (s.elements || []).some((n) =>
  (n.role === "option" && n.selected === true && (n.name || "").includes("Region-37")) ||
  ((n.role === "combobox" || n.role === "button") && (((n.value || "") + (n.name || "")).includes("Region-37"))));

for (const fw of ["react", "vue", "angular"]) {
  test(`rust browser ${fw} rich form (Playwright-style): fill, select, checkbox, submit`, { timeout: 120000, skip: !exe && "zepi_browser binary not built" }, async () => {
    const child = spawn(exe, ["serve"], { stdio: ["pipe", "pipe", "ignore"] });
    const c = client(child);
    const call = async (name, args) => { const r = await c.request("tools/call", { name, arguments: args }); assert.ok(r.result && !r.result.isError, `${name}: ${r.result?.content?.[0]?.text}`); return JSON.parse(r.result.content[0].text); };
    const type = (n, t) => call("browser_type", { node: n, text: t });
    const click = (n) => call("browser_click", { node: n });
    const select = (n, v) => call("browser_select_option", { node: n, value: v });
    const snap = () => call("browser_snapshot", {});
    try {
      await c.request("initialize", { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: fw, version: "0" } });
      c.send({ jsonrpc: "2.0", method: "notifications/initialized" });
      await call("browser_navigate", { url: "file:///" + fixture(fw).replace(/\\/g, "/") });

      let s = await snap();
      for (let i = 0; i < 40 && !find(s, "textbox", "Name"); i++) { await sleep(250); s = await snap(); }
      const nameBox = find(s, "textbox", "Name") || (s.elements || []).find((n) => n.role === "textbox");
      assert.ok(nameBox, `${fw} mounted`);
      s = await type(nameBox.index, "Alice");

      const choices = find(s, "tab", "Choices");
      if (choices) { s = await click(choices.index); await sleep(200); }

      const region = find(s, "combobox", "Region") || find(s, "button", "Region");
      assert.ok(region, `${fw} region control`);
      s = await select(region.index, "Region-37");
      assert.ok(regionSelected(s), `${fw} region selected`);

      const addr = find(s, "textbox", "Address") || find(s, "combobox", "Address");
      if (addr) {
        s = await select(addr.index, "Address-25");
        const av = find(s, "combobox", "Address") || find(s, "textbox", "Address");
        assert.ok((value(av) || (av && av.name) || "").includes("Address-25"), `${fw} address selected`);
      }

      const notify = find(s, "switch", "Notify") || find(s, "checkbox", "Notify");
      if (notify) { s = await click(notify.index); await sleep(200); const n2 = find(s, "switch", "Notify") || find(s, "checkbox", "Notify"); assert.equal(n2.checked, true, `${fw} toggle`); }
      const dateBox = find(s, "textbox", "Date");
      if (dateBox) { s = await type(dateBox.index, "2026-01-15"); assert.equal(value(find(s, "textbox", "Date")), "2026-01-15", `${fw} date`); }
      const country = find(s, "combobox", "Country");
      if (country) { s = await select(country.index, "us"); assert.equal(value(find(s, "combobox", "Country")), "US", `${fw} native select`); }

      const agree = find(s, "checkbox", "Agree");
      if (agree) { s = await click(agree.index); assert.equal(find(s, "checkbox", "Agree").checked, true); }
      const submit = find(s, "button", "Submit");
      if (submit) { s = await click(submit.index); assert.ok(find(s, "button", "Done"), `${fw} submit ran`); }
    } finally {
      await Promise.race([c.request("tools/call", { name: "browser_close", arguments: {} }).catch(() => {}), sleep(3000)]);
      child.stdin.end(); child.kill();
    }
  });
}

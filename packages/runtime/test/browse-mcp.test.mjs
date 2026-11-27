import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { ensureBrowseMcp, removeBrowseMcp, writeBrowseMcp } from "../src/browser/browse.mjs";

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), "zepi-browse-mcp-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await writeFile(join(dir, "cli.mjs"), "");
  return { dir, path: join(dir, "mcp.json") };
}

test("browse never rewrites an unreadable MCP configuration", async t => {
  const { dir, path } = await fixture(t);
  for (const original of ['{"mcpServers":{"corp":{"url":"http://localhost/mcp"},}}', '[]', '{"mcpServers":[]}']) {
    await writeFile(path, original);
    assert.equal(await ensureBrowseMcp(dir, dir, "plain"), false);
    await assert.rejects(writeBrowseMcp(dir, dir, "plain"), /mcp\.json.*not changed/);
    await assert.rejects(removeBrowseMcp(dir), /mcp\.json.*not changed/);
    assert.equal(await readFile(path, "utf8"), original);
  }
});

test("browse self-heal creates a missing config and preserves existing servers", async t => {
  const { dir, path } = await fixture(t);
  assert.equal(await ensureBrowseMcp(dir, dir, "plain"), true);
  const { mcpServers: { browse } } = JSON.parse(await readFile(path, "utf8"));
  assert.deepEqual(browse.args, [join(dir, "cli.mjs"), "mcp"]);
  assert.equal(await ensureBrowseMcp(dir, dir, "execute"), false);
  const config = { autoEnableCodemode: false, mcpServers: { corp: { url: "http://localhost/mcp" } } };
  await writeFile(path, JSON.stringify(config));
  assert.equal(await ensureBrowseMcp(dir, dir, "plain"), true);
  assert.deepEqual(JSON.parse(await readFile(path, "utf8")), { ...config, mcpServers: { ...config.mcpServers, browse } });
});

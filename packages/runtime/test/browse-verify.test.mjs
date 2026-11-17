import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { verifyDownload } from "../src/browser/browse.mjs";

const sri = data => `sha256-${createHash("sha256").update(data).digest("base64")}`;

test("verifyDownload passes a matching artifact and rejects and deletes a mismatched one", async () => {
  const dir = await mkdtemp(join(tmpdir(), "zepi-browse-verify-"));
  try {
    const file = join(dir, "relay.tgz");
    await writeFile(file, "artifact bytes");
    await verifyDownload("relay", file, sri("artifact bytes"));
    await writeFile(file, "tampered bytes");
    await assert.rejects(verifyDownload("relay", file, sri("artifact bytes")), /integrity/);
    assert.ok(!existsSync(file), "a failed verification must delete the downloaded file");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

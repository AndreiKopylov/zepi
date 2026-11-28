import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { generateCommitMessage } from "../src/commit-message.ts";
import { call, declaredTools, fixtureModel } from "./fixture-model.ts";

test("commit messages use the chosen model once without tools or chat history", async t => {
  const directory = await mkdtemp(join(tmpdir(), "zepi-commit-message-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const prompt = "diff --git a/main.rs b/main.rs\n+Добавить проверку";
  const fixture = await fixtureModel(directory, [(context, _signal, options) => {
    assert.match(options?.sessionId ?? "", /^[0-9a-f-]{36}$/);
    assert.equal(options?.maxTokens, 4096);
    assert.deepEqual(declaredTools(context), []);
    const system = context.messages.find(message => message.role === "system");
    assert.match(String(system?.content), /predominant language/);
    assert.match(String(system?.content), /explicit commit message instructions/);
    const messages = context.messages.filter(message => message.role !== "system");
    assert.equal(messages.length, 1);
    assert.equal(messages[0]?.role, "user");
    assert.equal(messages[0]?.content, prompt);
    return "  Add validation\n";
  }, () => call("write")]);
  const signal = new AbortController().signal;

  await assert.rejects(generateCommitMessage(fixture.runtime, "missing", "missing", prompt, signal), /default model/);
  assert.equal(fixture.requests(), 0);
  assert.equal(await generateCommitMessage(fixture.runtime, "zepi-fixture", "scripted", prompt, signal), "Add validation");
  assert.equal(fixture.requests(), 1);
  await assert.rejects(generateCommitMessage(fixture.runtime, "zepi-fixture", "scripted", prompt, signal), /did not finish/);
  assert.equal(fixture.requests(), 2);
});

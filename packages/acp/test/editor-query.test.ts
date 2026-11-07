import assert from "node:assert/strict";
import {test} from "node:test";
import {mkdtemp, realpath, mkdir, writeFile, symlink, rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {client, methods, type AgentContext} from "@agentclientprotocol/sdk";
import type {ToolInfo, ExtensionToolContext} from "@earendil-works/pi-coding-agent";
import {editorQueryTools, EDITOR_QUERY} from "../src/editor-query.ts";
import {resolveAgentTools} from "../src/agent-tools.ts";
import {startZepiAgent} from "../src/server.ts";
import {fixtureModel, call, lastToolText} from "./fixture-model.ts";

test("read roles use native queries and never reactivate pi builtin tools as plugins", () => {
  const info = (name: string, path: string, source: string) => ({name,sourceInfo:{path,source}} as ToolInfo);
  const tools = [info("find", "builtin:find", "builtin"), info("bash", "<inline:bash>", "inline"),
    info("hidden", "builtin:hidden", "builtin"), info("plugin", "/extensions/tool.js", "local"),
    ...["read", "grep", "ls", "write"].map(name => info(name, `<sdk:${name}>`, "sdk"))];
  assert.deepEqual(resolveAgentTools(["read"], tools), ["read", "grep", "ls"]);
  assert.deepEqual(resolveAgentTools(["extensions"], tools), ["plugin"]);
  assert.ok(!resolveAgentTools(undefined, tools).includes("find"));
  assert.throws(() => resolveAgentTools(["tool:hidden"], tools), /unavailable/);
});

test("query adapter rejects private paths and falls back to disk search outside the workspace", async () => {
  const dir = await realpath(await mkdtemp(join(tmpdir(), "zepi-query-path-")));
  const outside = await realpath(await mkdtemp(join(tmpdir(), "zepi-query-outside-")));
  let requests = 0;
  try {
    await mkdir(join(dir,".local"));
    await writeFile(join(outside,"current-notes.md"), "current marker line\n");
    await symlink(outside, join(dir,"outside"));
    const controller = new AbortController();
    const endpoint = {request: async (method: string, _args: unknown, options: {cancellationSignal: AbortSignal}) => {
      requests++; assert.equal(method, EDITOR_QUERY); assert.equal(options.cancellationSignal, controller.signal);
      controller.abort(); options.cancellationSignal.throwIfAborted();
    }} as unknown as Pick<AgentContext,"request">;
    const tools = editorQueryTools(dir,"s",endpoint);
    const grep = tools.find(tool => tool.name === "grep")!;
    const find = tools.find(tool => tool.name === "find")!;
    const textOf = (result: {content: Array<unknown>}) => result.content.map(part => typeof part === "object" && part !== null && "text" in part ? String((part as {text?: unknown}).text ?? "") : "").join("\n");
    const execute = (path: string, signal?: AbortSignal) => grep.execute("q", {pattern:"current",path},signal,undefined,{} as ExtensionToolContext);
    await assert.rejects(execute(join(dir,".local")), /Private/);
    assert.match(textOf(await execute(join(dir,"outside"))), /current marker line/);
    assert.match(textOf(await find.execute("q", {pattern:"*current*",path:join(dir,"outside")},undefined,undefined,{} as ExtensionToolContext)), /current-notes\.md/);
    assert.equal(requests,0);
    await assert.rejects(execute(dir,controller.signal), /abort/i);
    assert.equal(requests,1);
    await assert.rejects(execute(dir,controller.signal), /abort/i);
    assert.equal(requests,1);
  } finally {await rm(dir,{recursive:true,force:true});await rm(outside,{recursive:true,force:true});}
});

test("actual pi tools query ACP, preserve schemas and require native tool permission", {timeout:30_000}, async () => {
  const dir = await realpath(await mkdtemp(join(tmpdir(), "zepi-query-acp-")));
  await writeFile(join(dir,"settings.json"), JSON.stringify({defaultProvider:"zepi-fixture",defaultModel:"scripted",compaction:{enabled:false}}));
  const fixture = await fixtureModel(dir, [
    () => call("find", {pattern:"**/*.ts"}),
    context => {assert.match(lastToolText(context,"find"),/sample.ts/);return call("grep",{pattern:"unsaved",context:1,ignoreCase:true});},
    context => {assert.match(lastToolText(context,"grep"),/sample.ts:2: unsaved/);return call("ls",{limit:5});},
    context => {assert.match(lastToolText(context,"ls"),/sample.ts/);return call("grep",{pattern:"denied"});},
    context => {assert.throws(() => lastToolText(context,"grep"), /denied by user/i);return "Queries verified.";},
  ]);
  const toAgent = new TransformStream(), toClient = new TransformStream();
  const server = await startZepiAgent(dir,join(dir,"sessions"),{readable:toAgent.readable,writable:toClient.writable},fixture.runtime);
  const queries: Array<Record<string,unknown>> = [];
  let permissions = 0;
  const connection = client({name:"zepi-query-test"})
    .onNotification(methods.client.session.update,()=>{})
    .onRequest(methods.client.session.requestPermission, () => {
      permissions++;
      return {outcome:{outcome:"selected" as const,optionId: permissions === 4 ? "reject_once" : "allow_once"}};
    })
    .onRequest(EDITOR_QUERY, {parse: raw => raw as Record<string,unknown>}, ({params}) => {
      queries.push(params);
      assert.equal(params.path,dir);
      return {output:params.operation === "grep" ? "sample.ts:2: unsaved" : "sample.ts",truncated:false};
    })
    .connect({readable:toClient.readable,writable:toAgent.writable});
  try {
    await connection.agent.request(methods.agent.initialize,{protocolVersion:1,clientCapabilities:{fs:{readTextFile:true,writeTextFile:true}}});
    const session = await connection.agent.request(methods.agent.session.new,{cwd:dir,mcpServers:[]});
    await connection.agent.request(methods.agent.session.prompt,{sessionId:session.sessionId,prompt:[{type:"text",text:"Check native queries"}]});
    assert.equal(fixture.requests(),5);
    assert.deepEqual(queries.map(query=>query.operation),["find","grep","ls"]);
    assert.ok(queries.every(query=>query.sessionId === session.sessionId));
    assert.equal(queries[1]?.context,1);assert.equal(queries[1]?.ignoreCase,true);
    assert.equal(permissions,4);
  } finally {await server.agent.dispose(); connection.close();server.connection.close();await rm(dir,{recursive:true,force:true});}
});

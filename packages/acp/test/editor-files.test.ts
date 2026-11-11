import assert from "node:assert/strict";
import {test} from "node:test";
import {mkdtemp, realpath, readFile, writeFile, symlink, rm, access} from "node:fs/promises";
import {join} from "node:path";
import {tmpdir} from "node:os";
import {client, methods} from "@agentclientprotocol/sdk";
import {workspacePath, writableFile} from "../src/workspace-path.ts";
import {startZepiAgent} from "../src/server.ts";
import {editorTools} from "../src/editor-tools.ts";
import {fixtureModel, call, lastToolText} from "./fixture-model.ts";

test("new file resolution preserves workspace boundaries without creating directories", async () => {
  const dir = await realpath(await mkdtemp(join(tmpdir(),"zepi-new-path-")));
  try {
    assert.equal(await workspacePath(dir,"a/b/new.ts",true),join(dir,"a/b/new.ts"));
    await assert.rejects(access(join(dir,"a")));
    await assert.rejects(workspacePath(dir,"a/b/new.ts"),/ENOENT/);
    assert.equal((await writableFile(dir,"../escape.ts")).insideWorkspace,false);
    await assert.rejects(workspacePath(dir,".pi/new.ts",true),/Private/);
    await assert.rejects(writableFile(dir,"../.pi/secret.ts"),/Private/);
    await symlink(tmpdir(),join(dir,"outside"));
    assert.equal((await writableFile(dir,"outside/escape.ts")).insideWorkspace,false);
    await symlink(join(dir,"missing"),join(dir,"dangling"));
    await assert.rejects(workspacePath(dir,"dangling/new.ts",true),/ENOENT/);
  } finally {await rm(dir,{recursive:true,force:true});}
});

test("write outside the workspace goes straight to disk like pi", async () => {
  const dir = await realpath(await mkdtemp(join(tmpdir(),"zepi-write-outside-")));
  const target = join(dir,"..","zepi-write-outside-target.md");
  const client = {request: async () => {throw new Error("ACP write must not be used outside the workspace");}};
  const tools = editorTools(dir,"session",client as never);
  const write = tools.find(tool => tool.name === "write")!;
  try {
    await (write.execute as never as (...args: unknown[]) => Promise<unknown>)("call-1",{path:target,content:"external marker"},undefined,undefined,undefined);
    assert.equal(await readFile(target,"utf8"),"external marker");
  } finally {await rm(target,{force:true}); await rm(dir,{recursive:true,force:true});}
});
test("pi creates, reads and edits a new unsaved file through native ACP", {timeout:30_000}, async () => {
  const dir = await realpath(await mkdtemp(join(tmpdir(),"zepi-new-file-")));
  await writeFile(join(dir,"settings.json"),JSON.stringify({defaultProvider:"zepi-fixture",defaultModel:"scripted",compaction:{enabled:false}}));
  const path = join(dir,"nested/new.ts");
  const fixture = await fixtureModel(dir,[
    () => call("write",{path,content:"export const created = 1;\n"}),
    context => {lastToolText(context,"write");return call("read",{path});},
    context => {assert.match(lastToolText(context,"read"),/created = 1/);return call("edit",{path,edits:[{oldText:"created = 1",newText:"created = 2"}]});},
    context => {lastToolText(context,"edit");return "New file edited.";},
  ]);
  const toAgent = new TransformStream(), toClient = new TransformStream();
  const server = await startZepiAgent(dir,join(dir,"sessions"),{readable:toAgent.readable,writable:toClient.writable},fixture.runtime);
  let buffer: string | undefined;
  let writes = 0;
  const connection = client({name:"zepi-new-file-test"})
    .onNotification(methods.client.session.update,()=>{})
    .onRequest(methods.client.session.requestPermission,()=>({outcome:{outcome:"selected",optionId:"allow_once"}}))
    .onRequest(methods.client.fs.readTextFile,({params})=>{assert.equal(params.path,path);assert.ok(buffer);return {content:buffer};})
    .onRequest(methods.client.fs.writeTextFile,({params})=>{
      assert.equal(params.path,path);assert.equal(params._meta?.["zepi.dev/createFile"],true);
      writes++;buffer=params.content;return {};
    })
    .connect({readable:toClient.readable,writable:toAgent.writable});
  try {
    await connection.agent.request(methods.agent.initialize,{protocolVersion:1,clientCapabilities:{fs:{readTextFile:true,writeTextFile:true}}});
    const session = await connection.agent.request(methods.agent.session.new,{cwd:dir,mcpServers:[]});
    await connection.agent.request(methods.agent.session.prompt,{sessionId:session.sessionId,prompt:[{type:"text",text:"Create and edit a new file"}]});
    assert.equal(writes,2);assert.equal(buffer,"export const created = 2;\n");
    await assert.rejects(access(join(dir,"nested")));
    assert.equal(fixture.requests(),4);
  } finally {await server.agent.dispose();connection.close();server.connection.close();await rm(dir,{recursive:true,force:true});}
});

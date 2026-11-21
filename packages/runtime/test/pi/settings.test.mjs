import assert from "node:assert/strict";

// chmod is a no-op on Windows, so private-file modes cannot be asserted there.
async function assertPrivateMode(path) {
  if (process.platform === "win32") return;
  assert.equal((await stat(path)).mode & 0o777, 0o600);
}
import { test } from "node:test";
import { mkdtemp, mkdir, writeFile, readFile, rm, stat, readdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createPiSettings, checkPiUpdate, bundledVersion } from "../../src/pi/settings.mjs";

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), "zepi-pi-settings-"));
  t.after(() => rm(root, {recursive:true, force:true}));
  const target = join(root, "target"), source = join(root, "source");
  await mkdir(target); await mkdir(source);
  const put = (dir, name, value) => writeFile(join(dir, name), JSON.stringify(value));
  const get = (dir, name) => readFile(join(dir, name), "utf8").then(JSON.parse);
  const config = {providers:{"zepi-fixture":{api:"openai-completions",baseUrl:"https://example.invalid/v1",models:[{id:"test-model",name:"Test",reasoning:false,contextWindow:32000,maxTokens:2048}]}}};
  await put(target, "models.json", config);
  await put(target, "settings.json", {defaultProvider:"zepi-fixture",defaultModel:"test-model",defaultThinkingLevel:"off",customSetting:{retain:true}});
  return {target,source,put,get,config,bridge:createPiSettings(target,source)};
}

test("defaults roundtrip uses pi model validation and preserves unrelated settings", async t => {
  const f=await fixture(t);
  const result=await f.bridge.execute({operation:"defaults",provider:"zepi-fixture",model:"test-model",thinking:"off"});
  assert.equal(result.version,"1.6.0");
  assert.equal(result.defaultModel,"test-model");
  assert.deepEqual((await f.get(f.target,"settings.json")).customSetting,{retain:true});
  const before=await readFile(join(f.target,"settings.json"),"utf8");
  await assert.rejects(f.bridge.execute({operation:"defaults",provider:"zepi-fixture",model:"missing",thinking:"off"}));
  assert.equal(await readFile(join(f.target,"settings.json"),"utf8"),before);
});

test('model overrides use pi precedence and preserve other models and concurrent fields', async t => {
  const f = await fixture(t);
  const id = 'family/model.v1';
  f.config.providers['zepi-fixture'].models.push({id, name:'Thinking fixture', reasoning:true, contextWindow:64000, maxTokens:4096});
  await f.put(f.target, 'models.json', f.config);
  const original = await f.get(f.target, 'settings.json');
  original.compaction = {reserveTokens:2048, keepRecentTokens:8192,
    modelOverrides:{'other/provider-model':{reserveTokens:100}}};
  original.modelThinkingLevels = {'other/provider-model':'high'};
  await f.put(f.target, 'settings.json', original);
  const model = status => status.providers.find(p=>p.id==='zepi-fixture').models.find(m=>m.id===id);
  const initial = model(await f.bridge.status()).settings;
  assert.deepEqual(initial.values, {thinkingLevel:null, reserveTokens:null, keepRecentTokens:null});
  assert.deepEqual(initial.effective, {reserveTokens:2048, keepRecentTokens:8192});
  const update = (changes, expected) => f.bridge.execute({operation:'model-settings',provider:'zepi-fixture',model:id,changes,expected});
  const changed = {thinkingLevel:'high',reserveTokens:4096};
  let result = await update(changed, initial.values);
  assert.equal(model(result).settings.values.thinkingLevel,'high');
  assert.deepEqual(model(result).settings.effective, {reserveTokens:4096,keepRecentTokens:8192});
  let stored = await f.get(f.target,'settings.json');
  assert.equal(stored.compaction.modelOverrides['zepi-fixture/'+id].reserveTokens,4096);
  assert.equal(stored.compaction.modelOverrides['other/provider-model'].reserveTokens,100);
  assert.equal(stored.modelThinkingLevels['other/provider-model'],'high');
  assert.equal(stored.defaultThinkingLevel,'off');
  assert.equal(stored.customSetting.retain,true);
  const saved = await readFile(join(f.target,'settings.json'),'utf8');
  for (const changes of [{thinkingLevel:'not-a-level'},{reserveTokens:-1},{keepRecentTokens:0.1},{other:true}]) {
    await assert.rejects(update(changes,model(result).settings.values));
    assert.equal(await readFile(join(f.target,'settings.json'),'utf8'),saved);
  }
  await assert.rejects(update({reserveTokens:6000},initial.values),/changed while/);
  await assert.rejects(f.bridge.execute({operation:'model-settings',provider:'zepi-fixture',model:'test-model',changes:{thinkingLevel:'high'},expected:{thinkingLevel:null}}));
  // An unrelated field written since opening must survive a sparse save.
  stored.compaction.modelOverrides['zepi-fixture/'+id].keepRecentTokens = 9999;
  await f.put(f.target,'settings.json',stored);
  result = await update({reserveTokens:null,thinkingLevel:null},model(result).settings.values);
  assert.equal(model(result).settings.effective.keepRecentTokens,9999);
  await update({keepRecentTokens:null},model(result).settings.values);
  assert.deepEqual(await f.get(f.target,'settings.json'),original);
});

test('tool defaults preserve modifiers, explicit empty lists and concurrent edits', async t => {
  const f = await fixture(t);
  assert.equal((await f.bridge.status()).defaultTools, null);
  const tools = ['+codemode','-edit','+custom_tool','mcp__fixture__*'];
  assert.deepEqual((await f.bridge.execute({operation:'tool-defaults',tools,expected:null})).defaultTools,tools);
  assert.equal((await f.get(f.target,'settings.json')).customSetting.retain,true);
  await assert.rejects(f.bridge.execute({operation:'tool-defaults',tools:[],expected:null}),/changed while/);
  for (const invalid of [['bad name'],['+'],[''], 'read', Array(257).fill('read')]) {
    await assert.rejects(f.bridge.execute({operation:'tool-defaults',tools:invalid,expected:tools}));
    assert.deepEqual((await f.bridge.status()).defaultTools,tools);
  }
  assert.deepEqual((await f.bridge.execute({operation:'tool-defaults',tools:[],expected:tools})).defaultTools,[]);
  assert.equal((await f.bridge.execute({operation:'tool-defaults',tools:null,expected:[]})).defaultTools,null);
  assert.equal(Object.hasOwn(await f.get(f.target,'settings.json'),'defaultTools'),false);
});

test('HTTP proxy settings hide credentials, preserve unrelated fields and reject stale saves', async t => {
  const f = await fixture(t);
  const original = await f.get(f.target, 'settings.json');
  const initial = (await f.bridge.status()).httpProxy;
  assert.equal(initial.configured, false);
  const proxy = 'http://fixture-user:fixture-secret@127.0.0.1:19002';
  const saved = await f.bridge.execute({operation:'http-proxy', proxy, expected:initial.revision});
  assert.equal(saved.httpProxy.configured, true);
  assert.ok(!JSON.stringify(saved).includes('fixture-secret'));
  assert.ok(!JSON.stringify(saved).includes('fixture-user'));
  assert.deepEqual(await f.get(f.target,'settings.json'), {...original, httpProxy:proxy});
  await assert.rejects(f.bridge.execute({operation:'http-proxy', proxy:null, expected:initial.revision}), /changed while/);
  for (const invalid of ['', 10, 'socks5://localhost:8080', 'https://host.invalid/path', 'http://host.invalid/?secret=fixture-secret', 'malformed fixture-secret']) {
    await assert.rejects(f.bridge.execute({operation:'http-proxy',proxy:invalid,expected:saved.httpProxy.revision}), error => !error.message.includes('fixture-secret'));
    assert.equal((await f.get(f.target,'settings.json')).httpProxy,proxy);
  }
  const removed = await f.bridge.execute({operation:'http-proxy',proxy:null,expected:saved.httpProxy.revision});
  assert.equal(removed.httpProxy.configured,false);
  assert.deepEqual(await f.get(f.target,'settings.json'),original);
  await assertPrivateMode(join(f.target,'settings.json'));
});

test("auth responses contain metadata only; edits preserve other providers, OAuth and env mappings", async t => {
  const f=await fixture(t);
  const oauth={type:"oauth",access:"private-access-fixture",refresh:"private-refresh-fixture",expires:9999999999999};
  await f.put(f.target,"auth.json",{"zepi-fixture":{type:"api_key",key:"old-key-fixture",env:{FOO:"retain-env-fixture"}},anthropic:oauth});
  const result=await f.bridge.execute({operation:"key",provider:"zepi-fixture",key:"new-key-fixture"});
  for(const secret of ["new-key-fixture","private-access-fixture","private-refresh-fixture","retain-env-fixture"]) assert.ok(!JSON.stringify(result).includes(secret));
  const auth=await f.get(f.target,"auth.json");
  assert.deepEqual(auth.anthropic,oauth);
  assert.deepEqual(auth["zepi-fixture"],{type:"api_key",key:"new-key-fixture",env:{FOO:"retain-env-fixture"}});
  await assertPrivateMode(join(f.target,"auth.json"));
  await f.bridge.execute({operation:"remove-key",provider:"zepi-fixture"});
  assert.deepEqual(await f.get(f.target,"auth.json"),{anthropic:oauth});
});

test("custom model editing preserves pi advanced parameters and other models", async t => {
  const f=await fixture(t);
  f.config.providers["zepi-fixture"].headers={"X-Test":"retain"};
  await f.put(f.target,"models.json",f.config);
  await f.bridge.execute({operation:"custom-model",provider:"zepi-fixture",baseUrl:"https://example.invalid/v2",api:"openai-completions",model:"test-model",name:"Updated"});
  await f.bridge.execute({operation:"custom-model",provider:"zepi-fixture",baseUrl:"https://example.invalid/v2",api:"openai-completions",model:"second-model"});
  const provider=(await f.get(f.target,"models.json")).providers["zepi-fixture"];
  assert.equal(provider.models.length,2);
  assert.equal(provider.models[0].contextWindow,32000);
  assert.deepEqual(provider.headers,{"X-Test":"retain"});
  const before=await readFile(join(f.target,"models.json"),"utf8");
  await assert.rejects(f.bridge.execute({operation:"custom-model",provider:"zepi-fixture",baseUrl:"file:///tmp/test",api:"openai-completions",model:"bad"}));
  assert.equal(await readFile(join(f.target,"models.json"),"utf8"),before);
});

test("connections project metadata only and save keeps advanced model parameters", async t => {
  const f=await fixture(t);
  const advanced={id:"sr-one",name:"Existing name",reasoning:true,contextWindow:64000,maxTokens:4096,input:["text","image"]};
  f.config.providers.sr={api:"openai-completions",baseUrl:"https://example.invalid/v1",headers:{"x-fixture":"private-header"},models:[advanced,{id:"removed"}]};
  await f.put(f.target,"models.json",f.config);
  await f.put(f.target,"auth.json",{sr:{type:"api_key",key:"saved-private-key"}});
  const initial=await f.bridge.status();
  const sr=initial.customConnections.find(c=>c.id==="sr");
  assert.deepEqual(sr.models,[{id:"sr-one",name:"Existing name"},{id:"removed",name:"removed"}]);
  assert.equal(sr.credentialConfigured,true);
  assert.doesNotMatch(JSON.stringify(sr),/private|headers|input|reasoning/);
  const request={operation:"custom-connection",provider:"sr",baseUrl:"https://example.invalid/v2",api:"openai-completions",key:"",models:[" sr-one，new-one\nnew-two ","new-one"],expectedRevision:sr.revision};
  const result=await f.bridge.execute(request);
  const config=await f.get(f.target,"models.json");
  assert.deepEqual(config.providers.sr.models,[advanced,{id:"new-one",name:"new-one"},{id:"new-two",name:"new-two"}]);
  assert.deepEqual(config.providers["zepi-fixture"],f.config.providers["zepi-fixture"]);
  assert.deepEqual(config.providers.sr.headers,{"x-fixture":"private-header"});
  assert.equal((await f.get(f.target,"auth.json")).sr.key,"saved-private-key");
  assert.equal(result.defaultModel,"test-model");
  const saved=await readFile(join(f.target,"models.json"),"utf8");
  await assert.rejects(f.bridge.execute(request),/changed while/);
  assert.equal(await readFile(join(f.target,"models.json"),"utf8"),saved);
  const defaultConnection=result.customConnections.find(c=>c.id==="zepi-fixture");
  await assert.rejects(f.bridge.execute({...request,provider:"zepi-fixture",baseUrl:"https://example.invalid/v1",models:["other"],expectedRevision:defaultConnection.revision}),/global default/);
  assert.equal(await readFile(join(f.target,"models.json"),"utf8"),saved);
  await f.bridge.execute({...request,expectedRevision:result.customConnections.find(c=>c.id==="sr").revision,models:["sr-one","new-one","new-two"],key:"replacement-key"});
  assert.equal((await f.get(f.target,"auth.json")).sr.key,"replacement-key");
  assert.equal((await createPiSettings(f.target).status()).customConnections.find(c=>c.id==="sr").models.length,3);
});

test("enabled models write a pi allowlist and clear back to all", async t => {
  const f=await fixture(t);
  f.config.providers["zepi-fixture"].models.push({id:"other",name:"Other"});
  await f.put(f.target,"models.json",f.config);
  const enabled = (status) => status.providers.find(p=>p.id==="zepi-fixture").models.filter(m=>m.enabled).map(m=>m.id);
  assert.deepEqual(enabled(await f.bridge.status()),["test-model","other"]);
  let result=await f.bridge.execute({operation:"enabled-models",enabled:["zepi-fixture/other"]});
  assert.deepEqual(enabled(result),["other"]);
  assert.deepEqual(result.enabledModels,["zepi-fixture/other"]);
  assert.deepEqual((await f.get(f.target,"settings.json")).enabledModels,["zepi-fixture/other"]);
  const saved=await readFile(join(f.target,"settings.json"),"utf8");
  await assert.rejects(f.bridge.execute({operation:"enabled-models",enabled:["zepi-fixture/missing"]}));
  assert.equal(await readFile(join(f.target,"settings.json"),"utf8"),saved);
  result=await f.bridge.execute({operation:"enabled-models",all:true});
  assert.deepEqual(enabled(result),["test-model","other"]);
  assert.equal(Object.hasOwn(await f.get(f.target,"settings.json"),"enabledModels"),false);
});

test("removing a connection keeps the default provider and drops its key", async t => {
  const f=await fixture(t);
  f.config.providers.extra={api:"openai-completions",baseUrl:"https://example.invalid/v1",models:[{id:"extra-model"}]};
  await f.put(f.target,"models.json",f.config);
  await f.put(f.target,"auth.json",{extra:{type:"api_key",key:"extra-key"}});
  const initial=await f.bridge.status();
  const extra=initial.customConnections.find(c=>c.id==="extra");
  await assert.rejects(f.bridge.execute({operation:"remove-connection",provider:"zepi-fixture",expectedRevision:initial.customConnections.find(c=>c.id==="zepi-fixture").revision}),/global default/);
  await assert.rejects(f.bridge.execute({operation:"remove-connection",provider:"extra",expectedRevision:"stale"}),/changed while/);
  const result=await f.bridge.execute({operation:"remove-connection",provider:"extra",expectedRevision:extra.revision});
  assert.equal(result.customConnections.some(c=>c.id==="extra"),false);
  assert.equal((await f.get(f.target,"models.json")).providers.extra,undefined);
  assert.equal((await f.get(f.target,"auth.json")).extra,undefined);
});

test("connection model listing is read-only, bounded and never follows redirects", async t => {
  const {createServer}=await import("node:http");
  const {fetchConnectionModels,connectionEndpoint}=await import("../../src/pi/custom-models.mjs");
  const f=await fixture(t);
  let mode="success",redirected=0; const requests=[];
  const target=createServer((req,res)=>{redirected++;res.end("{}");});
  await new Promise(resolve=>target.listen(0,"127.0.0.1",resolve));
  const server=createServer((req,res)=>{
    requests.push({url:req.url,authorization:req.headers.authorization,key:req.headers["x-api-key"]});
    if(mode==="redirect"){res.writeHead(302,{Location:`http://127.0.0.1:${target.address().port}/stolen`});res.end();}
    else if(mode==="401"){res.writeHead(401);res.end("private-key");}
    else if(mode==="bad")res.end("{bad");
    else if(mode==="timeout")setTimeout(()=>res.end("{}"),100);
    else res.end(JSON.stringify({data:[{id:"a"},{id:"a"},{id:"b",name:"Second"}],has_more:mode==="page"}));
  });
  await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));
  t.after(()=>{server.closeAllConnections();target.closeAllConnections();server.close();target.close();});
  const baseUrl=`http://127.0.0.1:${server.address().port}/v1`;
  const before=await readFile(join(f.target,"models.json"),"utf8");
  const request={operation:"list-connection-models",provider:"sr",baseUrl,api:"openai-completions",key:"private-key"};
  assert.deepEqual(await f.bridge.execute(request),{candidates:[{id:"a"},{id:"b",name:"Second"}],incomplete:false});
  assert.deepEqual(requests[0],{url:"/v1/models",authorization:"Bearer private-key",key:undefined});
  mode="page"; assert.equal((await f.bridge.execute({...request,api:"anthropic-messages"})).incomplete,true);
  assert.equal(requests[1].key,"private-key");
  for (mode of ["401","bad","redirect"]) await assert.rejects(f.bridge.execute(request),error=>!error.message.includes("private-key"));
  mode="timeout"; await assert.rejects(fetchConnectionModels(connectionEndpoint(baseUrl,"openai-completions"),"openai-completions","private-key",undefined,undefined,15),/timed out/);
  assert.equal(redirected,0); assert.equal(await readFile(join(f.target,"models.json"),"utf8"),before);
  for(const url of ["ftp://example.org","https://u:p@example.org","https://example.org?key=x","https://example.org#x"]) await assert.rejects(f.bridge.execute({...request,baseUrl:url}));
});

test("import merges models and all auth without mutating the source or dropping target settings", async t => {
  const f=await fixture(t);
  const sourceModel={providers:{"zepi-fixture":{api:"openai-completions",baseUrl:"https://example.invalid/v1",models:[{id:"source-model",reasoning:false}]}}};
  const oauth={type:"oauth",access:"fixture-access",refresh:"fixture-refresh",expires:9999999999999};
  await f.put(f.source,"models.json",sourceModel);
  await f.put(f.source,"settings.json",{defaultProvider:"zepi-fixture",defaultModel:"source-model",defaultThinkingLevel:"off"});
  await f.put(f.source,"auth.json",{anthropic:oauth,"zepi-fixture":{type:"api_key",key:"ENV_FIXTURE"}});
  await f.put(f.target,"auth.json",{openai:{type:"api_key",key:"keep-key"}});
  const before=await Promise.all(["models.json","settings.json","auth.json"].map(n=>readFile(join(f.source,n),"utf8")));
  const result=await f.bridge.execute({operation:"import"});
  assert.equal(result.defaultModel,"source-model");
  assert.equal(result.providers.find(p=>p.id==="zepi-fixture").models.length,2);
  assert.deepEqual((await f.get(f.target,"auth.json")).anthropic,oauth);
  assert.equal((await f.get(f.target,"auth.json")).openai.key,"keep-key");
  assert.equal((await f.get(f.target,"settings.json")).customSetting.retain,true);
  assert.deepEqual(await Promise.all(["models.json","settings.json","auth.json"].map(n=>readFile(join(f.source,n),"utf8"))),before);
  assert.deepEqual((await readdir(f.source)).sort(),["auth.json","models.json","settings.json"]);
});

test("invalid import validates all inputs before changing destination files", async t => {
  const f=await fixture(t);
  await f.put(f.target,"auth.json",{openai:{type:"api_key",key:"keep-key"}});
  await f.put(f.source,"settings.json",{defaultProvider:"zepi-fixture",defaultModel:"missing"});
  const before=await Promise.all(["models.json","settings.json","auth.json"].map(n=>readFile(join(f.target,n),"utf8")));
  await assert.rejects(f.bridge.execute({operation:"import"}));
  assert.deepEqual(await Promise.all(["models.json","settings.json","auth.json"].map(n=>readFile(join(f.target,n),"utf8"))),before);
  assert.deepEqual(await readdir(f.source),["settings.json"]);
  await f.put(f.source,"settings.json",{});
  await writeFile(join(f.target,"settings.json"),"{invalid-json");
  const models=await readFile(join(f.target,"models.json"),"utf8");
  await assert.rejects(f.bridge.execute({operation:"import"}));
  assert.equal(await readFile(join(f.target,"models.json"),"utf8"),models);
});

test("update detection compares registry versions without installing packages", async () => {
  const response=version=>async (url,options)=>{
    assert.equal(url,"https://registry.npmjs.org/@earendil-works%2fpi-coding-agent/latest");
    assert.equal(options.redirect,"error");
    return {ok:true,json:async()=>({name:"@earendil-works/pi-coding-agent",version})};
  };
  assert.equal((await checkPiUpdate(response(bundledVersion))).status,"up_to_date");
  const nextVersion = bundledVersion.split('.').map((part, index) => index === 2 ? Number(part) + 1 : part).join('.');
  assert.equal((await checkPiUpdate(response(nextVersion))).status,"update_available");
  assert.equal((await checkPiUpdate(response("0.99.9"))).status,"ahead");
  const failed=await checkPiUpdate(async()=>{throw new Error("private-network-error")});
  assert.equal(failed.status,"error"); assert.ok(!JSON.stringify(failed).includes("private-network-error"));
});

test("browser settings are global and validated", async t => {
  const f = await fixture(t);
  const saved = await f.bridge.execute({operation:'browser-set', backend:'browse', browseTools:'execute', browseCli:'  C:\\tools\\browse\\  '});
  assert.equal(saved.browser.backend, 'browse');
  assert.equal(saved.browser.browseTools, 'execute');
  assert.equal(saved.browser.browseCli, 'C:\\tools\\browse\\');
  assert.equal(saved.browser.browseDir, join(f.target, 'browse'));
  await assert.rejects(f.bridge.execute({operation:'browser-set', backend:'chromium'}));
  assert.deepEqual((await f.get(f.target,'settings.json')).zepi.browser,
    {backend:'browse', browseTools:'execute', browseCli:'C:\\tools\\browse\\'});
  await assertPrivateMode(join(f.target,"settings.json"));
});

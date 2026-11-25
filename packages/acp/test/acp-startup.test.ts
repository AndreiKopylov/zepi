import assert from 'node:assert/strict';
import {test} from 'node:test';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {client,methods} from '@agentclientprotocol/sdk';
import {getCurrentSystemPrompt} from '@earendil-works/pi-ai';
import {startZepiAgent} from '../src/server.ts';
import {fixtureModel} from './fixture-model.ts';

test('requests arriving during Zepi startup wait for capability and session setup',{timeout:20000},async()=>{
  const dir=await mkdtemp(join(tmpdir(),'zepi-startup-'));
  await writeFile(join(dir,'settings.json'),JSON.stringify({defaultProvider:'zepi-fixture',defaultModel:'scripted'}));
  const zedConfigDir=join(dir,'custom editor profile','config');
  let promptChecked=false;
  const model=await fixtureModel(dir,[context=>{
    const system=getCurrentSystemPrompt(context.messages);
    assert.ok(system.includes(`${zedConfigDir} (settings.json, keymap.json)`));
    assert.ok(system.includes(`${dir} (settings.json, models.json, extensions/)`));
    assert.match(system,/Zepi uses Zed for the editor UI and pi for the coding agent/);
    promptChecked=true;
    return 'Configuration paths found.';
  }]);
  const toAgent=new TransformStream(),toClient=new TransformStream();
  const conn=client({name:'startup-client'}).connect({readable:toClient.readable,writable:toAgent.writable});
  // Queue the request before starting the server: an external client need not
  // know how long model loading and Zepi's history setup take.
  const initialized=conn.agent.request(methods.agent.initialize,{protocolVersion:1,clientCapabilities:{}});
  let server:Awaited<ReturnType<typeof startZepiAgent>>|undefined;
  const oldZedConfigDir=process.env.ZEPI_ZED_CONFIG_DIR;
  try {
    process.env.ZEPI_ZED_CONFIG_DIR=zedConfigDir;
    server=await startZepiAgent(dir,join(dir,'sessions'),{readable:toAgent.readable,writable:toClient.writable},model.runtime);
    const result=await initialized;
    assert.equal(result.agentCapabilities?.promptCapabilities?.embeddedContext,true);
    const task=await conn.agent.request(methods.agent.session.new,{cwd:dir,mcpServers:[]});
    assert.ok(task.sessionId);
    await conn.agent.request(methods.agent.session.prompt,{sessionId:task.sessionId,prompt:[{type:'text',text:'Where is Zepi configured?'}]});
    assert.ok(promptChecked,'the model saw the Zepi configuration paths');
  }finally{
    if (oldZedConfigDir===undefined) delete process.env.ZEPI_ZED_CONFIG_DIR;
    else process.env.ZEPI_ZED_CONFIG_DIR=oldZedConfigDir;
    await server?.agent.dispose();conn.close();server?.connection.close();await rm(dir,{recursive:true,force:true});
  }
});

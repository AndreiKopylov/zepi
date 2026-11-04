import {createHash} from 'node:crypto';
import {existsSync} from 'node:fs';
import {lstat, mkdir, readFile, readdir, writeFile} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {methods, type AgentContext} from '@agentclientprotocol/sdk';
import {defineTool} from '@earendil-works/pi-coding-agent';
import {Type} from 'typebox';
import {createPreview} from './preview.ts';
import {EDITOR_QUERY} from './editor-query.ts';
import {workspacePath} from './workspace-path.ts';

const hash=(text:string|Buffer)=>createHash('sha256').update(text).digest('hex');

// Windows has no /bin/bash: prefer Git Bash to keep POSIX habits, fall back to cmd.exe.
const winShell=():{command:string;args:(command:string)=>string[]}=>{
  const git=(process.env.PATH??'').split(';').map(dir=>join(dir,'git.exe')).find(existsSync);
  const candidates:string[]=[];
  for (let base=git?dirname(git):''; base&&candidates.length<6; base=dirname(base)) {
    candidates.push(join(base,'bin','bash.exe'),join(base,'usr','bin','bash.exe'));
  }
  candidates.push('C:/Program Files/Git/bin/bash.exe','C:/Program Files (x86)/Git/bin/bash.exe');
  const bash=candidates.find(path=>existsSync(path));
  return bash?{command:bash,args:(c:string)=>['--noprofile','--norc','-c',c]}:{command:'cmd.exe',args:(c:string)=>['/c',c]};
};
const shellSpec=process.platform==='win32'?winShell():{command:'/bin/bash',args:(c:string)=>['--noprofile','--norc','-c',c]};
type Snapshot={content:string;disk:string|null;buffer:string|null};
type Change={path:string;kind:'modified'|'created'|'deleted'|'unsupported';status:'applied'|'review'|'preserved';reason?:string;artifact?:string;reviewId?:string};

/** Commands share preview's native terminal, process supervisor and cleanup.
 * The copy is an execution directory, not a security sandbox. Only conditional
 * native writes can merge its text output back into the user's live buffers.
 */
export function createShell(cwd:string, storage:string, sessionId:string, client:AgentContext) {
  let capturing=false;
  const inputs=new Map<string,Snapshot>();
  const request:AgentContext['request']=async (method:any,params:any,options:any)=>{
    const result:any=await client.request(method,params,options);
    if(method==='_zepi/fs/snapshot' && capturing) {
      inputs.set(params.path,result);
    }
    if(method===methods.client.terminal.create) capturing=false;
    return result;
  };
  const forwarded=new Proxy(client,{get(target,key){
    if(key==='request') return request;
    const value=Reflect.get(target,key,target);
    return typeof value==='function'?value.bind(target):value;
  }});
  const runner=createPreview(cwd,storage,sessionId,forwarded,'shell');
  const tool=defineTool({
    name:'bash',label:'Shell',defaultActive:false,executionMode:'sequential',
    description:'Execute a bash command in a captured workspace including current unsaved editor contents and binary files. Supports pipes, redirects and shell syntax. Optional files selects inputs (max 512, 16 MiB); otherwise captures all visible workspace files and fails if incomplete. Optional dependencies copies installed dependency/asset directories (max 256 MiB). This is a local process, not an OS sandbox; use relative paths inside the capture. No packages are installed automatically. Changed, new and deleted files stay inside that copy: nothing is written back to the project, so results that must persist are produced with the write or edit tool, not by a command. Commands and descendants stop on completion, failure, timeout and cancellation. For long-running browser verification use preview with a server. Timeout defaults to 60 seconds, max 300.',
    promptSnippet:'Run shell commands on current editor contents and review resulting changes',
    parameters:Type.Object({command:Type.String({minLength:1,maxLength:16384}),timeout:Type.Optional(Type.Integer({minimum:1,maximum:300})),
      files:Type.Optional(Type.Array(Type.String(),{maxItems:512})),dependencies:Type.Optional(Type.Array(Type.String(),{maxItems:16}))}),
    async execute(toolCallId,params,signal,onUpdate,context) {
      signal?.throwIfAborted();
      let files=params.files;
      if(!files) {
        const result=await client.request<{output:string;truncated:boolean}>(EDITOR_QUERY,
          {sessionId,operation:'find',path:cwd,pattern:'**/*',limit:512},{cancellationSignal:signal});
        if(result.truncated) throw new Error('Workspace capture exceeds 512 files. Select the complete files and dependencies needed by this command.');
        files=result.output==='No files found matching pattern'?[]:result.output.split('\n').filter(Boolean);
      }
      const changes:Change[]=[];
      const previousRun=runner.snapshot();
      inputs.clear();capturing=true;
      try {
        const outcome=await runner.tool.execute(toolCallId,{action:'start',files,dependencies:params.dependencies,
          commands:[{command:shellSpec.command,args:shellSpec.args(params.command),timeoutSeconds:params.timeout??60}]},signal,onUpdate,context);
        capturing=false;
        const run=runner.snapshot();
        if(!run) return outcome;
        const result=JSON.parse(outcome.content.filter(item=>item.type==='text').map(item=>item.text).join('\n'));
        const root=join(run.directory,'files');
        const dependencies=run.dependencies.roots;
        const outputs=new Map<string,Buffer>();
        const unsupported=new Map<string,string>();
        let bytes=0, entries=0;
        async function visit(path:string) {
          for(const entry of await readdir(join(root,path),{withFileTypes:true})) {
            const local=path?`${path}/${entry.name}`:entry.name;
            if(dependencies.some(dep=>local===dep || local.startsWith(`${dep}/`))) continue;
            if(++entries>4096) throw new Error('Command output exceeds 4096 entries; it remains in the captured directory.');
            if(entry.isSymbolicLink()) {unsupported.set(local,'Symbolic links are not merged into editor buffers.');continue;}
            if(entry.isDirectory()) {await visit(local);continue;}
            if(!entry.isFile()) {unsupported.set(local,'Non-regular output is not merged.');continue;}
            const info=await lstat(join(root,local));
            if(!info.isFile() || info.isSymbolicLink()) {unsupported.set(local,'Output changed type during inspection.');continue;}
            bytes+=info.size;
            if(info.size>1024*1024 || bytes>16*1024*1024) {unsupported.set(local,'Output exceeds the text review size limit.');continue;}
            const content=await readFile(join(root,local));
            outputs.set(local,content);
          }
        }
        await visit('');
        const originals=new Map(run.files.map(file=>[file.path,file]));
        const paths=new Set([...originals.keys(),...outputs.keys(),...unsupported.keys()]);
        for(const path of paths) {
          const before=originals.get(path), after=outputs.get(path);
          if(after && before?.hash===hash(after)) continue;
          const kind=unsupported.has(path)?'unsupported':!after?'deleted':before?'modified':'created';
          const change:Change={path,kind,status:'preserved'};changes.push(change);
          // Save a recoverable artifact before attempting any native mutation.
          if(after) {
            const artifact=join(run.directory,'outputs',path);
            await mkdir(join(artifact,'..'),{recursive:true,mode:0o700});
            await writeFile(artifact,after,{mode:0o600});change.artifact=artifact;
          }
          if(unsupported.has(path)) {change.reason=unsupported.get(path);continue;}
          if(signal?.aborted || result.processCleanup!=='confirmed') {change.reason='Command cancellation or cleanup was not confirmed; output was not applied.';continue;}
          try {
            const canonical=await workspacePath(cwd,path,true);
            const snapshot=inputs.get(canonical);
            if(before && (!snapshot || hash(Buffer.from(snapshot.content,'base64'))!==before.hash)) throw new Error('Captured editor version could not be verified.');
            const content=after?.toString('utf8');
            const binary=after && (after.includes(0) || !Buffer.from(content!).equals(after));
            if(!after || binary || before && snapshot?.buffer===null) {
              // Nothing is written back to the project: binary and deleted outputs stay in the copy.
              change.reason='Binary or deleted output stays in the captured copy; write it with the write tool if it must persist.';
              change.status='preserved';
              continue;
            }
            const expected=snapshot?.buffer;
            if(before && typeof expected!=='string') throw new Error('Captured editor version could not be verified.');
            await client.request(methods.client.fs.writeTextFile,{sessionId,path:canonical,content,
              _meta:before?{'zepi.dev/expectedBuffer':expected!}:{'zepi.dev/createFile':true}},
              {cancellationSignal:signal});
            change.status='applied';
          } catch(error) {change.reason=error instanceof Error?error.message:String(error);}
        }
        const recovery=changes.some(change=>change.status==='preserved')?await runner.preserveInputs():undefined;
        const value={runId:run.runId,command:params.command,checks:result.project?.checks??[],recovery,
          cancelled:!!signal?.aborted,processCleanup:result.processCleanup,changes,
          includesUnsavedBuffers:true,files:run.files.map(file=>file.path),dependencies,
          scope:'Captured files and copied dependencies. System tools and absolute paths run locally outside this copy.',
          error:result.error??result.project?.error};
        await writeFile(join(run.directory,'shell-result.json'),JSON.stringify(value,null,2),{mode:0o600});
        return {content:[{type:'text' as const,text:JSON.stringify(value,null,2)}],details:{zepiShell:{version:1,...value}},
          ...((outcome as {isError?:boolean}).isError || changes.some(change=>change.status==='preserved')?{isError:true}:{})};
      } catch(error) {
        capturing=false;
        await runner.stop();
        const run=runner.snapshot();
        if(run?.stopped && run!==previousRun) {
          const recovery=await runner.preserveInputs();
          throw new Error(`${error instanceof Error?error.message:String(error)} Command output is preserved at ${recovery}.`);
        }
        throw error;
      } finally {capturing=false;await runner.finishTurn(true);}
    },
  });
  return {tool,extension:runner.extension,finishTurn:()=>runner.finishTurn(true)};
}

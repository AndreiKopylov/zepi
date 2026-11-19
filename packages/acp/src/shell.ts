import {createHash, randomUUID} from 'node:crypto';
import {existsSync} from 'node:fs';
import {copyFile, lstat, mkdir, readFile, readdir, rename, rm, writeFile} from 'node:fs/promises';
import {dirname, join, relative, resolve, sep} from 'node:path';
import {methods, type AgentContext} from '@agentclientprotocol/sdk';
import {defineTool} from '@earendil-works/pi-coding-agent';
import {Type} from 'typebox';
import {createCapture} from './capture.ts';
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
  // Nothing on Windows reaps a background job whose parent shell already exited, so bash kills its own on EXIT.
  const reaped=(c:string)=>`trap 'kill -9 $(jobs -p) 2>/dev/null; wait 2>/dev/null' EXIT; ${c}`;
  return bash?{command:bash,args:(c:string)=>['--noprofile','--norc','-c',reaped(c)]}:{command:'cmd.exe',args:(c:string)=>['/c',c]};
};
const bashSpec=process.platform==='win32'?winShell():{command:'/bin/bash',args:(c:string)=>['--noprofile','--norc','-c',c]};
// Same capture workflow for PowerShell: scripts run against the captured copies.
const pwshSpec=():{command:string;args:(command:string)=>string[]}=>{
  const pwsh=(process.env.PATH??'').split(';').map(dir=>join(dir,'pwsh.exe')).find(existsSync);
  return {command:pwsh??'powershell.exe',args:(c:string)=>['-NoProfile','-NonInteractive','-Command',c]};
};
type Snapshot={content:string;disk:string|null;buffer:string|null};
// Office/image binaries get a pre-command copy even when not listed, so Reject can restore them.
const BINARY_INPUT_EXTENSIONS=new Set(['xlsx','xlsm','xls','pptx','pptm','ppt','docx','docm','doc','pdf','png','jpg','jpeg','gif','webp','bmp']);
type Change={path:string;kind:'modified'|'created'|'deleted'|'unsupported';status:'applied'|'review'|'preserved';reason?:string;artifact?:string;reviewId?:string;binary?:boolean;backup?:string};

// Metadata snapshot of the real workspace: detects writes that bypass the captured copy
// (Excel COM SaveAs, absolute paths). Content is never read here, only stat metadata.
async function scanWorkspace(rootDir:string) {
  const files:string[]=[];
  let overflow=false;
  const walk=async (local:string) => {
    for(const item of await readdir(join(rootDir,local),{withFileTypes:true}).catch(()=>[])) {
      if(overflow) return;
      if(item.isDirectory()) { if(item.name==='.git'||item.name==='node_modules') continue; await walk(local?`${local}/${item.name}`:item.name); continue; }
      if(!item.isFile()) continue;
      if(files.length>=4096) {overflow=true;return;}
      files.push(local?`${local}/${item.name}`:item.name);
    }
  };
  await walk('');
  if(overflow) return undefined;
  const entries=new Map<string,{mtimeMs:number;size:number}>();
  // Batched lstat: sequential stats add seconds per command on Windows with antivirus.
  for(let i=0;i<files.length;i+=128) {
    await Promise.all(files.slice(i,i+128).map(async relative=>{
      const info=await lstat(join(rootDir,relative)).catch(()=>undefined);
      if(info?.isFile()&&!info.isSymbolicLink()) entries.set(relative,{mtimeMs:info.mtimeMs,size:info.size});
    }));
  }
  return entries;
}

/** Commands use the captured-workspace native terminal, process supervisor and cleanup.
 * The copy is an execution directory, not a security sandbox. Text output is merged back into
 * live editor buffers (keep/reject review); binary output is written to the project directly
 * with a backup, because binary files have no editor buffer to review.
 */
export function createShell(cwd:string, storage:string, sessionId:string, client:AgentContext, kind:'bash'|'powershell' = 'bash',
  onBinaryApply?:(change:{path:string;kind:'modified'|'created';backup?:string})=>void,
  onTextApply?:(change:{path:string;base:string;current:string;mtimeNs:string|null;created?:boolean})=>void) {
  const shell = kind === 'powershell' ? pwshSpec() : bashSpec;
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
  const runner=createCapture(cwd,storage,sessionId,forwarded);
  const tool=defineTool({
    name:kind,label:kind==='powershell'?'PowerShell':'Shell',defaultActive:false,executionMode:'sequential',
    description: kind==='powershell'
      ? 'Execute a PowerShell script against a captured copy of current editor contents (including unsaved files). PowerShell syntax is native (no shims, no bash). Optional files selects inputs (an empty list captures all); otherwise captures all visible workspace files. List every workspace file the script writes through absolute paths so its pre-command state is captured and Reject can restore it. Optional dependencies copies installed directories. Changed text files are merged back for review; binary changes (xlsx, images) are written to the project with a backup. Commands and descendants stop on completion, failure, timeout and cancellation. Timeout defaults to 60s, max 300s.'
      : 'Execute a bash command in a captured workspace including current unsaved editor contents and binary files. Supports pipes, redirects and shell syntax. Optional files selects inputs (max 512, 32 MiB; an empty list captures all); otherwise captures all visible workspace files and fails if incomplete. List every workspace file the command writes through absolute paths so its pre-command state is captured and Reject can restore it. Optional dependencies copies installed dependency/asset directories (max 256 MiB). This is a local process, not an OS sandbox; use relative paths inside the capture. No packages are installed automatically. Changed text files are merged back for review; binary changes (xlsx, images) are written to the project with a backup. Deleted files stay in the copy. Commands and descendants stop on completion, failure, timeout and cancellation. Timeout defaults to 60 seconds, max 300.',
    promptSnippet: kind==='powershell' ? 'Run PowerShell against current editor contents and review changes' : 'Run shell commands on current editor contents and review resulting changes',
    parameters:Type.Object({command:Type.String({minLength:1,maxLength:16384}),timeout:Type.Optional(Type.Integer({minimum:1,maximum:300})),
      files:Type.Optional(Type.Array(Type.String(),{maxItems:512})),dependencies:Type.Optional(Type.Array(Type.String(),{maxItems:16}))}),
    async execute(toolCallId,params,signal,onUpdate,context) {
      signal?.throwIfAborted();
      let files=params.files;
      if(!files || files.length===0) {
        const result=await client.request<{output:string;truncated:boolean}>(EDITOR_QUERY,
          {sessionId,operation:'find',path:cwd,pattern:'**/*',limit:512},{cancellationSignal:signal});
        if(result.truncated) throw new Error('Workspace capture exceeds 512 files. Select the complete files and dependencies needed by this command.');
        files=result.output==='No files found matching pattern'?[]:result.output.split('\n').filter(Boolean);
      }
      const changes:Change[]=[];
      const outside:{path:string;kind:'modified'|'created';note:string}[]=[];
      const previousRun=runner.snapshot();
      const workspaceBefore=await scanWorkspace(cwd);
      // Outside-capture writes need a pre-command copy for Reject: capture workspace binaries too.
      if(workspaceBefore) {
        const chosen=new Set(files.map(file=>relative(cwd,resolve(cwd,file)).split(sep).join('/')));
        let budget=24*1024*1024;
        for(const [local,meta] of workspaceBefore) {
          if(files.length>=512) break;
          const dot=local.lastIndexOf('.');
          if(dot<0 || !BINARY_INPUT_EXTENSIONS.has(local.slice(dot+1).toLowerCase()) || chosen.has(local) || meta.size>budget) continue;
          budget-=meta.size;chosen.add(local);files.push(local);
        }
      }
      inputs.clear();capturing=true;
      try {
        const outcome=await runner.tool.execute(toolCallId,{action:'start',files,dependencies:params.dependencies,
          commands:[{command:shell.command,args:shell.args(params.command),timeoutSeconds:params.timeout??60}]},signal,onUpdate,context);
        capturing=false;
        const run=runner.snapshot();
        if(!run) return outcome;
        const result=JSON.parse(outcome.content.filter(item=>item.type==='text').map(item=>item.text).join('\n'));
        const root=join(run.directory,'files');
        const dependencies=run.dependencies.roots;
        const outputs=new Map<string,Buffer>();
        const sizes=new Map<string,number>();
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
            // Binary outputs are copied as-is (xlsx, images); text is merged through buffers.
            if(info.size>16*1024*1024) {unsupported.set(local,'Output exceeds 16 MiB.');continue;}
            bytes+=info.size;
            if(bytes>32*1024*1024) {unsupported.set(local,'Output exceeds the 32 MiB review budget.');continue;}
            const content=await readFile(join(root,local));
            outputs.set(local,content);
            sizes.set(local,info.size);
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
            const binary=!!after && (after.includes(0) || !Buffer.from(content!).equals(after));
            if(!after) {
              // Deleted files stay in the copy: removing a project file needs an explicit request.
              change.reason='Deleted output stays in the captured copy; delete the file explicitly if it must go.';
              change.status='preserved';
              continue;
            }
            if(binary) {
              // No editor buffer exists for binary files, so keep/reject cannot cover them.
              // Apply directly with a backup the user can restore.
              const backup=join(run.directory,'backup',path);
              await mkdir(dirname(backup),{recursive:true,mode:0o700});
              if(existsSync(canonical)) {await copyFile(canonical,backup);change.backup=backup;}
              const temp=`${canonical}.zepi-${randomUUID()}.tmp`;
              try {
                await writeFile(temp,after,{mode:0o600});
                await rename(temp,canonical);
              } catch (error) {
                await rm(temp,{force:true}).catch(()=>{});
                throw error;
              }
              change.status='applied';change.binary=true;
              continue;
            }
            if((sizes.get(path)??0)>1024*1024) {
              change.reason='Text output exceeds the 1 MiB review limit; it remains in the captured copy.';
              change.status='preserved';
              continue;
            }
            const expected=snapshot?.buffer;
            if(before && typeof expected!=='string') throw new Error('Captured editor version could not be verified.');
            let textReview:{path:string;base:string;current:string;mtimeNs:string|null;created?:boolean}|undefined;
            if(change.artifact && typeof content==='string') {
              // Created files live only in an editor buffer, so their text is journaled too.
              const mtime=before?await lstat(canonical,{bigint:true}).then(info=>String(info.mtimeNs)).catch(()=>null):null;
              textReview={path,base:before?expected!:'' ,current:content,mtimeNs:mtime,...(before?{}:{created:true})};
            }
            await client.request(methods.client.fs.writeTextFile,{sessionId,path:canonical,content,
              _meta:before?{'zepi.dev/expectedBuffer':expected!}:{'zepi.dev/createFile':true}},
              {cancellationSignal:signal});
            change.status='applied';
            if(textReview) onTextApply?.(textReview);
          } catch(error) {change.reason=error instanceof Error?error.message:String(error);}
        }
        // Writes that bypassed the captured copy (Excel COM, absolute paths) are detected
        // by metadata diff of the real workspace; created binaries are reviewable as-is.
        if(!signal?.aborted && workspaceBefore) {
          const after=await scanWorkspace(cwd);
          if(after) {
            let budget=32*1024*1024;
            for(const [local,meta] of after) {
              if(signal?.aborted) break;
              const prior=workspaceBefore.get(local);
              if(prior && prior.mtimeMs===meta.mtimeMs && prior.size===meta.size) continue;
              if(dependencies.some(dep=>local===dep || local.startsWith(`${dep}/`))) continue;
              if(changes.some(change=>change.path===local)) continue;
              const info=await lstat(join(cwd,local)).catch(()=>undefined);
              if(!info || !info.isFile() || info.isSymbolicLink()) continue;
              if(info.size>16*1024*1024 || info.size>budget) {outside.push({path:local,kind:prior?'modified':'created',note:'Outside-capture output exceeds the size limit.'});continue;}
              budget-=info.size;
              try {
                const content=await readFile(join(cwd,local));
                const decoded=content.toString('utf8');
                if(!(content.includes(0) || !Buffer.from(decoded).equals(content))) {outside.push({path:local,kind:prior?'modified':'created',note:'Text file changed outside the captured copy; reload it in the editor to review.'});continue;}
                const change:Change={path:local,kind:prior?'modified':'created',status:'applied',binary:true};
                // Only captured inputs have a pre-command copy to back up from.
                if(prior && existsSync(join(root,local))) {
                  const backup=join(run.directory,'backup',local);
                  await mkdir(dirname(backup),{recursive:true,mode:0o700});
                  await copyFile(join(root,local),backup);
                  change.backup=backup;
                }
                changes.push(change);
              } catch(error) {
                outside.push({path:local,kind:prior?'modified':'created',note:`Outside-capture output could not be inspected: ${error instanceof Error?error.message:String(error)}`});
              }
            }
          }
        }
        for(const change of changes) if(change.binary&&change.status==='applied'&&change.kind!=='deleted'&&change.kind!=='unsupported')
          onBinaryApply?.({path:change.path,kind:change.kind,backup:change.backup});
        const recovery=changes.some(change=>change.status==='preserved')?await runner.preserveInputs():undefined;
        const value={runId:run.runId,command:params.command,checks:result.project?.checks??[],recovery,
          cancelled:!!signal?.aborted,processCleanup:result.processCleanup,changes,
          includesUnsavedBuffers:true,files:run.files.map(file=>file.path),dependencies,outside,
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

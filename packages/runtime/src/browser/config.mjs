import { readFile } from "node:fs/promises";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { browserDirectory } from "../paths.mjs";
import { ReadOnlyAuthStorage } from "../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";
import {ModelRuntime} from '../../../acp/node_modules/@earendil-works/pi-coding-agent/dist/index.js';

export const browserCredentialId = "zepi-jev";
export const browserTextCredentialId = "zepi-browser-text";
export const browserDefaults = {apiUrl: "https://api.typesafe.ai/v1/systemone", model: "jev-latest"};
export const browserTextDefaults={apiUrl:'https://api.deepseek.com/v1',model:'deepseek-chat'};
const globalSettings = async directory => JSON.parse(await readFile(join(directory, "settings.json"), "utf8").catch(error => {
  if (error.code === "ENOENT") return "{}"; throw error;
}));
const settings=async directory=>(await globalSettings(directory)).zepi??{};
const auth = directory => new ReadOnlyAuthStorage(join(directory, "auth.json"));
function state(credentials, id, env) {
  return credentials.some(entry => entry.providerId === id && entry.type === "api_key") ? "configured" : env ? "environment" : "none";
}
export async function browserDecisionSettings(directory) {
  const config = (await settings(directory)).browserDecision ?? {};
  const credentials = await auth(directory).list();
  return {apiUrl: config.apiUrl ?? process.env.JEV_API_URL ?? browserDefaults.apiUrl,
    model: config.model ?? process.env.JEV_MODEL ?? browserDefaults.model,
    engine: config.engine ?? process.env.ZEPI_BROWSER_ENGINE ?? 'ultrafast',
    insecure: config.insecure ?? process.env.ZEPI_BROWSER_INSECURE ?? '',
    credential: state(credentials, browserCredentialId, process.env.TYPESAFE_API_KEY)};
}
export async function browserTextSettings(directory, runtime) {
  const global=await globalSettings(directory),config=global.zepi?.browserText??{};
  const source=config.source??process.env.TEXT_MODEL_SOURCE??(config.apiUrl||config.model||process.env.TEXT_MODEL_API_KEY||process.env.TEXT_MODEL_BASE_URL||process.env.TEXT_MODEL?'custom':'pi');
  if(source==='pi') {
    const models=runtime??await ModelRuntime.create({authPath:join(directory,'auth.json'),modelsPath:join(directory,'models.json'),modelsStorePath:join(directory,'models-store.json'),allowModelNetwork:false});
    const provider=global.defaultProvider??'',id=global.defaultModel??'',model=models.getModel(provider,id);
    const credential=model&&(models.getProviderAuthStatus(provider).configured||(await auth(directory).list()).some(c=>c.providerId===provider))?'configured':'none';
    return {source,provider,model:id,apiUrl:model?.baseUrl??'',overrideApiUrl:config.apiUrl??'',overrideModel:config.model??'',reasoning:config.reasoning??'low',credential,
      message:model?`Uses the main model (${provider}/${id}) and its pi authentication. Configure an optional override only to use a different text model.`:'Choose a main model in global settings to generate browser input text.'};
  }
  return {source:'custom',apiUrl: config.apiUrl || process.env.TEXT_MODEL_BASE_URL || browserTextDefaults.apiUrl,model: config.model || process.env.TEXT_MODEL || browserTextDefaults.model,
    reasoning: config.reasoning ?? process.env.TEXT_MODEL_REASONING ?? 'low',
    credential: state(await auth(directory).list(),browserTextCredentialId,process.env.TEXT_MODEL_API_KEY)};
}
async function resolveKey(directory,id,fallback) {
  const credential=await auth(directory).read(id);
  const key=credential?.type==='api_key'?credential.key:fallback;
  if (key?.startsWith('!')) throw new Error('Browser credentials support an API key or a $ENV_VAR reference.');
  if (key?.startsWith('$')) return process.env[key.slice(1)]??'';
  return key??'';
}
// Chromium-family executables usable without downloading anything, in preference order.
function probeCache(dir) {
    if (!dir || !existsSync(dir)) return undefined;
    const revisions = readdirSync(dir).filter(name => name.startsWith("chromium-") && !name.includes("headless")).sort().reverse();
    for (const revision of revisions)
      for (const rel of ["chrome-win/chrome.exe", "chrome-win64/chrome.exe"])
        if (existsSync(join(dir, revision, rel))) return join(dir, revision, rel);
    return undefined;
}

function detected(env) {
  // MCP children run with a reduced environment: derive the usual roots when they are missing.
  const drive = env.SYSTEMDRIVE || env.WINDIR?.slice(0, 2) || env.USERPROFILE?.slice(0, 2) || "C:";
  const local = env.LOCALAPPDATA || (env.USERPROFILE ? join(env.USERPROFILE, "AppData", "Local") : undefined);
  const programFiles = [env["ProgramFiles(x86)"] || join(drive + "\\", "Program Files (x86)"), env.ProgramFiles || join(drive + "\\", "Program Files")];
  const out = [];
  const bundled = probeCache(browserDirectory(undefined, env));
  if (bundled) out.push({ path: bundled, source: "zepi" });
  const cached = probeCache(local && join(local, "ms-playwright"));
  if (cached) out.push({ path: cached, source: "playwright" });
  const systemPaths = [
    ...programFiles.map(pf => join(pf, "Google", "Chrome", "Application", "chrome.exe")),
    local && join(local, "Google", "Chrome", "Application", "chrome.exe"),
    ...programFiles.map(pf => join(pf, "Microsoft", "Edge", "Application", "msedge.exe")),
  ];
  for (const candidate of systemPaths) if (candidate && existsSync(candidate)) out.push({ path: candidate, source: "system" });
  return out;
}

export function browserCandidates(env = process.env) {
  return detected(env);
}

export function resolveBrowserExecutable(settingPath, env = process.env) {
  if (settingPath) return existsSync(settingPath) ? { path: settingPath, source: "setting" } : { path: settingPath, source: "missing" };
  return detected(env)[0] ?? { path: undefined, source: "none" };
}
export async function browserDecisionEnvironment(directory) {
  const config=await browserDecisionSettings(directory), text=await browserTextSettings(directory);
  const browserConfig=(await settings(directory)).browser??{};
  const browser=resolveBrowserExecutable(browserConfig.executablePath||process.env.ZEPI_BROWSER_EXECUTABLE||'');
  const keepOpen=browserConfig.keepOpen===true||process.env.ZEPI_BROWSER_KEEP_OPEN==='1';
  const windowMode=['normal','maximized','fullscreen'].includes(browserConfig.window)?browserConfig.window:(process.env.ZEPI_BROWSER_WINDOW||'fullscreen');
  const key=await resolveKey(directory,browserCredentialId,process.env.TYPESAFE_API_KEY);
  const textKey=text.source==='custom'?await resolveKey(directory,browserTextCredentialId,process.env.TEXT_MODEL_API_KEY):'';
  return {JEV_API_URL:config.apiUrl,JEV_MODEL:config.model,ZEPI_BROWSER_ENGINE:config.engine,ZEPI_BROWSER_INSECURE:config.insecure??process.env.ZEPI_BROWSER_INSECURE??'',
    ZEPI_BROWSER_EXECUTABLE:browser.path??'',ZEPI_BROWSER_SOURCE:browser.source,ZEPI_BROWSER_KEEP_OPEN:keepOpen?'1':'',ZEPI_BROWSER_WINDOW:windowMode,
    TYPESAFE_API_KEY:key,TEXT_MODEL_SOURCE:text.source,TEXT_MODEL_BASE_URL:text.source==='custom'?text.apiUrl:'',TEXT_MODEL:text.model,TEXT_MODEL_REASONING:text.reasoning,TEXT_MODEL_API_KEY:textKey};
}

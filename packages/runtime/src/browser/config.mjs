import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

const globalSettings = async directory => JSON.parse(await readFile(join(directory, "settings.json"), "utf8").catch(error => {
  if (error.code === "ENOENT") return "{}"; throw error;
}));
const settings = async directory => (await globalSettings(directory)).zepi ?? {};

// Chromium-family executables usable without downloading anything, in preference order.
function detected(env) {
  // MCP children run with a reduced environment: derive the usual roots when they are missing.
  const drive = env.SYSTEMDRIVE || env.WINDIR?.slice(0, 2) || env.USERPROFILE?.slice(0, 2) || "C:";
  const local = env.LOCALAPPDATA || (env.USERPROFILE ? join(env.USERPROFILE, "AppData", "Local") : undefined);
  const programFiles = [env["ProgramFiles(x86)"] || join(drive + "\\", "Program Files (x86)"), env.ProgramFiles || join(drive + "\\", "Program Files")];
  const out = [];
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

// Native Rust/CDP browser environment (no decision/text model: the agent drives the browser).
export async function browserEnvironment(directory) {
  const config = (await settings(directory)).browser ?? {};
  const browser = resolveBrowserExecutable(config.executablePath || process.env.ZEPI_BROWSER_EXECUTABLE || '');
  const keepOpen = config.keepOpen === true || process.env.ZEPI_BROWSER_KEEP_OPEN === '1';
  const window = ['normal', 'maximized', 'fullscreen'].includes(config.window) ? config.window : (process.env.ZEPI_BROWSER_WINDOW || 'fullscreen');
  return { keepOpen, window, executablePath: browser.path ?? '', source: browser.source };
}
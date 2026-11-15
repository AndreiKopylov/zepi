import { existsSync, constants, accessSync, readFileSync } from "node:fs";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export interface Adapter {
  id: string;
  surface: "browser" | "desktop";
  version: string;
  command: string;
  args: string[];
  cwd: string;
  env: Record<string, string>;
}

export function packageInfo(name: string) {
  let dir = dirname(fileURLToPath(import.meta.resolve(name)));
  for (;;) {
    const path = join(dir, "package.json");
    if (existsSync(path)) {
      const pkg = JSON.parse(readFileSync(path, "utf8"));
      if (pkg.name === name) return { root: dir, version: String(pkg.version) };
    }
    const parent = dirname(dir);
    if (parent === dir) throw new Error(`Package manifest not found: ${name}`);
    dir = parent;
  }
}

export function findExecutable(command: string): string | undefined {
  const candidates = command.includes("/")
    ? [resolve(command)]
    : (process.env.PATH ?? "").split(delimiter).map((path) => join(path, command));
  return candidates.find((path) => {
    try { accessSync(path, constants.X_OK); return true; } catch { return false; }
  });
}

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));

export function findZepiBrowser(): string | undefined {
  const exe = process.platform === "win32" ? "zepi_browser.exe" : "zepi_browser";
  const candidates = [
    process.env.ZEPI_BROWSER_EXE,
    join(repoRoot, "..", "zepi-build", "release-fast", exe),
    join(repoRoot, "..", "zepi-build", "debug", exe),
    "C:/Llm/zepi/" + exe,
  ].filter(Boolean) as string[];
  return candidates.find((p) => existsSync(p));
}

export function browserAdapter(cwd: string): Adapter {
  return {
    id: "zepi-browser", surface: "browser", version: "chromiumoxide",
    command: findZepiBrowser() ?? "zepi_browser",
    args: ["serve"], cwd,
    env: {
      ZEPI_PI_CONFIG_DIR: process.env.ZEPI_PI_CONFIG_DIR ?? fileURLToPath(new URL("../../../.local/zepi", import.meta.url)),
    },
  };
}

export async function doctor() {
  const binary = findZepiBrowser();
  return {
    node: process.version,
    pi: JSON.parse(readFileSync(new URL("../../acp/node_modules/@earendil-works/pi-coding-agent/package.json", import.meta.url), "utf8")).version,
    browser: {
      engine: "chromiumoxide",
      binary: binary ?? "missing",
      control: binary ? "available_unprobed" : "missing_binary",
    },
  };
}
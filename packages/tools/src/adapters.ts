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

export function findBrowseCli(): string | undefined {
  const candidates = [
    process.env.ZEPI_BROWSE_CLI,
    join(repoRoot, "..", "browse", "cli.mjs"),
    "C:/Llm/browse/cli.mjs",
  ].filter(Boolean) as string[];
  return candidates.find((p) => existsSync(p));
}

export function browserAdapter(cwd: string): Adapter {
  const cli = findBrowseCli();
  return {
    id: "browse", surface: "browser", version: "browse",
    command: process.env.ZEPI_BUN ?? "bun",
    args: cli ? [cli, "mcp"] : ["mcp"], cwd,
    env: { OPENCODE_BROWSER_TOOLS: "plain" },
  };
}

export async function doctor() {
  const binary = findBrowseCli();
  return {
    node: process.version,
    pi: JSON.parse(readFileSync(new URL("../../acp/node_modules/@earendil-works/pi-coding-agent/package.json", import.meta.url), "utf8")).version,
    browser: {
      engine: "browse",
      binary: binary ?? "missing",
      control: binary ? "available_unprobed" : "missing_binary",
    },
  };
}
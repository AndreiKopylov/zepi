// Compile the ACP entry to plain JS so the runtime skips the tsx loader (custom.rs prefers cli.js).
// shortcut: esbuild is resolved from tsx dependency tree; declare it in devDependencies if tsx ever leaves.
import { build } from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
await build({
  entryPoints: [join(root, "packages/acp/src/cli.ts")],
  outfile: join(root, "packages/acp/src/cli.js"),
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node22",
  packages: "external",
  logLevel: "info",
});

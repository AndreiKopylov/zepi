import { mkdir } from "node:fs/promises";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { browserAdapter, doctor } from "../packages/tools/src/adapters.ts";
import { probeAdapter } from "../packages/tools/src/probe.ts";

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    cwd: { type: "string" },
    browser: { type: "boolean" },
  },
});

try {
  const command = positionals[0] ?? "doctor";
  if (command === "doctor") {
    console.log(JSON.stringify(await doctor(), null, 2));
  } else if (command === "probe") {
    if (!values.browser) throw new Error("Select --browser.");
    const directory = resolve(values.cwd ?? join(process.cwd(), ".local/tool-workspace"));
    await mkdir(directory, { recursive: true, mode: 0o700 });
    const adapters = [browserAdapter(directory)];
    for (const adapter of adapters) console.log(JSON.stringify(await probeAdapter(adapter), null, 2));
  } else {
    throw new Error("Available diagnostics: doctor, probe --browser.");
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : "Tool diagnostics failed.");
  process.exitCode = 1;
}

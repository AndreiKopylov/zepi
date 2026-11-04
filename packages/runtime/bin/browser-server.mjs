import { fileURLToPath } from "node:url";
import { browserDirectory } from "../src/paths.mjs";
import { browserDecisionEnvironment } from "../src/browser/config.mjs";

// Resolve resources before Playwright is imported by the browser service.
const browsersDir = browserDirectory();
if (browsersDir) process.env.PLAYWRIGHT_BROWSERS_PATH ??= browsersDir;
process.env.JEV_BROWSER_HEADED ??= "1";
process.env.JEV_BROWSER_LOG = "0";
delete process.env.JEV_BROWSER_PROFILE;
// Non-empty config resolutions win; empty ones must not clobber env injected by the host.
Object.assign(process.env, Object.fromEntries(Object.entries(await browserDecisionEnvironment(process.env.ZEPI_PI_CONFIG_DIR ?? fileURLToPath(new URL("../../../.local/zepi", import.meta.url)))).filter(([, v]) => v !== "")));
const { serveBrowser } = await import("../src/browser/server.mjs");
await serveBrowser();

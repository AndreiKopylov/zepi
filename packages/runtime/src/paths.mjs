import {join, dirname} from 'node:path';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';

// Source checkouts and installed bundles share the same product package layout.
export const runtimeRoot = process.versions?.bun && process.env.ZEPI_ROOT
  ? process.env.ZEPI_ROOT
  : fileURLToPath(new URL('../../../', import.meta.url));

// Under bun, pi resolves themes/CHANGELOG/package.json from PI_PACKAGE_DIR; point it
// at the on-disk pi package when present (dev/tests) so assets are not looked up next to the exe.
if (process.versions?.bun && !process.env.PI_PACKAGE_DIR) {
  const piPkg = join(runtimeRoot, 'packages/acp/node_modules/@earendil-works/pi-coding-agent');
  if (existsSync(join(piPkg, 'package.json'))) process.env.PI_PACKAGE_DIR = piPkg;
}
export const bundledPiEntry = new URL('../../acp/node_modules/@earendil-works/pi-coding-agent/dist/index.js', import.meta.url);

// Installed apps keep mutable pi state outside their read-only runtime bundle.
export function piDirectory(root = runtimeRoot) {
  return process.env.ZEPI_PI_CONFIG_DIR || join(root, '.local/zepi');
}

// MCP transports do not inherit arbitrary parent environment variables. Resolve
// the cache from the same layout used by the packager, even with a clean env.
export function browserDirectory(root = runtimeRoot, env = process.env) {
  if (env.PLAYWRIGHT_BROWSERS_PATH !== undefined) return env.PLAYWRIGHT_BROWSERS_PATH;
  if (existsSync(join(root, 'zepi-runtime.json'))) return join(dirname(root), 'browsers');
  // Portable bundles keep browsers beside the pi state; undefined keeps playwright's own registry.
  return [join(dirname(piDirectory(root)), 'browsers'), join(root, 'deps/browsers'), join(root, '.pi/browsers'), join(root, '.local/browsers')].find(candidate => existsSync(candidate));
}

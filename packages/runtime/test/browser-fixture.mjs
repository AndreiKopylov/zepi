import {test} from 'node:test';
import {browserDirectory} from '../src/paths.mjs';

// Bundles keep the playwright cache in deps/browsers; a dev checkout may have none at all.
const browsers = browserDirectory();
if (browsers) process.env.PLAYWRIGHT_BROWSERS_PATH = browsers;

export const browserTest = browsers
  ? test
  : (name, ...rest) => test(name, {skip: 'playwright browsers are not installed (deps/browsers is missing)'}, () => {});

import type { AgentSession } from "@earendil-works/pi-coding-agent";

export function browserGuidance(_session: AgentSession | undefined, _config: {backend?:string;tools?:string} = {}) {
  // Emit unconditionally: MCP tools may not be registered yet at prompt-build time.
  if (_config.tools !== 'plain') {
    return `\nZepi Browser Use runs on browse (the user's real browser via a Chromium extension) in code mode: use the execute tool to run JavaScript with a Playwright-style page (page.goto/click/fill/press/selectOption/locator/evaluate, waitFor*). Batch related actions in one execute call; use locators and waitFor for SPA forms and custom dropdowns; read the DOM via evaluate when needed. tabs lists/opens tabs; status reports connection. screenshot for visual evidence (pass fullPage:true for long pages). For login/2FA/payment use handoff and wait for the user to click Continue. Verify actual page state before claiming success.`;
  }
  return `\nZepi Browser Use runs on browse (the user's real browser via a Chromium extension). navigate opens a background tab and returns its id; pass that tab to every other call. snapshot gives a compact outline with refs (e12); click/fill/type/press/select/hover/scroll act on a ref or selector. Use fill for inputs (it sets the value with trusted input, including date fields); select for <select>; press for Enter/Tab/Escape. wait_for after async loads; screenshot for visual evidence (default is viewport only - pass fullPage:true for long pages/forms so nothing is cut off); eval to run JS in the page. For login/2FA/payment use handoff and wait for the user to click Continue. Re-snapshot after navigation or errors; verify actual page state before claiming success.`;
}

// browse manages its own tabs (idle close); nothing to release per turn.
export function browserLifecycle(_session: AgentSession, _options: {keepOpen?: boolean; backend?: string} = {}) {
  return {
    track(_name: string) {},
    async close(_force = false) {},
  };
}
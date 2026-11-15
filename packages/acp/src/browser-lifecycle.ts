import { randomUUID } from "node:crypto";
import type { AgentSession } from "@earendil-works/pi-coding-agent";

export function browserGuidance(_session: AgentSession | undefined, _config: {engine?:string;decisionConfigured?:boolean;textConfigured?:boolean;textSource?:string}) {
  // Emit unconditionally: MCP tools may not be registered yet at prompt-build time,
  // and the browser is always the native Rust/CDP engine now.
  return `\nZepi Browser Use runs on a native Rust/CDP browser with Playwright-style tools. browser_navigate to open a URL; browser_snapshot to observe indexed elements (node ids); browser_click to click; browser_type {text} to fill inputs; browser_press_key {key} for Enter/Tab/Escape/arrows; browser_wait_for {text|text_gone|time} after async loads. Prefer targeting by element (the visible label/name, e.g. element:"Фактический адрес") over node ids — it resolves live and survives SPA re-renders; node ids from browser_snapshot also work. For ANY dropdown/select/combobox (region, address, city) use browser_select_option {element|node, value} — it opens the menu, scrolls virtualized lists, clicks the option, falls back to keyboard and verifies; do NOT click dropdown options manually. browser_take_screenshot saves a JPEG and returns its path (not sent to the model); open the file for visual evidence. Re-observe with browser_snapshot after navigation or errors; never replay completed actions. Verify actual page state before claiming success.`;
}

// The published MCP bridge owns a separate browser for each ACP session.
// A completed prompt retains the conversation, but must release its test browser.
export function browserLifecycle(session: AgentSession, options: {keepOpen?: boolean} = {}) {
  let used = false;
  let closing: Promise<void> | undefined;
  return {
    track(name: string) {
      if (name.startsWith("mcp__zepi_browser__browser_")) used = true;
    },
    async close(force = false) {
      if (closing) return closing;
      if (!used) return;
      // keepOpen retains the browser between turns; disposal closes it for good.
      if (options.keepOpen && !force) return;
      closing = (async () => {
        const name = "mcp__zepi_browser__browser_close";
        const info = session.getAllTools().find(tool => tool.name === name);
        const tool = session.getToolDefinition(name);
        if (!tool || info?.sourceInfo.path !== "<inline:agentprism-pi-acp-mcp>") {
          throw new Error("The test browser cleanup tool is unavailable.");
        }
        // Invoke the existing runtime tool, after model tools settle. Cleanup is
        // unconditional, so it must not ask the model or enter approval hooks.
        const result = await tool.execute(`zepi-cleanup-${randomUUID()}`, {}, AbortSignal.timeout(5000), undefined, {} as Parameters<typeof tool.execute>[4]);
        if (result.isError) throw new Error('The task browser did not confirm cleanup.');
        used = false;
      })().finally(() => { closing = undefined; });
      return closing;
    },
  };
}

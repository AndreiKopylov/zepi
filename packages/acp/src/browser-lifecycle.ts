import { randomUUID } from "node:crypto";
import type { AgentSession } from "@earendil-works/pi-coding-agent";

export function browserGuidance(session: AgentSession | undefined, _config: {engine?:string;decisionConfigured?:boolean;textConfigured?:boolean;textSource?:string}) {
  const entry=session?.getAllTools().find(tool=>tool.name==='mcp__zepi_browser__browser_open');
  if(entry?.sourceInfo.path!=='<inline:agentprism-pi-acp-mcp>')return '';
  return `\nZepi Browser Use runs on a native Rust/CDP browser. For a browser task: browser_open to navigate, browser_snapshot to observe indexed elements, then browser_act to interact. Prefer the indexed node from browser_snapshot (kind click/fill/select); use x/y only when no node fits. press_key supports Enter/Tab/Escape/Backspace/Delete/arrows/Home/End. Re-observe with browser_snapshot after navigation, errors or takeover before acting; never replay completed actions. browser_screenshot saves a JPEG and returns its path (the image is not sent to the model); open the file for visual evidence. Verify actual page state before claiming success.`;
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

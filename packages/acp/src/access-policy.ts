import type { AgentSession, BeforeAgentStartEvent, Extension, ToolCallEvent } from "@earendil-works/pi-coding-agent";
import {browserGuidance} from './browser-lifecycle.ts';

export function accessPolicy(session:()=>AgentSession|undefined, track:(name:string)=>void, browserConfig:Parameters<typeof browserGuidance>[1]={}): Extension {
  const path = "<inline:zepi-access>";
  return {
    path, resolvedPath: path, sourceInfo: { path, source: "inline", scope: "user", origin: "top-level" },
    handlers: new Map([["before_agent_start", [async (raw: unknown) => {
      const event = raw as BeforeAgentStartEvent;
      event.systemPromptOptions.appendSystemPrompt += browserGuidance(session(),browserConfig);
      event.systemPromptOptions.appendSystemPrompt += "\nWhen a reply mentions a project file, render it as a markdown link so the user can open it from the agent panel: a path relative to the project root, like [Program.cs](src/Program.cs); for files outside the project use an absolute file:/// URL, like [notes.md](file:///C:/temp/notes.md). Do not wrap linked paths in inline code; keep plain inline code for identifiers and snippets only.";
    }]], ["tool_call", [async (raw:unknown) => {
      // browser lifecycle needs to see browser tool usage to release the task browser
      track((raw as ToolCallEvent).toolName);
      return undefined;
    }]]]),
    tools: new Map(), messageRenderers: new Map(), commands: new Map(), flags: new Map(), shortcuts: new Map(),
  };
}

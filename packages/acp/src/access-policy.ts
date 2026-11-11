import type { AgentSession, BeforeAgentStartEvent, Extension, ToolCallEvent } from "@earendil-works/pi-coding-agent";
import {browserGuidance} from './browser-lifecycle.ts';

export function accessPolicy(session:()=>AgentSession|undefined, track:(name:string)=>void, browserConfig:Parameters<typeof browserGuidance>[1]={}): Extension {
  const path = "<inline:zepi-access>";
  return {
    path, resolvedPath: path, sourceInfo: { path, source: "inline", scope: "user", origin: "top-level" },
    handlers: new Map([["before_agent_start", [async (raw: unknown) => {
      const event = raw as BeforeAgentStartEvent;
      event.systemPromptOptions.appendSystemPrompt += browserGuidance(session(),browserConfig);
      event.systemPromptOptions.appendSystemPrompt += "\nFile references in your reply are clickable markdown links, never inline code and never bare paths. When you name a file the user could open (one you read, edited or point at), link it: a path relative to the project root for project files, an absolute file:/// URL otherwise. When you know the line, point at it: append #L<line> to the link target, or #L<first>-L<last> for a range, and the editor opens right there; a :<line> suffix in the target works too. Inline code stays reserved for identifiers, symbols and values (PollingInterval, TooEarly, 425); a code span may carry a :<line> or :<first>-<last> suffix and stays clickable.\nWrong: `src/Web/appsettings.json` updated, see SmevSigningWorkflow.cs.\nRight: [appsettings.json](src/Web/appsettings.json) updated, see [SmevSigningWorkflow.cs](src/Workflow/SmevSigningWorkflow.cs#L120-L134), budget count in [CohortRequestsCountService.cs](src/Application/Services/Cohorts/CohortRequestsCountService.cs:48).";
    }]], ["tool_call", [async (raw:unknown) => {
      // browser lifecycle needs to see browser tool usage to release the task browser
      track((raw as ToolCallEvent).toolName);
      return undefined;
    }]]]),
    tools: new Map(), messageRenderers: new Map(), commands: new Map(), flags: new Map(), shortcuts: new Map(),
  };
}

import type { BeforeAgentStartEvent, Extension } from "@earendil-works/pi-coding-agent";

export function accessPolicy(): Extension {
  const path = "<inline:zepi-access>";
  return {
    path, resolvedPath: path, sourceInfo: { path, source: "inline", scope: "user", origin: "top-level" },
    handlers: new Map([["before_agent_start", [async (raw: unknown) => {
      const event = raw as BeforeAgentStartEvent;
      event.systemPromptOptions.appendSystemPrompt += "\nFile references in your reply are clickable markdown links, never inline code and never bare paths. When you name a file the user could open (one you read, edited or point at), link it: a path relative to the project root for project files, an absolute file:/// URL otherwise. When the reference points at a specific place the link MUST carry the line: append #L<line> to the link target, or #L<first>-L<last> for a range, and the editor opens right there; a :<line> suffix in the target works too. A bare path is only for the file as a whole: never name the file and mention its line separately in prose (`x.cs` (и `:23`), `x.cs` — на :76) — repeat the path with its own :<line> for each place. Inline code stays reserved for identifiers, symbols and values (PollingInterval, TooEarly, 425); a code span may carry a :<line> or :<first>-<last> suffix and stays clickable.\nWrong: `src/Web/appsettings.json` updated, see SmevSigningWorkflow.cs.\nRight: [appsettings.json](src/Web/appsettings.json) updated, see [SmevSigningWorkflow.cs](src/Workflow/SmevSigningWorkflow.cs#L120-L134), budget count in [CohortRequestsCountService.cs](src/Application/Services/Cohorts/CohortRequestsCountService.cs:48).";
    }]]]),
    tools: new Map(), messageRenderers: new Map(), commands: new Map(), flags: new Map(), shortcuts: new Map(),
  };
}
import type {AgentContext} from "@agentclientprotocol/sdk";
import {readdir} from "node:fs/promises";
import {createFindTool, createFindToolDefinition, createGrepTool, createGrepToolDefinition, createLsToolDefinition, defineTool, type AgentToolUpdateCallback} from "@earendil-works/pi-coding-agent";
import {readableFile, workspacePath} from "./workspace-path.ts";

export const EDITOR_QUERY = "_zepi/fs/query";
type Query = {path?: string; pattern?: string; glob?: string; limit?: number; context?: number; ignoreCase?: boolean; literal?: boolean};

/** Keep pi's public tool schemas; project queries run against the native editor, disk fallback outside it. */
export function editorQueryTools(cwd: string, sessionId: string, client: Pick<AgentContext, "request">) {
  const execute = (operation: "find" | "grep" | "ls") => async (_id: string, args: Query, signal?: AbortSignal, onUpdate?: AgentToolUpdateCallback<any>) => {
    signal?.throwIfAborted();
    for (const key of ["limit", "context"] as const) {
      const value = args[key];
      if (value !== undefined && (!Number.isSafeInteger(value) || value < (key === "limit" ? 1 : 0))) {
        throw new Error(`${key} must be ${key === "limit" ? "a positive" : "a non-negative"} integer.`);
      }
    }
    let path: string;
    try { path = await workspacePath(cwd, args.path ?? ".", true); }
    catch (error) {
      // Skill and command folders live outside the project: list them from disk, read-only.
      const external = await readableFile(cwd, args.path ?? ".");
      if (external.insideWorkspace) throw error;
      signal?.throwIfAborted();
      if (operation === "ls") {
        const limit = args.limit ?? 500;
        const entries = await readdir(external.canonical, {withFileTypes: true});
        const sorted = entries.sort((a, b) => a.name.localeCompare(b.name));
        const text = sorted.slice(0, limit).map(entry => entry.name + (entry.isDirectory() ? "/" : "")).join("\n");
        return {content: [{type: "text" as const, text}], details: {source: "editor", truncated: sorted.length > limit, entryLimitReached: sorted.length > limit ? limit : undefined}};
      }
      if (!args.pattern) throw new Error("Search pattern is required.");
      // The editor indexes nothing outside the project, so search the disk like stock pi.
      if (operation === "find") return createFindTool(external.canonical).execute(_id, {pattern: args.pattern, limit: args.limit}, signal, onUpdate);
      return createGrepTool(external.canonical).execute(_id, {pattern: args.pattern, glob: args.glob, ignoreCase: args.ignoreCase, literal: args.literal, context: args.context, limit: args.limit}, signal, onUpdate);
    }
    signal?.throwIfAborted();
    const result = await client.request<{output: string; truncated: boolean}>(EDITOR_QUERY,
      {sessionId, operation, ...args, path}, {cancellationSignal: signal});
    return {content: [{type: "text" as const, text: result.output}], details: {
      source: "editor", truncated: result.truncated,
      resultLimitReached: operation === "find" && result.truncated ? args.limit ?? 500 : undefined,
      matchLimitReached: operation === "grep" && result.truncated ? args.limit ?? 100 : undefined,
      entryLimitReached: operation === "ls" && result.truncated ? args.limit ?? 500 : undefined,
    }};
  };
  return [
    defineTool({...createFindToolDefinition(cwd), execute: execute("find")}),
    defineTool({...createGrepToolDefinition(cwd), execute: execute("grep"),
      description: "Search current editor contents, including unsaved changes, using a regex or literal pattern. Returns file paths, line numbers and optional context. Respects project exclusions. Read a file before editing it. Results are bounded; narrow the path or pattern when truncated. Outside the project it searches the disk like stock pi."}),
    defineTool({...createLsToolDefinition(cwd), execute: execute("ls")}),
  ];
}

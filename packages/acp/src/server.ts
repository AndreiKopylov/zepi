import {createMcpConnector} from './mcp.ts';
import {refreshModelCatalogs} from '../../runtime/src/pi/model-catalog.mjs';
import {sessionOwners} from './session-owner.ts';
import {refreshPiNetwork} from './pi-network.ts';
import {preparePromptContent} from './prompt-content.ts';
import {createDeliveryLedger, DELIVERY} from './delivery.ts';
import {nativeUiAction} from './native-ui.ts';
import { runAcp } from "../adapter/src/lib.js";
import { createAgentSession, ModelRuntime, type AgentSession } from "@earendil-works/pi-coding-agent";
import { basename, dirname, join } from "node:path";
import { readdir, rm, stat } from "node:fs/promises";
import { watch } from "node:fs";
import { ndJsonStream, type AgentContext, type McpServer, type Stream } from "@agentclientprotocol/sdk";
import {Readable, Writable} from 'node:stream';
import { editorTools } from "./editor-tools.ts";
import { editorQueryTools } from "./editor-query.ts";
import { accessPolicy } from "./access-policy.ts";
import { installPiCommands } from "./pi-commands.ts";
import { createSubagents } from "./subagents.ts";
import { installAgentToolPolicy } from "./agent-tools.ts";
import { configuredMcp } from "../../runtime/src/pi/extensions.mjs";
import { browserEnvironment } from "../../runtime/src/browser/config.mjs";
import { ensureBrowseMcp, ensureRelayMode } from "../../runtime/src/browser/browse.mjs";
import {codemodeImages} from './codemode-images.ts';

// Runtime leases are written per process id; a dead process leaves them behind forever.
export const sweepDeadRuntimes = (root: string) => (async () => {
  for (const entry of await readdir(root).catch(() => [] as string[])) {
    const pid = Number(basename(entry, ".mcp.json").replace(/\.json$/, ""));
    if (!Number.isInteger(pid) || pid === process.pid) continue;
    try { process.kill(pid, 0); } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "EPERM") continue;
      await rm(join(root, entry), {force: true}).catch(() => {});
    }
  }
})();

/**
 * Calls `onChange` after the watched files settle; failures are logged, never thrown.
 * One directory watcher and one debounce gate serve every file, so a save that touches
 * several of them refreshes once; an event arriving mid-refresh is replayed afterwards.
 * The directory is watched because each file is replaced atomically on every save.
 */
function watchModelsFile(paths: string[], onChange: () => Promise<void>) {
  const names = new Set(paths.map(path => basename(path)));
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running = false;
  let pending = false;
  const run = () => {
    if (running) { pending = true; return; }
    running = true;
    void onChange()
      .catch((error: unknown) => console.error("model catalog reload failed", error))
      .finally(() => {
        running = false;
        if (pending) { pending = false; run(); }
      });
  };
  try {
    const watcher = watch(dirname(paths[0] ?? "."), (_event, file) => {
      if (file && !names.has(file)) return;
      if (timer) clearTimeout(timer);
      timer = setTimeout(run, 500);
      timer.unref?.();
    });
    watcher.on("error", (error: unknown) => console.error("models.json watch failed", error));
  } catch (error) {
    console.error("models.json watch unavailable", error);
  }
}

export async function startZepiAgent(agentDir: string, sessionDir: string, stream?: Stream, modelRuntime?: ModelRuntime) {
  const zedConfigDir = process.env.ZEPI_ZED_CONFIG_DIR;
  const runtime = modelRuntime ?? await ModelRuntime.create({
    authPath: join(agentDir, "auth.json"), modelsPath: join(agentDir, "models.json"), allowModelNetwork: false,
  });
  let connectClient!: (client: AgentContext) => void;
  let supportsForms = false;
  let supportsNativeUi = false;
  const subagents = createSubagents(agentDir, sessionDir);
  const mcp = createMcpConnector(agentDir);
  // Skip the per-session local catalog rebuild when models/settings/auth are unchanged.
  let catalogKey = "";
  const browseSelfHeal = async () => {
    const cfg = await browserEnvironment(agentDir);
    if (cfg.backend !== "browse") return;
    await ensureBrowseMcp(agentDir, cfg.browseDir, cfg.browseTools);
    await ensureRelayMode(cfg.browseDir, cfg.browseTools);
  };
  const refreshModelsIfChanged = async () => {
    const key = (await Promise.all(["models.json", "settings.json", "auth.json"].map(async f => {
      try { return (await stat(join(agentDir, f))).mtimeMs; } catch { return 0; }
    }))).join(":");
    if (key === catalogKey) return;
    catalogKey = key;
    await runtime.refresh({ allowNetwork: false });
    void browseSelfHeal().catch(e => console.error("browse self-heal failed", e));
  };
  const deliveries = new Map<string, ReturnType<typeof createDeliveryLedger>>();
  const clientReady = new Promise<AgentContext>(resolve => { connectClient = resolve; });
  // The transport can receive initialize/new while asynchronous Zepi setup is
  // still installing wrappers. Admit requests only after that setup completes.
  let admitRequests!: () => void;
  const ready = new Promise<void>(resolve => {admitRequests = resolve;});
  const transport = stream ?? ndJsonStream(Writable.toWeb(process.stdout) as Parameters<typeof ndJsonStream>[0],
    Readable.toWeb(process.stdin) as Parameters<typeof ndJsonStream>[1]);
  const incoming = new TransformStream({async transform(message, controller) {
    await ready;
    controller.enqueue(message);
  }});
  const server = await runAcp({
    stream: {readable: transport.readable.pipeThrough(incoming), writable: transport.writable},
    deps: {
      ...{zepiClaimSession: sessionOwners(agentDir)},
      agentDir,
      sessionDir,
      connectMcpClient: mcp,
      modelRuntime: runtime,
      createAgentSession: async options => {
        if (!options.cwd || !options.sessionManager) throw new Error("Missing ACP session context.");
        const client = await clientReady;
        let liveSession:AgentSession|undefined;
        const images = codemodeImages();
        // Settings are owned by pi. New tasks see credentials/models changed in
        // Zepi's settings without replacing the model of an active conversation.
        await refreshModelsIfChanged();
        // Let pi resolve explicit lists, +/- modifiers, pending registrations and
        // reload additions. Only the absent-setting fallback is Zepi-specific.
        if (options.settingsManager) {
          const defaults = options.settingsManager.getDefaultTools.bind(options.settingsManager);
          options.settingsManager.getDefaultTools = () => defaults() ?? ["read", "edit", "write", "find", "grep", "ls", process.platform === "win32" ? "powershell" : "bash"];
        }
        if (options.resourceLoader) {
          const getExtensions = options.resourceLoader.getExtensions.bind(options.resourceLoader);
          options.resourceLoader.getExtensions = () => {
            const loaded = getExtensions();
            return { ...loaded, extensions: [...loaded.extensions, accessPolicy()] };
          };
          const python = process.env.ZEPI_PYTHON;
          const hasOfficePython = python && (await stat(python).catch(() => undefined))?.isFile();
          const append = options.resourceLoader.getAppendSystemPrompt.bind(options.resourceLoader);
          options.resourceLoader.getAppendSystemPrompt = () => [...append(),
            "Zepi uses Zed for the editor UI and pi for the coding agent. The paths below are Zepi's user-level configuration, not project settings.",
            `pi settings, models, and extensions are under ${agentDir} (settings.json, models.json, extensions/).`,
            zedConfigDir
              ? `Zed editor settings and keybindings are under ${zedConfigDir} (settings.json, keymap.json).`
              : "Zed's configuration directory was not provided by the editor. Do not guess it from standalone Zed defaults.",
            "Before writing code for an Office file, read the matching skill: `.docx` uses `zepi-docx`, `.xlsx` uses `zepi-xlsx`, `.pptx` uses `zepi-deck-build` and `zepi-deck-design`, and PDF text extraction uses `zepi-office-docs`. For visual checks of Word, Excel and PowerPoint files, follow `zepi-office-qa`: use the native renderer and inspect the PNGs. If it is unavailable, report that visual verification could not be done; do not guess the path or use LibreOffice or Microsoft Office.",
            "If edit reports that the text was not found, re-read the file and retry against the current contents; writes always land on disk, so a missing match means stale text, never a change waiting in a buffer.",
            "PowerShell has no heredocs (<<EOF, python - <<'PY') and no cmd flags such as dir /b. Write the script to a real file with the write tool, then invoke the appropriate interpreter on that file.",
            hasOfficePython
              ? "Zepi's bundled Office Python is available. Run Office scripts with `& $env:ZEPI_PYTHON <file>`. It includes python-docx, openpyxl, python-pptx, pypdf and Pillow; never run pip in it."
              : "Zepi's bundled Office Python is unavailable. If the task requires Office Python, tell the user it is unavailable instead of attempting to run scripts. Native Office viewers and `zepi office render` do not require Python.",
            "Use the bundled Pillow via `$env:ZEPI_PYTHON` for `zepi-image-edit` too. If the bundled runtime is unavailable, do not substitute another Python or install packages as a workaround for a bundled skill. For other Python tasks use `py -3` and create a project venv with `py -3 -m venv .venv`. If `py` is missing, tell the user the task needs system Python.",
            "fetch_content fetches http(s) URLs only. Local files are read with the read tool.",
          ];
        }
        const created = await createAgentSession({
          ...options,
          agentDir,
          // Preserve pi's defaultTools lifecycle with native file operations.
          tools: undefined,
          noTools: undefined,
          customTools: [...[...editorTools(options.cwd, options.sessionManager.getSessionId(), client, images.get),
            ...editorQueryTools(options.cwd, options.sessionManager.getSessionId(), client)]
            .map(tool => ({...tool, defaultActive: false})),
            ...(subagents.enabled ? [subagents.tool(options.sessionManager.getSessionId(), client)] : [])],
        });
        liveSession=created.session;
        // dsh-style tool rows: the model writes a short UI description for shell commands.
        for (const toolName of ["bash", "pwsh", "powershell"]) {
          const params = created.session.getToolDefinition(toolName)?.parameters as { properties?: Record<string, unknown>; required?: string[] } | undefined;
          if (params?.properties && !params.properties.description) {
            params.properties.description = { type: "string", description: 'Clear, concise description of what this command does in active voice, 5-10 words (shown in the UI). Examples: "ls" -> "List files in current directory"; "git status" -> "Show working tree status".' };
            params.required = [...(params.required ?? []), "description"];
          }
        }
        const disposeImages = images.attach(created.session);
        // pi reads most runtime settings dynamically. These Agent properties
        // are copied at creation, so refresh them at the same reload boundary.
        const settings = created.session.settingsManager;
        const reloadSettings = settings.reload.bind(settings);
        settings.reload = async () => {
          await reloadSettings();
          refreshPiNetwork(settings);
          created.session.agent.transport = settings.getTransport();
          created.session.agent.thinkingBudgets = settings.getThinkingBudgets();
          created.session.agent.maxRetryDelayMs = settings.getProviderRetrySettings().maxRetryDelayMs;
        };
        const ledger = createDeliveryLedger(created.session, (id,state) => {
          if(supportsNativeUi)void nativeUiAction(client,created.session.sessionId,'delivery_state',{id,state})
            .catch(error=>console.error('Delivery UI update failed',error));
        }, (message,entryId)=>subagents.consumedInput(created.session.sessionId,message,entryId));
        deliveries.set(created.session.sessionId, ledger);
        Object.defineProperty(created.session, DELIVERY, {value:ledger});
        const dispose = created.session.dispose.bind(created.session);
        created.session.dispose = () => { disposeImages(); ledger.dispose(); if(deliveries.get(created.session.sessionId)===ledger)deliveries.delete(created.session.sessionId); dispose(); };
        const prompt = created.session.prompt.bind(created.session);
        created.session.prompt = async (...args) => {
          const result = await prompt(...args);
          // Extension commands can start a custom run without awaiting it.
          // Pi 1.2+ is busy during before_agent_start as well. Keep the ACP
          // turn and its resources alive until that public SDK run settles.
          await created.session.waitForIdle();
          return result;
        };
        // Role restrictions apply after discovery.
        const child = subagents.register(created.session);
        const decisionTools = new Set<string>();
        let toolPolicy: ReturnType<typeof installAgentToolPolicy>;
        try { toolPolicy = installAgentToolPolicy(created.session, child ? child.role?.tools ?? ["read"] : undefined,
          tool => !decisionTools.has(tool.name)); }
        catch (error) {created.session.dispose(); throw error;}
        const reload = created.session.reload.bind(created.session);
        created.session.reload = async (...args) => {
          await reload(...args);
          toolPolicy.changed();
        };
        installPiCommands(created.session, client, supportsForms, agentDir, supportsNativeUi, () => server.agent.refreshGlobalModelScope());
        const dispatch = created.session.prompt.bind(created.session);
        let roleChecked = false;
        created.session.prompt = async (...args) => {
          // The adapter runs pi.prompt inside the MCP turn boundary, so registrations
          // have settled here; the strict role check belongs at this first prompt.
          if (!roleChecked) { roleChecked = true; toolPolicy.reset(); } else toolPolicy.changed();
          try { return await dispatch(...args); } finally { toolPolicy.registered(); }
        };
        return created;
      },
    },
  });
  const initialize = server.agent.initialize.bind(server.agent);
  server.agent.initialize = context => {
    supportsForms = context.params.clientCapabilities?.elicitation?.form != null;
    supportsNativeUi = context.params.clientCapabilities?._meta?.zepiNativeUi === 1;
    subagents.setEnabled(context.params.clientCapabilities?._meta?.zepiSubagents === 1);
    const result = initialize(context);
    return {...result, agentCapabilities: {...result.agentCapabilities,
      promptCapabilities: {...result.agentCapabilities?.promptCapabilities, embeddedContext: true}}};
  };
  await subagents.connect(server);
  const deliver = server.agent.prompt.bind(server.agent);
  server.agent.prompt = context => {
    const prepared = {...context, params: {...context.params, prompt: preparePromptContent(context.params.prompt)}};
    const ledger = deliveries.get(context.params.sessionId);
    return ledger ? ledger.deliver(context.params, () => deliver(prepared)) : deliver(prepared);
  };
  const steer = server.agent.steer.bind(server.agent);
  server.agent.steer = context => {
    const prepared = {...context, params: {...context.params, prompt: preparePromptContent(context.params.prompt)}};
    const ledger = deliveries.get(context.params.sessionId);
    return ledger ? ledger.steer(context.params, () => steer(prepared)) : steer(prepared);
  };
  // The configured servers enter the same ACP MCP bridge as built-in tools.
  const withMcp = async <T extends {params: {mcpServers?: McpServer[]; cwd: string}}>(context: T): Promise<T> => {
    let configured: McpServer[] = [];
    try {configured = await configuredMcp(agentDir, context.params.cwd);}
    catch { /* The bridge reports invalid configuration after the task opens. */ }
    const backendCfg = await browserEnvironment(agentDir);
    // Self-heal runs at startup / on settings change; only restore a missing entry here.
    if (backendCfg.backend === 'browse' && !configured.some(s => s.name === 'browse')) {
      if (await ensureBrowseMcp(agentDir, backendCfg.browseDir, backendCfg.browseTools)) {
        configured = await configuredMcp(agentDir, context.params.cwd);
      }
    }
    const bundled = [...(context.params.mcpServers ?? []).filter(server => server.name !== 'zepi_computer')];
    const servers = [...bundled, ...configured.filter(server => !bundled.some(s => s.name === server.name))];
    return {...context, params: {...context.params, mcpServers: servers}};
  };
  const create = server.agent.newSession.bind(server.agent), load = server.agent.loadSession.bind(server.agent);
  server.agent.newSession = async context => create(await withMcp(context));
  server.agent.loadSession = async context => load(await withMcp(context));
  connectClient(server.connection.client);
  // Settings edits to models.json must reach open threads without restarting them.
  // models.json changes the catalog; settings.json changes the enabled shortlist.
  const refreshCatalogScope = async () => {
    await runtime.refresh({ allowNetwork: false });
    await server.agent.refreshGlobalModelScope();
  };
  watchModelsFile([join(agentDir, "models.json"), join(agentDir, "settings.json")], refreshCatalogScope);
  void refreshModelCatalogs(runtime).then(() => server.agent.refreshGlobalModelScope())
    .catch((error: unknown) => console.error("background model catalog refresh failed", error));
  void browseSelfHeal().catch((error: unknown) => console.error("browse self-heal failed", error));
  void sweepDeadRuntimes(join(agentDir, "runtimes")).catch((error: unknown) =>
    console.error("runtime lease cleanup failed", error));
  admitRequests();
  return server;
}

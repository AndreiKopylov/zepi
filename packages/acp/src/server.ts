import {createMcpConnector} from './mcp.ts';
import {refreshModelCatalogs} from '../../runtime/src/pi/model-catalog.mjs';
import {sessionOwners} from './session-owner.ts';
import {createShell} from './shell.ts';
import {refreshPiNetwork} from './pi-network.ts';
import {preparePromptContent} from './prompt-content.ts';
import {createDeliveryLedger, DELIVERY} from './delivery.ts';
import {nativeUiAction} from './native-ui.ts';
import { runAcp } from "../adapter/src/lib.js";
import { createAgentSession, ModelRuntime, type AgentSession } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";
import { ndJsonStream, type AgentContext, type McpServer, type Stream } from "@agentclientprotocol/sdk";
import {Readable, Writable} from 'node:stream';
import { editorTools } from "./editor-tools.ts";
import { editorQueryTools } from "./editor-query.ts";
import { createPreview } from "./preview.ts";
import { browserLifecycle } from "./browser-lifecycle.ts";
import { accessPolicy } from "./access-policy.ts";
import { installPiCommands } from "./pi-commands.ts";
import { createSubagents } from "./subagents.ts";
import { installAgentToolPolicy } from "./agent-tools.ts";
import { configuredMcp } from "../../runtime/src/pi/extensions.mjs";
import { browserDecisionEnvironment } from "../../runtime/src/browser/config.mjs";
import {codemodeImages} from './codemode-images.ts';

export async function startZepiAgent(agentDir: string, sessionDir: string, stream?: Stream, modelRuntime?: ModelRuntime) {
  const runtime = modelRuntime ?? await ModelRuntime.create({
    authPath: join(agentDir, "auth.json"), modelsPath: join(agentDir, "models.json"), allowModelNetwork: false,
  });
  let connectClient!: (client: AgentContext) => void;
  let supportsForms = false;
  let supportsNativeUi = false;
  const subagents = createSubagents(agentDir, sessionDir);
  const mcp = createMcpConnector(agentDir);
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
        let browserForce: (() => void) | undefined;
        const browserDecision = await browserDecisionEnvironment(agentDir);
        const images = codemodeImages();
        const preview = createPreview(options.cwd, join(agentDir, "previews"), options.sessionManager.getSessionId(), client);
        const shell = createShell(options.cwd, join(agentDir, "commands"), options.sessionManager.getSessionId(), client);
        // Settings are owned by pi. New tasks see credentials/models changed in
        // Zepi's settings without replacing the model of an active conversation.
        if (!modelRuntime) await runtime.refresh({ allowNetwork: false });
        // Let pi resolve explicit lists, +/- modifiers, pending registrations and
        // reload additions. Only the absent-setting fallback is Zepi-specific.
        if (options.settingsManager) {
          const defaults = options.settingsManager.getDefaultTools.bind(options.settingsManager);
          options.settingsManager.getDefaultTools = () => defaults() ?? ["read", "edit", "write", "find", "grep", "ls", "bash"];
        }
        if (options.resourceLoader) {
          const getExtensions = options.resourceLoader.getExtensions.bind(options.resourceLoader);
          options.resourceLoader.getExtensions = () => {
            const loaded = getExtensions();
            return { ...loaded, extensions: [...loaded.extensions, preview.extension, shell.extension, accessPolicy(()=>liveSession,name=>browser.track(name),{
              engine:browserDecision.ZEPI_BROWSER_ENGINE,decisionConfigured:!!browserDecision.TYPESAFE_API_KEY,
              textConfigured:browserDecision.TEXT_MODEL_SOURCE==='pi'||!!browserDecision.TEXT_MODEL_API_KEY,textSource:browserDecision.TEXT_MODEL_SOURCE,
            })] };
          };
          const append = options.resourceLoader.getAppendSystemPrompt.bind(options.resourceLoader);
          options.resourceLoader.getAppendSystemPrompt = () => [...append(),
            "Office documents: when a task involves .docx, .xlsx, .pptx or .pdf, load the matching skill before writing any code — read its SKILL.md from the skill list in this prompt, or run /skill:zepi-docx, /skill:zepi-xlsx, /skill:zepi-deck-build, /skill:zepi-office-qa. A Word file means the \`zepi-docx\` skill, a spreadsheet the \`zepi-xlsx\` skill, a deck \`zepi-deck-build\` with \`zepi-deck-design\`. Those skills define the runtime (the bundled Python), the known traps and the verification step; re-deriving the pipeline from a sample file or from experience skips all three and is not acceptable. A visual check means rendering the result with \`zepi office render\` and looking at the PNGs, not only a script exiting zero. Reading a skill costs one tool call.",
            "Edit a file one way only: through edit/write or through the shell, never both for the same file in one turn — a shell rewrite makes the recorded review stale and resurfaces as a conflict the next time the thread opens.",
            "If edit reports that the text was not found, re-read the file and retry against the current contents; writes always land on disk, so a missing match means stale text, never a change waiting in a buffer.",
            "PowerShell has no heredocs (<<EOF, python - <<'PY') and no cmd flags such as dir /b. Write the script to a real file with the write tool, then run python <file>; never assume a heredoc created a file.",
            "Two Pythons, and they are not interchangeable.",
            "Bundled (the default): `python`, `python3` or `$env:ZEPI_PYTHON`, living in `<ZEPI_ROOT>\\deps\\python`. Has python-docx, openpyxl, python-pptx, pypdf and Pillow, and no pip. Use it for every .docx/.xlsx/.pptx task; never invent a path under %LOCALAPPDATA% and never try `pip` here.",
            "System: reachable only through `py` or `py -3`, if the machine has Python at all. It has pip but none of the Office libraries. Use it for anything else (pandas, requests, scraping) and always create a project venv (`.venv`) with `py -3 -m venv .venv`, installing into the venv rather than the global interpreter. If `py` is not found, tell the user the task needs a system Python instead of silently switching to the bundled one.",
            "fetch_content fetches http(s) URLs only. Local files are read with the read tool.",
            `For development verification, use preview start with explicit editor files, including unsaved changes. For static HTML/CSS/JS supply an HTML entry. For projects supply commands for checks/builds and a server command; select installed dependency directories to copy into the captured project. Read the project configuration before choosing commands. Commands run in a copy and do not update source buffers. Open its exact URL with the bundled browser tools, observe, interact, then observe the actual result. After a failure, read and repair the files, start a new preview and repeat the check. Capture a screenshot when visual inspection matters. Before reporting completion, use preview status; a stale or unknown input state does not verify current edits. Include the run ID and tested criteria in the result. Preview status and successful browser calls are not acceptance passes. Evidence covers only captured files and copied dependencies. Successful check exits and server reachability alone do not demonstrate correct behavior. Commands that modify captured source invalidate freshness; repair editor buffers and recapture instead. Never install missing dependencies without user authorization. Re-observe after cancellation or user takeover. Stop preview when finished. Project commands and servers also stop automatically when this turn ends. ${browserDecision.ZEPI_BROWSER_KEEP_OPEN === '1' ? "The task browser stays open between turns so a later turn can continue the same page state; it closes when the task closes." : "Zepi automatically closes the task browser when this turn ends or is cancelled; reopen and re-observe in a later turn."} If the target cannot be started with available tools, report missing verification instead of claiming success.`
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
            .map(tool => ({...tool, defaultActive: false})), preview.tool, shell.tool,
            ...(subagents.enabled ? [subagents.tool(options.sessionManager.getSessionId(), client)] : [])],
        });
        liveSession=created.session;
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
        created.session.dispose = () => { browserForce?.(); disposeImages(); ledger.dispose(); if(deliveries.get(created.session.sessionId)===ledger)deliveries.delete(created.session.sessionId); dispose(); };
        const browser = browserLifecycle(created.session, {keepOpen: browserDecision.ZEPI_BROWSER_KEEP_OPEN === '1'});
        browserForce = () => { void browser.close(true).catch(error => console.error("Zepi browser cleanup failed:", error)); };
        const prompt = created.session.prompt.bind(created.session);
        created.session.prompt = async (...args) => {
          try {
            const result = await prompt(...args);
            // Extension commands can start a custom run without awaiting it.
            // Pi 1.2+ is busy during before_agent_start as well. Keep the ACP
            // turn and its resources alive until that public SDK run settles.
            await created.session.waitForIdle();
            return result;
          } finally {
            const cleanup = await Promise.allSettled([browser.close(), preview.finishTurn(!!child), shell.finishTurn()]);
            const failed = cleanup.find(result => result.status === "rejected");
            if (failed?.status === "rejected") throw failed.reason;
          }
        };
        // The adapter awaits pi.abort() on cancellation. Settlement hooks are not
        // guaranteed on an aborted turn, so release preview after tools settle.
        const abort = created.session.abort.bind(created.session);
        created.session.abort = async () => {
          try { await abort(); } finally {
            const results = await Promise.allSettled([preview.finishTurn(true), shell.finishTurn(), browser.close()]);
            const failed = results.find(result => result.status === "rejected");
            if (failed?.status === "rejected") throw failed.reason;
          }
        };
        // The SDK bash override uses current editor captures and the same native
        // process supervisor as preview. Role restrictions apply after discovery.
        const child = subagents.register(created.session);
        const decisionTools = new Set(["do", "check", "choose"].map(name => `mcp__zepi_browser__browser_${name}`));
        let toolPolicy: ReturnType<typeof installAgentToolPolicy>;
        try { toolPolicy = installAgentToolPolicy(created.session, child ? child.role?.tools ?? ["read"] : undefined,
          tool => !!browserDecision.TYPESAFE_API_KEY || !decisionTools.has(tool.name)); }
        catch (error) {created.session.dispose(); throw error;}
        const reload = created.session.reload.bind(created.session);
        created.session.reload = async (...args) => {
          await reload(...args);
          toolPolicy.changed();
        };
        installPiCommands(created.session, client, supportsForms, agentDir, supportsNativeUi, () => server.agent.refreshGlobalModelScope());
        const dispatch = created.session.prompt.bind(created.session);
        created.session.prompt = async (...args) => {
          toolPolicy.changed();
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
    const bundled = [...(context.params.mcpServers ?? []).filter(server => server.name !== 'zepi_computer')];
    const servers = [...bundled, ...configured.filter(server => !bundled.some(s => s.name === server.name))].map(server =>
      server.name === "zepi_browser" && "command" in server
        ? {...server, env: [...server.env?.filter(entry => entry.name !== "ZEPI_PI_CONFIG_DIR") ?? [], {name: "ZEPI_PI_CONFIG_DIR", value: agentDir}]}
        : server);
    return {...context, params: {...context.params, mcpServers: servers}};
  };
  const create = server.agent.newSession.bind(server.agent), load = server.agent.loadSession.bind(server.agent);
  server.agent.newSession = async context => create(await withMcp(context));
  server.agent.loadSession = async context => load(await withMcp(context));
  connectClient(server.connection.client);
  void refreshModelCatalogs(runtime).then(() => server.agent.refreshGlobalModelScope())
    .catch((error: unknown) => console.error("background model catalog refresh failed", error));
  admitRequests();
  return server;
}

import {piDirectory, runtimeRoot as root} from '../paths.mjs';
import {ModelRuntime, SettingsManager, readStoredCredential, FileSettingsStorage, AuthStorage, ReadOnlyAuthStorage, ModelConfig, getSupportedThinkingLevels, resolveModelScopeFromModels, piManifest} from './embed.mjs';
import { readFile, writeFile, mkdir, chmod, rename, mkdtemp, rm, access } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { homedir } from "node:os";
import { installBrowse, updateBrowse, checkBrowseUpdate, browseStatus, pairBrowse, setBrowseSettings, writeBrowseMcp, removeBrowseMcp, ensureBrowseMcp, restartBrowseRelay } from "../browser/browse.mjs";
import {modelSettings, mergeModelSettings, PiRuntimeSettingsError} from './runtime-settings.mjs';
import {httpProxyStatus, mergeHttpProxy, PiHttpSettingsError} from './http-settings.mjs';
import {refreshModelCatalogs} from './model-catalog.mjs';
import {connectionRevision, projectConnections, normalizeModelIds, connectionEndpoint, fetchConnectionModels, resolveDraftKey} from './custom-models.mjs';

export class PiConfigError extends Error {}

// Use pi's own locked credential store and models.json validator. These internal
// APIs are audited against the exact bundled version, not reimplemented here.
const manifest = piManifest;
export const bundledVersion = manifest.version;
if (bundledVersion !== "1.6.0") throw new PiConfigError("Zepi requires bundled pi 1.6.0. Run setup:acp.");

async function readJson(path, fallback = {}) {
  try { return JSON.parse(await readFile(path, "utf8")); }
  catch (error) { if (error.code === "ENOENT") return fallback; throw new PiConfigError("Invalid configuration file format. The file was not changed."); }
}
async function atomicJson(path, value) {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${process.pid}.${crypto.randomUUID()}.tmp`;
  try {
    await writeFile(temporary, JSON.stringify(value, null, 2) + "\n", { mode: 0o600 });
    await rename(temporary, path);
    await chmod(path, 0o600);
  } finally { await rm(temporary, { force: true }); }
}
function text(value, label) {
  if (typeof value !== "string" || !value.trim() || value.length > 8192) throw new PiConfigError(`${label} is required and must be at most 8192 characters.`);
  return value.trim();
}
function providerId(value) {
  const id = text(value, "Provider ID");
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/.test(id)) throw new PiConfigError("Provider IDs may only contain letters, numbers, dots, hyphens, and underscores.");
  return id;
}

export function createPiSettings(directory = piDirectory(root), sourceDirectory = join(homedir(), ".pi/agent")) {
  const authPath = join(directory, "auth.json");
  const modelsPath = join(directory, "models.json");
  const settings = () => SettingsManager.create(directory, directory, { projectTrusted: false });
  const runtime = () => ModelRuntime.create({ authPath, modelsPath, allowModelNetwork: false });
  const validateModels = async (path) => {
    const config = await ModelConfig.load(path);
    if (config.getError()) throw new PiConfigError("models.json did not pass pi validation. Check the provider, model, and API fields.");
    return config;
  };
  const writeModels = async (providers, validateOnly = false) => {
    await mkdir(directory, { recursive: true, mode: 0o700 });
    const temporary = await mkdtemp(join(directory, ".models-validation-"));
    try {
      const path = join(temporary, "models.json");
      await atomicJson(path, { providers });
      await validateModels(path);
      const check = await ModelRuntime.create({ authPath, modelsPath: path, refreshOnCreate: false, allowModelNetwork: false });
      if (check.getError()) throw new PiConfigError("Invalid custom model API, URL, or parameters. Configuration was not changed.");
      if (!validateOnly) await atomicJson(modelsPath, { providers });
      return check;
    } finally { await rm(temporary, { recursive: true, force: true }); }
  };

  async function browserStatus() {
    const config = (await readJson(join(directory, "settings.json"), {})).zepi?.browser ?? {};
    return { backend: "browse", browseCli: config.browseCli ?? "", browseDir: config.browseDir ?? join(directory, "browse"), browseTools: config.browseTools === "plain" ? "plain" : "execute" };
  }

  async function status(refreshModels = false) {
    const manager = settings();
    if (manager.drainErrors().length) throw new PiConfigError("Unable to read pi settings.json. Fix the configuration and try again.");
    const validated = await validateModels(modelsPath);
    const modelRuntime = await runtime();
    if (modelRuntime.getError()) throw new PiConfigError("Invalid pi model configuration. Fix models.json and try again.");
    const modelCatalogNotice = refreshModels ? await refreshModelCatalogs(modelRuntime, true) : '';
    const credentials = await new ReadOnlyAuthStorage(authPath).list();
    const patterns = manager.getEnabledModels() ?? [];
    const enabledRefs = patterns.length ? new Set(resolveModelScopeFromModels(patterns, modelRuntime.getModels()).scopedModels
      .map(({model}) => `${model.provider}/${model.id}`)) : null;
    const customIds = new Set(validated.providers.keys());
    const credentialSource = provider => {
      const stored = credentials.find(entry => entry.providerId === provider.id)?.type;
      if (stored) return stored;
      const inline = validated.providers.get(provider.id)?.apiKey;
      if (typeof inline === "string" && inline.trim()) {
        const value = inline.trim();
        return value.startsWith("$") ? `models.json $${value.slice(1)}` : "models.json";
      }
      return modelRuntime.getProviderAuthStatus(provider.id).configured ? "environment" : "none";
    };
    const providers = modelRuntime.getProviders().map(provider => {
      const credential = credentials.find(entry => entry.providerId === provider.id);
      const state = modelRuntime.getProviderAuthStatus(provider.id);
      return { id: provider.id, name: provider.name ?? provider.id, custom: customIds.has(provider.id),
        credential: credential?.type ?? (state.configured ? state.source ?? "configured" : "none"),
        credentialSource: credentialSource(provider),
        models: modelRuntime.getModels(provider.id).map(model => ({ id: model.id, name: model.name,
          enabled: enabledRefs ? enabledRefs.has(`${provider.id}/${model.id}`) : true,
          thinkingLevels: getSupportedThinkingLevels(model), settings: modelSettings(manager, model) })) };
    }).filter(provider => provider.models.length)
      // Built-in providers first, then custom connections, each alphabetically.
      .sort((a, b) => Number(a.custom) - Number(b.custom) || a.name.localeCompare(b.name));
    return { version: bundledVersion, adapterVersion: "0.9.4", directory, modelCatalogNotice,
      defaultProvider: manager.getDefaultProvider() ?? "",
      defaultModel: manager.getDefaultModel() ?? "",
      defaultThinkingLevel: manager.getDefaultThinkingLevel() ?? "off",
      enabledModels: patterns,
      defaultTools: (await readJson(join(directory, "settings.json"))).defaultTools ?? null,
      defaultToolsEffective: (() => { try { return manager.getDefaultTools() ?? null; } catch { return null; } })(),
      httpProxy: httpProxyStatus(manager.getGlobalSettings()),
      browser: await browserStatus(),
      customConnections: projectConnections(validated.providers, modelRuntime),
      providers, update: await readJson(join(directory, "pi-update.json"), null) };
  }

  async function execute(request) {
    const operation = request?.operation ?? "status";
    await mkdir(directory, { recursive: true, mode: 0o700 });
    if (!["status", "check-update", "import"].includes(operation)) await status();
    if (operation === 'http-proxy') {
      new FileSettingsStorage(directory, directory).withLock('global', current => {
        try { return JSON.stringify(mergeHttpProxy(current ? JSON.parse(current) : {}, request.proxy, request.expected), null, 2) + '\n'; }
        catch (error) { throw new PiConfigError(error instanceof PiHttpSettingsError ? error.message : 'Unable to read global HTTP proxy settings. Repair settings.json before saving.'); }
      });
      await chmod(join(directory, 'settings.json'), 0o600);
    } else if (operation === 'model-settings') {
      const provider = providerId(request.provider);
      const model = (await runtime()).getModel(provider, text(request.model, 'Model'));
      if (!model) throw new PiConfigError('The selected model is not in the pi catalog.');
      new FileSettingsStorage(directory, directory).withLock('global', current => {
        try {return JSON.stringify(mergeModelSettings(current ? JSON.parse(current) : {}, `${provider}/${model.id}`,
          request.changes, request.expected, getSupportedThinkingLevels(model)), null, 2) + '\n';}
        catch (error) {throw new PiConfigError(error instanceof PiRuntimeSettingsError ? error.message : 'Unable to read model settings. Repair settings.json before saving.');}
      });
      await chmod(join(directory, 'settings.json'), 0o600);
    } else if (operation === "tool-defaults") {
      const selected = request.tools;
      if (selected !== null && (!Array.isArray(selected) || selected.length > 256
        || selected.some(name => typeof name !== 'string' || !/^[+-]?[A-Za-z0-9_*][A-Za-z0-9_.*-]{0,255}$/.test(name)))) {
        throw new PiConfigError('Enter comma-separated tool names, optional + or - prefixes, or [] for no default file tools.');
      }
      new FileSettingsStorage(directory, directory).withLock('global', current => {
        const value = current ? JSON.parse(current) : {};
        if (JSON.stringify(value.defaultTools ?? null) !== JSON.stringify(request.expected ?? null)) {
          throw new PiConfigError('Tool defaults changed while this page was open. Reload the page before saving.');
        }
        if (selected === null) delete value.defaultTools;
        else value.defaultTools = selected;
        return JSON.stringify(value, null, 2) + '\n';
      });
      await chmod(join(directory, 'settings.json'), 0o600);
    } else if (operation === "browser-set") {
      const backend = request.backend, browseCli = request.browseCli;
      if (backend !== undefined && backend !== "browse") throw new PiConfigError("Backend must be browse.");
      if (browseCli !== undefined && typeof browseCli !== "string") throw new PiConfigError("Browse CLI must be a path.");
      new FileSettingsStorage(directory, directory).withLock("global", current => {
        const value = current ? JSON.parse(current) : {};
        const browser = { ...value.zepi?.browser };
        if (backend !== undefined) browser.backend = backend;
        if (browseCli !== undefined) { if (browseCli.trim()) browser.browseCli = browseCli.trim(); else delete browser.browseCli; }
        if (request.browseDir !== undefined) browser.browseDir = request.browseDir;
        if (request.browseTools !== undefined) browser.browseTools = request.browseTools;
        value.zepi = { ...value.zepi, browser };
        return JSON.stringify(value, null, 2) + "\n";
      });
      await chmod(join(directory, "settings.json"), 0o600);
      return status();
    } else if (operation === "browse-status") {
      const bs = await browserStatus();
      await ensureBrowseMcp(directory, bs.browseDir, bs.browseTools);
      return browseStatus(bs.browseDir, bs.browseTools);
    } else if (operation === "browse-install") {
      const dir = request.dir || (await browserStatus()).browseDir;
      await installBrowse(dir);
      const tools = (await browserStatus()).browseTools;
      await writeBrowseMcp(directory, dir, tools);
      new FileSettingsStorage(directory, directory).withLock("global", current => {
        const value = current ? JSON.parse(current) : {};
        value.zepi = { ...value.zepi, browser: { ...value.zepi?.browser, browseDir: dir, backend: "browse" } };
        return JSON.stringify(value, null, 2) + "\n";
      });
      return browseStatus(dir, tools);
    } else if (operation === "browse-check-update") {
      return checkBrowseUpdate((await browserStatus()).browseDir);
    } else if (operation === "browse-update") {
      const dir = (await browserStatus()).browseDir;
      await updateBrowse(dir);
      const tools = (await browserStatus()).browseTools;
      await writeBrowseMcp(directory, dir, tools);
      return browseStatus(dir, tools);
    } else if (operation === "browse-pair") {
      return { code: await pairBrowse((await browserStatus()).browseDir) };
    } else if (operation === "browse-settings") {
      const dir = (await browserStatus()).browseDir;
      if (!(await browseStatus(dir)).installed) return { settings: {} };
      const { execFile } = await import("node:child_process");
      const { promisify } = await import("node:util");
      try {
        const { stdout } = await promisify(execFile)(join(dir, "bun.exe"), [join(dir, "cli.mjs"), "settings"], { timeout: 30000 });
        return JSON.parse(stdout);
      } catch { return { settings: {} }; }
    } else if (operation === "browse-set") {
      return setBrowseSettings((await browserStatus()).browseDir, request.changes ?? {});
    } else if (operation === "browse-mcp") {
      const bs = await browserStatus();
      const tools = request.tools ?? bs.browseTools;
      if (request.enabled === false) await removeBrowseMcp(directory);
      else await writeBrowseMcp(directory, bs.browseDir, tools);
      new FileSettingsStorage(directory, directory).withLock("global", current => {
        const value = current ? JSON.parse(current) : {};
        value.zepi = { ...value.zepi, browser: { ...value.zepi?.browser, browseTools: tools } };
        return JSON.stringify(value, null, 2) + "\n";
      });
      // Relay caches the tools mode at startup; restart in the background so the UI doesn't block.
      restartBrowseRelay(bs.browseDir).catch(() => {});
      return status();
    } else if (operation === "defaults") {
      const provider = providerId(request.provider);
      const modelRuntime = await runtime();
      const model = modelRuntime.getModel(provider, text(request.model, "Model"));
      if (!model) throw new PiConfigError("The selected model is not in the pi catalog.");
      if (!getSupportedThinkingLevels(model).includes(request.thinking)) throw new PiConfigError("This model does not support the selected thinking level.");
      const manager = settings();
      if (manager.drainErrors().length) throw new PiConfigError("Unable to read pi settings. Configuration was not changed.");
      manager.setDefaultModelAndProvider(provider, model.id);
      manager.setDefaultThinkingLevel(request.thinking);
      await manager.flush();
      if (manager.drainErrors().length) throw new PiConfigError("Unable to save pi settings. Try again.");
      await chmod(join(directory, "settings.json"), 0o600);
    } else if (operation === "key") {
      const provider = providerId(request.provider);
      if (!(await runtime()).getProvider(provider)) throw new PiConfigError("Configure a model for this provider first.");
      const key = text(request.key, "API Key");
      await AuthStorage.create(authPath).modify(provider, async current => ({ type: "api_key", key,
        ...(current?.type === "api_key" && current.env ? { env: current.env } : {}) }));
      await chmod(authPath, 0o600);
    } else if (operation === "remove-key") {
      const provider = providerId(request.provider);
      const stored = (await new ReadOnlyAuthStorage(authPath).list()).some(entry => entry.providerId === provider);
      if (!stored) {
        const inline = (await validateModels(modelsPath)).providers.get(provider)?.apiKey;
        const where = typeof inline === "string" && inline.trim()
          ? (inline.trim().startsWith("$") ? `models.json (${inline.trim()})` : "models.json")
          : "the environment";
        throw new PiConfigError(`No stored credential for ${provider}: it comes from ${where}. Nothing was changed.`);
      }
      await AuthStorage.create(authPath).delete(provider);
    } else if (operation === "list-connection-models") {
      try {
        const provider = providerId(request.provider);
        const endpoint = connectionEndpoint(request.baseUrl, request.api);
        const config = await validateModels(modelsPath);
        const current = config.providers.get(provider);
        const modelRuntime = await runtime();
        const key = request.key?.trim() ? resolveDraftKey(request.key) : undefined;
        let auth;
        try { auth = modelRuntime.getProvider(provider) ? await modelRuntime.getAuth(provider, key ? {apiKey:key} : undefined) : undefined; }
        catch { throw new PiConfigError("Unable to resolve connection credentials."); }
        return await fetchConnectionModels(endpoint, request.api, key, auth, auth?.auth.headers ?? current?.headers);
      } catch (error) { throw new PiConfigError(error instanceof PiConfigError || error?.name === 'ConnectionError' ? error.message : "Unable to fetch models. Your draft was not changed."); }
    } else if (operation === "custom-connection") {
      const provider = providerId(request.provider);
      try { connectionEndpoint(request.baseUrl, request.api); }
      catch (error) { throw new PiConfigError(error.message); }
      let ids;
      try { ids = normalizeModelIds(request.models); } catch (error) { throw new PiConfigError(error.message); }
      const key = request.key?.trim() || undefined;
      if (key && (key.length > 8192 || /[\r\n]/.test(key) || key.startsWith('!'))) throw new PiConfigError("Use an API key or a $ENV_VAR reference for this connection.");
      if (key) await new ReadOnlyAuthStorage(authPath).list();
      const config = await validateModels(modelsPath);
      const providers = Object.fromEntries(config.providers);
      const current = providers[provider];
      if ((request.expectedRevision ?? null) !== (current ? connectionRevision(current) : null)) throw new PiConfigError("Connection changed while this page was open. Reload before saving.");
      const previous = new Map((current?.models ?? []).map(model => [model.id, model]));
      providers[provider] = {...current, baseUrl:request.baseUrl.trim(), api:request.api,
        models:ids.map(id => previous.get(id) ?? {id, name:id})};
      const checked = await writeModels(providers, true);
      const manager = settings();
      if (manager.getDefaultProvider() === provider && manager.getDefaultModel() && !checked.getModel(manager.getDefaultProvider(), manager.getDefaultModel())) throw new PiConfigError("This would remove the global default model. Choose another default before saving.");
      await writeModels(providers);
      if (key) {
        try {
          await AuthStorage.create(authPath).modify(provider, async current => ({type:'api_key', key,
            ...(current?.type === 'api_key' && current.env ? {env:current.env} : {})}));
          await chmod(authPath, 0o600);
        } catch { throw new PiConfigError("Connection saved, but its API key could not be saved. Reload and enter the key again."); }
      }
    } else if (operation === "enabled-models") {
      const manager = settings();
      if (manager.drainErrors().length) throw new PiConfigError("Unable to read pi settings. Configuration was not changed.");
      const all = (await runtime()).getModels().map(model => `${model.provider}/${model.id}`);
      let patterns;
      if (request.all === true) patterns = undefined;
      else {
        if (!Array.isArray(request.enabled) || request.enabled.length > 2000) throw new PiConfigError("Choose the models to enable.");
        const known = new Set(all);
        const refs = [...new Set(request.enabled.map(ref => typeof ref === "string" ? ref.trim() : "").filter(Boolean))];
        if (refs.some(ref => !known.has(ref))) throw new PiConfigError("The model list changed. Reload the page and try again.");
        patterns = refs.length === all.length && all.every(ref => refs.includes(ref)) ? undefined : refs;
      }
      manager.setEnabledModels(patterns);
      await manager.flush();
      if (manager.drainErrors().length) throw new PiConfigError("Unable to save pi settings. Try again.");
      await chmod(join(directory, "settings.json"), 0o600);
    } else if (operation === "import-connection") {
      let parsed;
      try { parsed = JSON.parse(request.text ?? ""); }
      catch { throw new PiConfigError("Paste a provider JSON object. Nothing was changed."); }
      const incoming = parsed?.providers ?? parsed;
      if (!incoming || typeof incoming !== "object" || Array.isArray(incoming)) throw new PiConfigError("Paste a provider JSON object. Nothing was changed.");
      const names = Object.keys(incoming);
      if (!names.length) throw new PiConfigError("Paste a provider JSON object. Nothing was changed.");
      for (const name of names) providerId(name);
      const config = await validateModels(modelsPath);
      const providers = Object.fromEntries(config.providers);
      for (const name of names) providers[name] = incoming[name];
      const checked = await writeModels(providers, true);
      const manager = settings();
      const defaultProvider = manager.getDefaultProvider();
      const defaultModel = manager.getDefaultModel();
      if (defaultProvider && defaultModel && providers[defaultProvider]
        && !checked.getModel(defaultProvider, defaultModel)) {
        throw new PiConfigError("This import would remove the global default model. Choose another default before importing.");
      }
      await writeModels(providers);
    } else if (operation === "remove-connection") {
      const provider = providerId(request.provider);
      const config = await validateModels(modelsPath);
      const providers = Object.fromEntries(config.providers);
      const current = providers[provider];
      if ((request.expectedRevision ?? null) !== (current ? connectionRevision(current) : null)) throw new PiConfigError("Connection changed while this page was open. Reload before saving.");
      if (settings().getDefaultProvider() === provider) throw new PiConfigError("This connection provides the global default model. Choose another default before removing it.");
      delete providers[provider];
      await writeModels(providers);
      await AuthStorage.create(authPath).delete(provider).catch(() => {});
    } else if (operation === "custom-model") {
      const provider = providerId(request.provider);
      const baseUrl = text(request.baseUrl, "Base URL");
      const url = new URL(baseUrl);
      if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) throw new PiConfigError("Enter an HTTP(S) API URL without a username or password.");
      const api = text(request.api, "API Type");
      if (!["openai-completions", "openai-responses", "anthropic-messages"].includes(api)) throw new PiConfigError("Select a supported API type.");
      const id = text(request.model, "Model ID");
      const config = await validateModels(modelsPath);
      const providers = Object.fromEntries(config.providers);
      const current = providers[provider] ?? {};
      const models = [...(current.models ?? [])];
      const index = models.findIndex(model => model.id === id);
      const model = { ...(index >= 0 ? models[index] : {}), id, name: request.name?.trim() || id };
      if (index >= 0) models[index] = model; else models.push(model);
      providers[provider] = { ...current, baseUrl, api, models };
      await writeModels(providers);
    } else if (operation === "import") {
      if (resolve(sourceDirectory) === resolve(directory)) throw new PiConfigError("The source is already the Zepi pi configuration directory.");
      if (!(await Promise.all(["models.json", "auth.json", "settings.json"].map(name => access(join(sourceDirectory, name)).then(() => true, () => false)))).some(Boolean)) {
        throw new PiConfigError("Local pi configuration was not found. Nothing was imported.");
      }
      const sourceSettings = SettingsManager.create(sourceDirectory, sourceDirectory, { projectTrusted: false });
      const manager = settings();
      if (sourceSettings.drainErrors().length || manager.drainErrors().length) throw new PiConfigError("Unable to read pi settings. Nothing was imported.");
      const sourceConfig = await validateModels(join(sourceDirectory, "models.json"));
      const sourceAuth = new ReadOnlyAuthStorage(join(sourceDirectory, "auth.json"));
      const credentials = await sourceAuth.list();
      await new ReadOnlyAuthStorage(authPath).list();
      const destination = await validateModels(modelsPath);
      const mergedProviders = Object.fromEntries(destination.providers);
      for (const [id, config] of sourceConfig.providers) {
        const current = mergedProviders[id] ?? {};
        const models = new Map((current.models ?? []).map(model => [model.id, model]));
        for (const model of config.models ?? []) models.set(model.id, { ...models.get(model.id), ...model });
        mergedProviders[id] = { ...current, ...config, ...(models.size ? { models: [...models.values()] } : {}) };
      }
      // Validate all input files and the resulting defaults before changing any
      // destination file. The source credential store is strictly read-only.
      const checked = await writeModels(mergedProviders, true);
      const provider = sourceSettings.getDefaultProvider();
      const modelId = sourceSettings.getDefaultModel();
      const thinking = sourceSettings.getDefaultThinkingLevel();
      if (provider && modelId) {
        const model = checked.getModel(provider, modelId);
        if (!model) throw new PiConfigError("The local pi default model is missing from the merged catalog. Nothing was imported.");
        if (thinking && !getSupportedThinkingLevels(model).includes(thinking)) throw new PiConfigError("The local default model does not support its thinking level. Nothing was imported.");
      }
      const importedCredentials = credentials.map(entry => [entry.providerId, readStoredCredential(entry.providerId, join(sourceDirectory, "auth.json"))]);
      await writeModels(mergedProviders);
      const target = AuthStorage.create(authPath);
      for (const [id, credential] of importedCredentials) {
        if (credential) await target.modify(id, async () => credential);
      }
      if (provider && modelId) manager.setDefaultModelAndProvider(provider, modelId);
      if (thinking) manager.setDefaultThinkingLevel(thinking);
      await manager.flush();
      if (manager.drainErrors().length) throw new PiConfigError("Unable to import default model settings. Try again.");
      for (const name of ["auth.json", "settings.json"]) {
        await chmod(join(directory, name), 0o600).catch(error => { if (error.code !== "ENOENT") throw error; });
      }
    } else if (operation === "check-update") {
      const update = await checkPiUpdate();
      await atomicJson(join(directory, "pi-update.json"), update);
    } else if (operation !== "status") {
      throw new PiConfigError("Unsupported pi settings operation.");
    }
    return status(operation === 'status' && request.refreshModels === true);
  }
  return { execute, status };
}

export async function checkPiUpdate(fetcher = fetch) {
  const checkedAt = new Date().toISOString();
  try {
    const response = await fetcher("https://registry.npmjs.org/@earendil-works%2fpi-coding-agent/latest", {
      signal: AbortSignal.timeout(8000), redirect: "error", headers: { accept: "application/json" },
    });
    if (!response.ok) throw new PiConfigError("registry request failed");
    const data = await response.json();
    const versionPattern = /^(\d+)\.(\d+)\.(\d+)$/;
    if (data.name !== "@earendil-works/pi-coding-agent" || !versionPattern.test(data.version)) throw new PiConfigError("invalid release metadata");
    const a = data.version.split(".").map(Number); const b = bundledVersion.split(".").map(Number);
    const difference = a.map((n, i) => n - b[i]).find(n => n !== 0) ?? 0;
    return { status: difference > 0 ? "update_available" : difference < 0 ? "ahead" : "up_to_date",
      current: bundledVersion, latest: data.version, checkedAt };
  } catch {
    return { status: "error", current: bundledVersion, checkedAt, message: "Unable to reach the update service. Try again later." };
  }
}

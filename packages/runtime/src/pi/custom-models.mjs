import {createHash} from 'node:crypto';

class ConnectionError extends Error { name = 'ConnectionError'; }
const fail = message => { throw new ConnectionError(message); };
export const connectionRevision = connection => createHash('sha256').update(JSON.stringify(connection)).digest('hex');
export function projectConnections(providers, runtime) {
  return [...providers].map(([id, config]) => ({id, baseUrl:config.baseUrl ?? '', api:config.api ?? 'openai-completions',
    models:(config.models ?? []).map(model => ({id:model.id, name:model.name ?? model.id})),
    credentialConfigured:runtime.hasConfiguredAuth(id), revision:connectionRevision(config)}));
}
export function normalizeModelIds(value) {
  if (!Array.isArray(value) || value.length > 500 || value.some(id => typeof id !== 'string' || id.length > 8192)) fail('Enter between 1 and 500 model IDs, separated by commas or new lines.');
  const ids = [...new Set(value.flatMap(id => id.split(/[,，\r\n]/)).map(id => id.trim()).filter(Boolean))];
  if (!ids.length || ids.length > 500 || ids.some(id => id.length > 512 || /[\x00-\x1f]/.test(id))) fail('Enter between 1 and 500 model IDs, separated by commas or new lines.');
  return ids;
}
export function connectionEndpoint(baseUrl, api) {
  if (!['openai-completions','openai-responses','anthropic-messages'].includes(api)) fail('Select a supported API type.');
  let url;
  try { url = new URL(baseUrl); } catch { fail('Enter an HTTP(S) base URL without credentials, query or fragment.'); }
  if (!['http:','https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) fail('Enter an HTTP(S) base URL without credentials, query or fragment.');
  let path = url.pathname.replace(/\/+$/, '');
  if (!path && api === 'anthropic-messages') path = '/v1';
  url.pathname = path + '/models';
  return url;
}
export function resolveDraftKey(value) {
  const key = value.trim();
  if (key.length > 8192 || /[\r\n]/.test(key) || key.startsWith('!')) fail('Use an API key or a $ENV_VAR reference for this connection.');
  if (key.startsWith('$')) {
    if (!/^\$[A-Za-z_][A-Za-z0-9_]*$/.test(key) || !process.env[key.slice(1)]) fail('Unable to resolve connection credentials.');
    return process.env[key.slice(1)];
  }
  return key;
}
const THINKING_LEVELS = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
const NON_CHAT_TYPES = new Set(['embedding', 'decision', 'image', 'audio', 'rerank', 'classifier']);

// Providers list chat models next to embeddings and classifiers; those never reach the model list.
const isChatModel = item => {
  const type = typeof item?.type === 'string' ? item.type.toLowerCase() : '';
  return !NON_CHAT_TYPES.has(type);
};

// `thinkingLevelMap` mirrors pi's levels; every level the provider does not offer maps to null.
const thinkingMap = (efforts, mandatory) => Object.fromEntries(THINKING_LEVELS.map(level => {
  if (level === 'off') return ['off', mandatory ? null : 'off'];
  return [level, efforts.includes(level) ? level : null];
}));

// Spec fields a provider can report. Anything missing stays absent so pi keeps its defaults.
export const modelSpec = item => {
  const spec = {};
  const context = Number(item?.context_length ?? item?.context_window ?? item?.architecture?.context_length);
  if (Number.isFinite(context) && context > 0) spec.contextWindow = context;
  const maxTokens = Number(item?.max_completion_tokens ?? item?.top_provider?.max_completion_tokens);
  if (Number.isFinite(maxTokens) && maxTokens > 0) spec.maxTokens = maxTokens;
  const reasoning = item?.reasoning;
  const efforts = Array.isArray(reasoning?.supported_efforts)
    ? reasoning.supported_efforts.filter(effort => typeof effort === 'string')
    : [];
  if (efforts.length) {
    spec.reasoning = true;
    spec.thinkingLevelMap = thinkingMap(efforts, reasoning.mandatory === true);
  }
  const modalities = item?.architecture?.input_modalities ?? item?.input_modalities;
  if (Array.isArray(modalities)) {
    const input = modalities.filter(value => value === 'text' || value === 'image');
    if (input.length) spec.input = input;
  }
  return spec;
};

const SPEC_FIELDS = ['contextWindow', 'maxTokens', 'reasoning', 'thinkingLevelMap', 'input'];

export function specEntry(id, name, raw) {
  const spec = {};
  for (const field of SPEC_FIELDS) if (raw && raw[field] !== undefined) spec[field] = raw[field];
  return {...spec, id, name: typeof name === 'string' && name.trim() ? name : id};
}

export async function fetchConnectionModels(endpoint, api, key, resolvedAuth, customHeaders, timeoutMs = 10_000) {
  const headers = {Accept:'application/json'};
  // Saved headers stay inside the runtime and use pi's literal/$ENV convention.
  for (const [name, value] of Object.entries(customHeaders ?? {})) {
    if (typeof value !== 'string' || value.startsWith('!')) fail('Unable to resolve connection credentials.');
    headers[name] = value.startsWith('$') ? resolveDraftKey(value) : value;
  }
  const auth = resolvedAuth?.auth;
  const token = key ?? (typeof auth === 'string' ? auth : auth?.apiKey);
  if (token) {
    if (api === 'anthropic-messages') headers['x-api-key'] = token;
    else headers.Authorization = `Bearer ${token}`;
  }
  if (api === 'anthropic-messages') headers['anthropic-version'] = '2023-06-01';
  try {
    const response = await fetch(endpoint, {method:'GET',headers,redirect:'error',signal:AbortSignal.timeout(timeoutMs)});
    if (response.status === 401 || response.status === 403) fail('Model list authentication failed. Check this connection’s credentials.');
    if (response.status === 429) fail('Model list rate limited. Try again later.');
    if (!response.ok) fail('Unable to fetch models. Your draft was not changed.');
    const reader = response.body?.getReader();
    if (!reader) fail('The model list response has an unsupported format.');
    const chunks = []; let bytes = 0;
    while (true) {
      const {done,value} = await reader.read(); if (done) break;
      bytes += value.byteLength;
      if (bytes > 1_048_576) { await reader.cancel(); fail('The model list is too large. Enter model IDs manually.'); }
      chunks.push(value);
    }
    let body;
    try { body = JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { fail('The model list response has an unsupported format.'); }
    if (!Array.isArray(body.data) || body.data.some(item => !item || typeof item.id !== 'string' || !item.id.trim() || item.id.length > 512)) fail('The model list response has an unsupported format.');
    const candidates = [...new Map(body.data
      .filter(item => isChatModel(item))
      .map(item => [item.id, {id:item.id, ...(typeof item.name === 'string' ? {name:item.name.slice(0,256)} : {}), ...modelSpec(item)}]))
      .values()].slice(0,500);
    const incomplete = body.data.length > 500 || body.has_more === true || !!body.next || !!body.next_page || !!body.next_cursor || !!body.links?.next;
    return {candidates, incomplete};
  } catch (error) {
    if (error instanceof ConnectionError) throw error;
    if (error?.name === 'TimeoutError' || error?.name === 'AbortError') fail('Model list request timed out. Your draft was not changed.');
    fail('Unable to fetch models. Redirects are not allowed; your draft was not changed.');
  }
}

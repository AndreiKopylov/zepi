const get = (value,path) => (Array.isArray(path) ? path : path.split('.')).reduce((v,key)=>v?.[key],value);
export class PiRuntimeSettingsError extends Error {}
const modelFields = ['thinkingLevel', 'reserveTokens', 'keepRecentTokens'];
function modelValues(settings, key) {
  const compaction = settings.compaction?.modelOverrides?.[key];
  return {thinkingLevel: settings.modelThinkingLevels?.[key] ?? null,
    reserveTokens: compaction?.reserveTokens ?? null, keepRecentTokens: compaction?.keepRecentTokens ?? null};
}
export function modelSettings(manager, model) {
  const key = `${model.provider}/${model.id}`;
  return {values: modelValues(manager.getGlobalSettings(), key), effective: {
    reserveTokens: manager.getCompactionReserveTokens(model),
    keepRecentTokens: manager.getCompactionKeepRecentTokens(model),
  }};
}
export function mergeModelSettings(current, key, changes, expected, levels) {
  if (!changes || typeof changes !== 'object' || Array.isArray(changes)
    || !expected || typeof expected !== 'object' || Array.isArray(expected)) throw new PiRuntimeSettingsError('Invalid model settings request.');
  const original = modelValues(current, key);
  for (const [field, value] of Object.entries(changes)) {
    if (!modelFields.includes(field)) throw new PiRuntimeSettingsError('Unknown model setting.');
    if (value !== null && (field === 'thinkingLevel' ? !levels.includes(value)
      : !Number.isSafeInteger(value) || value < 0 || value > 2_147_483_647)) {
      throw new PiRuntimeSettingsError('Use a supported thinking level or non-negative whole token count.');
    }
    if (!Object.hasOwn(expected, field) || JSON.stringify(original[field]) !== JSON.stringify(expected[field])) {
      throw new PiRuntimeSettingsError('Model settings changed while this page was open. Reload before saving.');
    }
  }
  return mergePaths(current, Object.entries(changes).map(([field, value]) => [
    field === 'thinkingLevel' ? ['modelThinkingLevels', key] : ['compaction', 'modelOverrides', key, field], value,
  ]));
}

// Keep model IDs containing dots or slashes as a single JSON key.
function mergePaths(current, updates) {
  const result = structuredClone(current);
  for (const [keys, value] of updates) {
    if (value === null && get(result, keys) === undefined) continue;
    let target = result;
    const parents = [];
    for (const key of keys.slice(0, -1)) {
      if (target[key] !== undefined && (!target[key] || typeof target[key] !== 'object' || Array.isArray(target[key]))) {
        throw new PiRuntimeSettingsError('A settings section has an invalid format. Repair settings.json before saving.');
      }
      parents.push([target, key]); target = target[key] ??= {};
    }
    if (value === null) delete target[keys.at(-1)]; else target[keys.at(-1)] = value;
    if (value === null) for (const [parent, key] of parents.reverse()) {
      if (!Object.keys(parent[key]).length) delete parent[key]; else break;
    }
  }
  return result;
}

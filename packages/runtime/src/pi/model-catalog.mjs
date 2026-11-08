// Pi owns provider filtering, credentials and the persisted model catalog.
// Network refresh is explicit or background; offline starts use the cached catalog.
export async function refreshModelCatalogs(runtime, force = false) {
  if (process.env.PI_OFFLINE !== undefined) return 'Model catalogs were not refreshed (PI_OFFLINE); using cached models.';
  const result = await runtime.refresh({allowNetwork: true, force,
    signal: AbortSignal.timeout(15_000)});
  if (result.aborted || result.errors.size) {
    return 'Model catalogs could not be refreshed; using cached models.';
  }
  return 'Model catalogs refreshed.';
}

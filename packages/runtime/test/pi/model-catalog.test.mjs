import assert from 'node:assert/strict';
import {test} from 'node:test';
import {refreshModelCatalogs} from '../../src/pi/model-catalog.mjs';

test('offline mode skips the network catalog refresh', async () => {
  const previous = process.env.PI_OFFLINE;
  process.env.PI_OFFLINE = '1';
  try {
    assert.match(await refreshModelCatalogs({}, true), /PI_OFFLINE/);
  } finally {
    if (previous === undefined) delete process.env.PI_OFFLINE; else process.env.PI_OFFLINE = previous;
  }
});

test('forced refresh tolerates provider failures and keeps the cached catalog', async () => {
  let seen;
  const ok = {refresh: async options => {seen = options; return {aborted: false, errors: new Map()};}};
  assert.equal(await refreshModelCatalogs(ok, true), 'Model catalogs refreshed.');
  assert.equal(seen.allowNetwork, true);
  assert.equal(seen.force, true);
  assert.ok(seen.signal);
  const broken = {refresh: async () => ({aborted: false, errors: new Map([['x', 'error']])})};
  assert.match(await refreshModelCatalogs(broken), /could not be refreshed/);
});

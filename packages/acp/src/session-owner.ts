import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import {RequestError} from '@agentclientprotocol/sdk';
// @ts-ignore CJS package without bundled types
import lockfileModule from 'proper-lockfile';

const lockfile = lockfileModule as {
  lock(path: string, options: object): Promise<() => Promise<void>>;
};

const OWNERSHIP_ERROR = 'This task is already open in another Zepi window or process. Continue there, or close the task there before reopening it here.';
const HEARTBEAT_MS = 20_000;
const STALE_MS = 120_000;

type OwnershipRecord = { pid: number; token: string; updatedAt?: number };

const owned = new Map<string, string>();
let heartbeat: ReturnType<typeof setInterval> | undefined;

async function writeOwnershipRecord(path: string, token: string) {
  const temp = `${path}.${token}.tmp`;
  try {
    await writeFile(temp, JSON.stringify({ pid: process.pid, token, updatedAt: Date.now() }), { mode: 0o600 });
    await rename(temp, path);
  } finally {
    await rm(temp, { force: true });
  }
}

function stopHeartbeat(path: string) {
  owned.delete(path);
  if (owned.size > 0 || !heartbeat) return;
  clearInterval(heartbeat);
  heartbeat = undefined;
}

function startHeartbeat(path: string, token: string) {
  owned.set(path, token);
  heartbeat ??= setInterval(() => {
    for (const [ownerPath, ownerToken] of owned) void refreshOwnership(ownerPath, ownerToken);
  }, HEARTBEAT_MS);
  heartbeat.unref();
}

// Stops beating as soon as the record belongs to someone else
async function refreshOwnership(path: string, token: string) {
  let current: OwnershipRecord | undefined;
  try { current = JSON.parse(await readFile(path, 'utf8')) as OwnershipRecord; }
  catch { current = undefined; }
  if (current?.token !== token) { stopHeartbeat(path); return; }
  await writeOwnershipRecord(path, token).catch(() => {});
}

/** A session's journal, deliveries and tools have exactly one live ACP owner.
 * Ownership is kept alive by a heartbeat: a crashed, aborted or zombie process
 * stops refreshing it, so the next window can take the task over instead of
 * reporting it as open forever. The filesystem mutex only serializes claim/release.
 */
export function sessionOwners(directory: string) {
  const root = join(directory, 'session-owners');
  const token = randomUUID();
  return async (sessionId: string) => {
    await mkdir(root, { recursive: true, mode: 0o700 });
    const path = join(root, `${createHash('sha256').update(sessionId).digest('hex')}.json`);
    async function locked<T>(fn: () => Promise<T>) {
      const release = await lockfile.lock(path, { realpath: false, stale: 10_000,
        retries: { retries: 20, minTimeout: 25, maxTimeout: 100, factor: 1.5 } });
      try { return await fn(); } finally { await release(); }
    }
    async function owner(): Promise<OwnershipRecord | undefined> {
      try { return JSON.parse(await readFile(path, 'utf8')); }
      catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return; throw error; }
    }
    await locked(async () => {
      const previous = await owner();
      if (previous) {
        if (!Number.isSafeInteger(previous.pid) || previous.pid <= 0) throw new Error('Invalid task ownership record. The task was not opened.');
        let alive = true;
        try { process.kill(previous.pid, 0); }
        catch (error) { if ((error as NodeJS.ErrnoException).code === 'ESRCH') alive = false; }
        const fresh = Date.now() - (previous.updatedAt ?? 0) < STALE_MS;
        if (alive && fresh) throw new RequestError(-32602, OWNERSHIP_ERROR);
      }
      await writeOwnershipRecord(path, token);
    });
    startHeartbeat(path, token);
    let released = false;
    return async () => {
      if (released) return;
      await locked(async () => {
        const current = await owner();
        if (current?.token === token && current.pid === process.pid) await rm(path, { force: true });
      });
      stopHeartbeat(path);
      released = true;
    };
  };
}

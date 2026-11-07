import {rename} from 'node:fs/promises';

// Windows refuses to rename onto a file somebody else holds open: pollers read these snapshots
// every few tens of milliseconds, and a virus scanner holds a freshly written file even without
// a reader. Both windows outlast one short retry, so back off up to ~0.7s before giving up.
export async function renameWithRetry(from, to) {
  for (let attempt = 0; ; attempt++) {
    try { return await rename(from, to); }
    catch (error) {
      if (attempt === 7 || !['EPERM', 'EACCES', 'EBUSY'].includes(error?.code)) throw error;
      await new Promise(resolve => setTimeout(resolve, Math.min(10 * 2 ** attempt, 200)));
    }
  }
}

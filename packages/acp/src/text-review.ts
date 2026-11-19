import {appendZepiEntry} from './session-persistence.ts';
import {randomUUID} from 'node:crypto';
import {existsSync} from 'node:fs';
import {lstat, mkdir, readFile, rm, writeFile} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import type {AgentSession} from '@earendil-works/pi-coding-agent';
import {workspacePath} from './workspace-path.ts';
import {methods, type AgentContext} from '@agentclientprotocol/sdk';

export const TEXT_REVIEW = Symbol.for('zepi.pi.textReview');
const TEXT_REVIEW_RECORD = 'zepi.text-review.v1';
const TEXT_REVIEW_METHOD = '_zepi/textReview';
const PUBLISH_TIMEOUT = 30_000;

// PowerShell output uses CRLF while editor buffers carry LF: compare as text.
const sameText = (a: Buffer, b: Buffer) =>
  a.equals(b) || a.toString('utf8').replaceAll('\r\n', '\n') === b.toString('utf8').replaceAll('\r\n', '\n');
const MAX_FILE_BYTES = 1024 * 1024;
const MAX_TOTAL_BYTES = 8 * 1024 * 1024;

/** A text shell edit. Editor buffers do not survive a reload, so the journal keeps both
 * sides of the diff and Zed rebuilds the review from them on the next open.
 */
export type TextReview = {
  sessionId: string;
  path: string;
  baseFile: string;
  currentFile: string;
  /** Disk mtime in nanoseconds; a float ms value loses the precision Zed checks. */
  mtimeNs: string | null;
  state: 'pending' | 'kept' | 'rejected';
  /** Created files have no disk file yet: the replay re-creates the editor buffer. */
  created?: boolean;
};

export function createTextReviewLedger(pi: AgentSession, cwd: string, storage: string, client: AgentContext, enabled: () => boolean) {
  const sessionDir = join(storage, pi.sessionId);
  const sessionId = pi.sessionId;
  const pending = new Map<string, TextReview>();
  const versions = new Map<string, string[]>();
  for (const entry of pi.sessionManager.getEntries()) {
    if (entry.type !== 'custom' || entry.customType !== TEXT_REVIEW_RECORD) continue;
    const review = entry.data as TextReview;
    if (review?.sessionId !== sessionId || typeof review.path !== 'string') continue;
    if (review.state === 'pending') {
      // Swept snapshots can never replay: keep the path out of the pending list.
      if (!existsSync(review.baseFile) || !existsSync(review.currentFile)) continue;
      pending.set(review.path, review);
      const files = versions.get(review.path) ?? [];
      if (!files.includes(review.currentFile)) files.push(review.currentFile);
      versions.set(review.path, files);
    } else {
      pending.delete(review.path);
      versions.delete(review.path);
    }
  }
  const publishPaths = () => {
    if (!enabled()) return;
    void client.request(TEXT_REVIEW_METHOD, {sessionId, pending: [...pending.keys()], reviews: []},
      {cancellationSignal: AbortSignal.timeout(PUBLISH_TIMEOUT)})
      .catch(error => console.error('Text review list update failed', error));
  };
  const publish = async () => {
    if (!enabled() || pending.size === 0) return;
    const paths = [...pending.keys()];
    const reviews: {path: string; base: string; current: string; diskMtimeNs?: string}[] = [];
    let total = 0;
    for (const review of pending.values()) {
      try {
        const base = await readFile(review.baseFile);
        const current = await readFile(review.currentFile);
        if (base.length > MAX_FILE_BYTES || current.length > MAX_FILE_BYTES) continue;
        total += base.length + current.length;
        if (total > MAX_TOTAL_BYTES) break;
        const canonical = await workspacePath(cwd, review.path, true);
        const disk = await readFile(canonical).then(data => data, () => undefined);
        const currentText = current.toString('utf8');
        if (disk === undefined) {
          if (!review.created) continue;
          // Nothing on disk yet: re-apply the text the way the original edit did.
          await client.request(methods.client.fs.writeTextFile, {
            sessionId, path: canonical, content: currentText,
            _meta: {'zepi.dev/createFile': true},
          }, {cancellationSignal: AbortSignal.timeout(PUBLISH_TIMEOUT)});
          continue;
        }
        const saved = disk !== undefined && sameText(disk, current);
        if (saved) {
          // The user saved the result (Keep before a restart): the review rebuilds
          // from the disk, so Keep has nothing to write over.
          const mtimeNs = await lstat(canonical, {bigint: true}).then(info => String(info.mtimeNs)).catch(() => null);
          reviews.push({path: review.path, base: base.toString('utf8'), current: currentText,
            ...(mtimeNs === null ? {} : {diskMtimeNs: mtimeNs})});
          continue;
        }
        if (review.created && disk !== undefined) {
          // An autosave can catch a stale agent version while the buffer holds the
          // final one: as long as the disk is one of the agent's own versions, the
          // review rebuilds from the disk toward the final result.
          const agentVersions = versions.get(review.path) ?? [];
          const intermediate = await Promise.all(agentVersions.map(file => readFile(file).catch(() => undefined)));
          if (intermediate.some(data => data !== undefined && sameText(data, disk))) {
            const mtimeNs = await lstat(canonical, {bigint: true}).then(info => String(info.mtimeNs)).catch(() => null);
            reviews.push({path: review.path, base: disk.toString('utf8'), current: currentText,
              ...(mtimeNs === null ? {} : {diskMtimeNs: mtimeNs})});
            continue;
          }
        }
        // The recorded mtime predates the buffer edit, which never touches the disk.
        const diskNs = await lstat(canonical, {bigint: true}).then(info => info.mtimeNs).catch(() => null);
        const untouched = review.mtimeNs !== null && diskNs !== null && diskNs === BigInt(review.mtimeNs);
        if (untouched) {
          // The classic case: the edit lives only in the editor buffer, the disk is old.
          reviews.push({path: review.path, base: base.toString('utf8'), current: currentText,
            diskMtimeNs: review.mtimeNs!});
          continue;
        }
        // The user edited the file after the review: the disk is the source of truth.
      } catch {
        // The snapshot is gone: the review cannot be rebuilt, keep it out of the list.
      }
    }
    if (reviews.length < pending.size) {
      // Partial replay means some buffers come up without their review: worth a trace.
      console.error(`zepi text review replay: only ${reviews.length}/${pending.size} rebuilt for session open`);
    }
    if (reviews.length === 0) return;
    try {
      await client.request(TEXT_REVIEW_METHOD, {sessionId, pending: paths, reviews}, {cancellationSignal: AbortSignal.timeout(PUBLISH_TIMEOUT)});
    } catch (error) {
      console.error('Text review replay failed', error);
    }
  };
  const record = async (change: {path: string; base: string; current: string; mtimeNs: string | null; created?: boolean}) => {
    // Editor buffers die with the session: both sides of the diff live next to the journal.
    const dir = join(sessionDir, randomUUID());
    const baseFile = join(dir, 'base'), currentFile = join(dir, 'current');
    try {
      await mkdir(dir, {recursive: true, mode: 0o700});
      await writeFile(baseFile, change.base, {mode: 0o600});
      await writeFile(currentFile, change.current, {mode: 0o600});
    } catch (error) {
      console.error('Could not store the text review snapshots', error);
      return;
    }
    const review: TextReview = {sessionId, path: change.path, baseFile, currentFile,
      mtimeNs: change.mtimeNs, state: 'pending', ...(change.created ? {created: true} : {})};
    pending.set(review.path, review);
    const files = versions.get(review.path) ?? [];
    if (!files.includes(currentFile)) files.push(currentFile);
    versions.set(review.path, files);
    try { appendZepiEntry(pi.sessionManager, TEXT_REVIEW_RECORD, review); }
    catch (error) { console.error('Could not journal the text review', error); }
    publishPaths();
  };
  /** Zed reports a settled review; the journal must not offer it again after a reload. */
  const resolve = (params: {path: string; state: 'kept' | 'rejected'}) => {
    const review = pending.get(params.path);
    if (!review) return {handled: false};
    pending.delete(params.path);
    try { appendZepiEntry(pi.sessionManager, TEXT_REVIEW_RECORD, {...review, state: params.state}); }
    catch (error) { console.error('Could not journal the text review', error); }
    // A settled review never replays again: every version snapshot is garbage now.
    for (const file of [review.baseFile, ...(versions.get(params.path) ?? [])]) {
      void rm(dirname(file), {recursive: true, force: true}).catch(() => {});
    }
    versions.delete(params.path);
    publishPaths();
    return {handled: true};
  };
  if (pending.size > 0) { void publish(); }
  return {record, publish, resolve};
}

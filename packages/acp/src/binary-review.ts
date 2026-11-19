import {appendZepiEntry} from './session-persistence.ts';
import {existsSync} from 'node:fs';
import {copyFile, rm} from 'node:fs/promises';
import {dirname, join} from 'node:path';
import {workspacePath} from './workspace-path.ts';
import type {AgentSession} from '@earendil-works/pi-coding-agent';
import type {AgentContext} from '@agentclientprotocol/sdk';

export const BINARY_REVIEW = Symbol.for('zepi.pi.binaryReview');
const BINARY_REVIEW_RECORD = 'zepi.binary-review.v1';
const BINARY_REVIEW_METHOD = '_zepi/binaryReview';
const RESOLVE_TIMEOUT = 30_000;

export type BinaryReview = {
  sessionId: string;
  path: string;
  kind: 'modified' | 'created';
  backup?: string;
  state: 'pending' | 'kept' | 'rejected';
};

/** Binary shell outputs (xlsx, images) have no editor buffer, so keep/reject
 * lives here instead of the buffer review. The journal is the source of truth:
 * the pending list is rebuilt from it and re-sent to Zed on every session open.
 */
export function createBinaryReviewLedger(pi: AgentSession, cwd: string, client: AgentContext, enabled: () => boolean) {
  const sessionId = pi.sessionId;
  const pending = new Map<string, BinaryReview>();
  const publish = () => {
    if (!enabled()) return;
    void client.request(BINARY_REVIEW_METHOD, {sessionId, reviews: [...pending.values()]}, {cancellationSignal: AbortSignal.timeout(RESOLVE_TIMEOUT)})
      .catch(error => console.error('Binary review update failed', error));
  };
  for (const entry of pi.sessionManager.getEntries()) {
    if (entry.type !== 'custom' || entry.customType !== BINARY_REVIEW_RECORD) continue;
    const review = entry.data as BinaryReview;
    if (review?.sessionId !== sessionId || typeof review.path !== 'string') continue;
    if (review.state === 'pending') {
      // A deleted target makes the review moot: Keep would accept nothing.
      if (!existsSync(join(cwd, review.path))) continue;
      // A swept backup cannot restore anything: hide Reject instead of failing it.
      if (review.backup && !existsSync(review.backup)) review.backup = undefined;
      pending.set(review.path, review);
    } else pending.delete(review.path);
  }
  if (pending.size > 0) publish();
  const record = (change: {path: string; kind: 'modified' | 'created'; backup?: string}) => {
    const review: BinaryReview = {sessionId, path: change.path, kind: change.kind, backup: change.backup, state: 'pending'};
    pending.set(review.path, review);
    try { appendZepiEntry(pi.sessionManager, BINARY_REVIEW_RECORD, review); }
    catch (error) { console.error('Could not journal the binary review', error); }
    publish();
  };
  const resolve = async (params: {path: string; state: 'kept' | 'rejected'}) => {
    const review = pending.get(params.path);
    if (!review) return {handled: false};
    try {
      if (params.state === 'rejected') {
        const canonical = await workspacePath(cwd, review.path, true);
        if (review.kind === 'modified') {
          if (!review.backup) return {handled: true, error: 'The pre-command backup is no longer available.'};
          await copyFile(review.backup, canonical);
        } else if (review.kind === 'created') await rm(canonical, {force: true});
      }
    } catch (error) {
      // Rejecting an open file can fail (sharing lock); keep the row so the user can retry.
      return {handled: true, error: error instanceof Error ? error.message : String(error)};
    }
    pending.delete(review.path);
    try { appendZepiEntry(pi.sessionManager, BINARY_REVIEW_RECORD, {...review, state: params.state}); }
    catch (error) { console.error('Could not journal the binary review', error); }
    // Settled: the backup can no longer be needed by anything.
    if (review.backup) void rm(review.backup, {force: true}).catch(() => {});
    publish();
    return {handled: true, path: review.path, state: params.state};
  };
  return {record, resolve};
}

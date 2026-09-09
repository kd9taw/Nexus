// Collections have sealed snapshots and cursors, not latest-value stream deltas.
// Both ends validate this closed read contract. No argument names a path or mutation.
import { validApplicationJson, APPLICATION_TIMEOUT_MS } from './application-protocol'
import type { Json } from './application-protocol'
import { streamExact, streamId } from './application-stream-protocol'

export const QUERY_COMMAND = 'get_remote_page'
export const RECALL_COMMAND = 'get_remote_recall'
export const INSIGHTS_COMMAND = 'get_remote_insights'
export const DXPEDITIONS_COMMAND = 'get_remote_dxpeditions'
export const COLLECTIONS = ['decodes', 'needs', 'spots', 'log', 'entities', 'health'] as const
export type InsightCollection = 'awards' | 'statistics'
export type Collection = typeof COLLECTIONS[number] | 'recall' | InsightCollection | 'dxpeditions'
export const QUERY_MAX_BYTES = 256 * 1024
export const QUERY_ROWS = 128
export const QUERY_MAX_ROWS = 3000
export const QUERY_ERRORS = ['applicationBusy', 'applicationUnavailable', 'applicationTooLarge', 'applicationUnsupported', 'queryExpired'] as const
export type QueryArgs = { collection: Collection; cursor: string | null; search: string; unconfirmed: boolean; after: number | null }
export type QueryRequest = QueryArgs & { type: 'applicationQuery'; requestId: string }
export type QueryPage = { type: 'applicationPage'; requestId: string; collection: Collection; snapshotId: string;
  offset: number; total: number; retained: number; nextCursor: string | null; ageMs: number; rows: Json[]; meta: Json }
export const insightCollection = (v: unknown): v is InsightCollection => v === 'awards' || v === 'statistics'
export const collection = (v: unknown, version = 3): v is Collection => COLLECTIONS.includes(v as typeof COLLECTIONS[number]) ||
  ([4, 6, 7].includes(version) && v === 'recall') || ([6, 7].includes(version) && insightCollection(v)) || (version === 7 && v === 'dxpeditions')
const integer = (v: unknown) => Number.isSafeInteger(v) && Number(v) >= 0
export function queryCursor(v: unknown): v is string {
  if (typeof v !== 'string') return false
  const parts = v.split(':')
  const [id, offset] = parts
  return parts.length === 2 && streamId(id) && /^[1-9][0-9]{0,3}$/.test(offset ?? '') && Number(offset) < QUERY_MAX_ROWS
}
export function queryRequest(v: Record<string, unknown>, version = 3): QueryRequest {
  streamExact(v, ['type', 'requestId', 'collection', 'cursor', 'search', 'unconfirmed', 'after'])
  if (v.type !== 'applicationQuery' || !streamId(v.requestId) || !collection(v.collection, version) ||
    (v.cursor !== null && !queryCursor(v.cursor)) || typeof v.search !== 'string' || new TextEncoder().encode(v.search).length > 96 ||
    /[\p{Cc}\uD800-\uDFFF]/u.test(v.search) || typeof v.unconfirmed !== 'boolean' ||
    (v.collection === 'recall' ? !/^[A-Z0-9/]{3,32}$/.test(v.search) || v.unconfirmed || v.cursor !== null :
      v.collection !== 'log' && (v.search !== '' || v.unconfirmed)) ||
    ((insightCollection(v.collection) || v.collection === 'dxpeditions') && v.cursor !== null) ||
    (v.after !== null && (v.collection !== 'decodes' || !integer(v.after)))) throw new Error('invalidApplicationQuery')
  return v as QueryRequest
}
export function queryPage(v: Record<string, unknown>, version = 3): QueryPage {
  streamExact(v, ['type', 'requestId', 'collection', 'snapshotId', 'offset', 'total', 'retained', 'nextCursor', 'ageMs', 'rows', 'meta'])
  if (v.type !== 'applicationPage' || !streamId(v.requestId) || !streamId(v.snapshotId) || !collection(v.collection, version) ||
    !integer(v.offset) || !integer(v.total) || !integer(v.retained) || Number(v.retained) > QUERY_MAX_ROWS || Number(v.total) < Number(v.retained) ||
    !integer(v.ageMs) || Number(v.ageMs) >= APPLICATION_TIMEOUT_MS || !Array.isArray(v.rows) || v.rows.length > QUERY_ROWS ||
    !validApplicationJson(v.rows) || !validApplicationJson(v.meta) ||
    Number(v.offset) + v.rows.length > Number(v.retained) ||
    (Number(v.offset) + v.rows.length < Number(v.retained)
      ? v.rows.length === 0 || v.nextCursor !== `${v.snapshotId}:${Number(v.offset) + v.rows.length}`
      : v.nextCursor !== null) ||
    new TextEncoder().encode(JSON.stringify(v)).length > QUERY_MAX_BYTES) throw new Error('invalidApplicationPage')
  return v as QueryPage
}

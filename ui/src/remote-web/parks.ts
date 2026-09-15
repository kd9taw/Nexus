// The station's own offline park directory, searched for the log form. Only the
// local index is read: no live POTA fetch, download or import is reachable here.
import type { Park } from '../api'
import type { QueryPage } from './application-query-protocol'

export const PARK_ROWS = 12
const text = (v: unknown, max = 256): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= max && !/[\uD800-\uDFFF]/u.test(v)
const count = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
function exact(v: unknown, keys: string[]): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v) || Object.keys(v).length !== keys.length || !keys.every(k => Object.prototype.hasOwnProperty.call(v, k))) throw new Error('invalidParks')
  return v as Record<string, unknown>
}
function park(v: unknown): Park {
  const p = exact(v, ['reference', 'name', 'grid', 'location', 'latitude', 'longitude'])
  // The local index never carries coordinates; only the live lookup did, and it is not connected.
  if (!text(p.reference, 32) || !p.reference || !text(p.name) || !text(p.grid, 16) || !text(p.location) || p.latitude !== null || p.longitude !== null) throw new Error('invalidParks')
  return p as unknown as Park
}
export function parseParks(page: QueryPage): { parks: Park[]; exact: Park | null; parkCount: number } {
  const meta = page.meta as Record<string, unknown> | null
  if (page.collection !== 'parks' || page.offset !== 0 || page.nextCursor !== null || page.rows.length > PARK_ROWS ||
    page.total !== page.rows.length || page.retained !== page.rows.length ||
    !meta || !count(meta.capturedAgeMs) || Number(meta.capturedAgeMs) >= 60_000) throw new Error('invalidParks')
  const source = exact(meta.source, ['parkCount', 'exact'])
  if (!count(source.parkCount)) throw new Error('invalidParks')
  return { parks: page.rows.map(park), exact: source.exact === null ? null : park(source.exact), parkCount: source.parkCount }
}

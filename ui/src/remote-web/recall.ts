import type { LoggedQso } from '../types'
import type { CallHistory, EntitySlots } from '../features/callHistory'
import type { QueryPage } from './application-query-protocol'

export type Recall = { call: string; entity: string | null; rows: LoggedQso[];
  history: Omit<CallHistory, 'qsos' | 'dupeThisBand'>; slots: EntitySlots;
  workedBandModes: [string, string][]; latestNote: string | null }
const object = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const count = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
const string = (v: unknown): v is string => typeof v === 'string' && v.length <= 128
const strings = (v: unknown): v is string[] => Array.isArray(v) && v.length <= 512 && v.every(string)
const optional = (v: unknown) => v == null || typeof v === 'string'
export function parseRecall(page: QueryPage, call: string): Recall {
  const source = object(page.meta) ? page.meta.source : null
  if (!object(source) || source.call !== call || !optional(source.entity) || !object(source.history) || !object(source.slots)) throw new Error('invalidRecall')
  const h = source.history, s = source.slots
  if (page.collection !== 'recall' || page.offset !== 0 || page.nextCursor !== null || page.rows.length > 20 || page.retained !== page.rows.length ||
    !count(h.count) || h.count !== page.total || h.count < page.rows.length || h.workedBefore !== (h.count > 0) ||
    !count(h.confirmedCount) || h.confirmedCount > h.count || (h.count > 0 ? !count(h.lastUnix) || page.rows.length === 0 : h.lastUnix !== null) ||
    !strings(h.bands) || !strings(h.modes) || !strings(s.bandsWorked) || !strings(s.modesWorked) ||
    typeof s.workedEver !== 'boolean' || typeof s.bandUnknown !== 'boolean' || !optional(source.latestNote) ||
    !Array.isArray(source.workedBandModes) || source.workedBandModes.length > 512 || !source.workedBandModes.every(p => Array.isArray(p) && p.length === 2 && p.every(string)) ||
    !page.rows.every(q => object(q) && typeof q.call === 'string' && q.call.trim().toUpperCase() === call && count(q.whenUnix) &&
      string(q.band) && string(q.mode) && typeof q.confirmed === 'boolean' && typeof q.freqMhz === 'number' && Number.isFinite(q.freqMhz) &&
      [q.name, q.qth, q.grid, q.notes, q.comment, q.rstSent, q.rstRcvd].every(optional))) throw new Error('invalidRecall')
  if (page.rows.some((q, i) => (q as unknown as LoggedQso).whenUnix > Number(h.lastUnix) ||
    (i > 0 && (q as unknown as LoggedQso).whenUnix > (page.rows[i - 1] as unknown as LoggedQso).whenUnix))) throw new Error('invalidRecall')
  return { ...source, rows: page.rows } as unknown as Recall
}

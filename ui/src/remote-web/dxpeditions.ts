import type { CalendarEntry, DxpedDashboard, DxpedWindow, PropagationSnapshot, WorkableCard, BandOutlook } from '../types'
import type { QueryPage } from './application-query-protocol'

export type Dxpeditions = Pick<PropagationSnapshot, 'dxpeditions' | 'source' | 'asOf'> & {
  capturedAgeMs: number; sourceAgeMs: number; windows: DxpedWindow[] | null; windowAgeMs: number | null; windowValidForMs: number | null
}
const object = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const number = (v: unknown, max: number) => typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= max
const integer = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
const text = (v: unknown): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= 1024
const fields = (v: Record<string, unknown>, names: string[]) => names.every(k => text(v[k]))
const list = (v: unknown, check: (v: unknown) => boolean, max = 256): v is unknown[] => Array.isArray(v) && v.length <= max && v.every(check)
const date = (v: unknown) => integer(v) && v <= 8_640_000_000_000
const optionalDate = (v: unknown) => v == null || date(v)
const geo = (v: Record<string, unknown>) => number(v.bearingDeg, 360) && number(v.distanceKm, 41000)
function outlook(v: unknown): v is BandOutlook {
  return object(v) && fields(v, ['band', 'workability', 'window']) && number(v.score, 1) && typeof v.grayline === 'boolean' &&
    list(v.hourly, h => number(h, 1), 24) && v.hourly.length === 24 && number(v.reliability, 100) &&
    (v.modeNow === undefined || list(v.modeNow, m => object(m) && text(m.mode) && number(m.score, 1), 16))
}
function card(v: unknown): v is WorkableCard {
  return object(v) && fields(v, ['call', 'entity', 'band', 'octant', 'likelihood', 'howToCall', 'windowHint', 'need', 'status']) && geo(v) &&
    ['Atno', 'NewBand', 'NewMode', 'Confirm', 'Satisfied'].includes(String(v.need)) &&
    ['WorkNow', 'OpeningPredicted', 'NotOpen'].includes(String(v.status)) && number(v.likelihoodScore, 1) &&
    typeof v.liveConfirmed === 'boolean' && integer(v.priority) &&
    (v.ft8Mode == null || (typeof v.ft8Mode === 'string' && ['FoxHound', 'Mshv', 'SuperFox'].includes(v.ft8Mode))) &&
    (v.modes === undefined || list(v.modes, text, 16))
}
function entry(v: unknown): v is CalendarEntry {
  return object(v) && fields(v, ['call', 'entity', 'region', 'octant', 'best']) && geo(v) && date(v.startUnix) && date(v.endUnix) &&
    Number(v.endUnix) >= Number(v.startUnix) && list(v.bands, text, 32) && list(v.modes, text, 16) && list(v.outlook, outlook, 16) &&
    (v.website == null || (typeof v.website === 'string' && new TextEncoder().encode(v.website).length <= 2048))
}
function window(v: unknown): v is DxpedWindow {
  return object(v) && fields(v, ['call', 'engine', 'best']) && list(v.outlook, outlook, 16) &&
    optionalDate(v.startUnix) && optionalDate(v.endUnix) &&
    (v.days === undefined || list(v.days, d => object(d) && date(d.dayUnix) && text(d.best) && number(d.score, 1), 10))
}
export function parseDxpeditions(page: QueryPage): Dxpeditions {
  const meta: unknown = page.meta
  const v = object(meta) ? meta.source : null
  if (page.collection !== 'dxpeditions' || page.offset !== 0 || page.total !== 0 || page.retained !== 0 || page.rows.length !== 0 || page.nextCursor !== null ||
    !object(meta) || !integer(meta.capturedAgeMs) || meta.capturedAgeMs >= 60_000 || !object(v) || !object(v.dxpeditions) ||
    typeof v.source !== 'string' || !['live', 'partial', 'cached'].includes(v.source) || !date(v.asOf) || Number(v.asOf) <= 0 || !integer(v.sourceAgeMs) || v.sourceAgeMs >= 300_000 ||
    !list(v.dxpeditions.active, text) || !list(v.dxpeditions.workableNow, card) || !list(v.dxpeditions.upcoming, entry) ||
    !(v.windows === null ? v.windowAgeMs === null && v.windowValidForMs === null : list(v.windows, window) && integer(v.windowAgeMs) && v.windowAgeMs < 21_600_000 &&
      integer(v.windowValidForMs) && v.windowValidForMs <= 21_600_000 - v.windowAgeMs)) throw new Error('invalidDxpeditions')
  const board = v.dxpeditions as unknown as DxpedDashboard, windows = v.windows as DxpedWindow[] | null
  const calls = new Set([...board.workableNow, ...board.upcoming].map(c => c.call))
  if (windows && (new Set(windows.map(w => w.call)).size !== windows.length || windows.some(w => !calls.has(w.call)))) throw new Error('invalidDxpeditions')
  return { dxpeditions: board, source: v.source as Dxpeditions['source'], asOf: Number(v.asOf), sourceAgeMs: v.sourceAgeMs,
    capturedAgeMs: meta.capturedAgeMs + page.ageMs, windows, windowAgeMs: v.windowAgeMs as number | null, windowValidForMs: v.windowValidForMs as number | null }
}

// Validate complete station summaries before handing them to the existing views.
// No contact list, upload outcome or connector credential is part of this read.
import type { AwardSummary, GeoLogStats } from '../types'
import { compareTallies, type LogStats } from '../features/logStats'
import type { InsightCollection, QueryPage } from './application-query-protocol'

export type Insights = { logCount: number; capturedAgeMs: number } & (
  { kind: 'awards'; awards: AwardSummary } | { kind: 'statistics'; statistics: LogStats; geography: GeoLogStats })
const object = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const count = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
const string = (v: unknown): v is string => typeof v === 'string' && v.length <= 512
const boolean = (v: unknown) => typeof v === 'boolean'
const fields = (v: unknown, names: string[], check: (v: unknown) => boolean = count): boolean => object(v) && names.every(k => check(v[k]))
const list = (v: unknown, check: (v: unknown) => boolean, max = 2048): boolean => Array.isArray(v) && v.length <= max && v.every(check)
const band = (v: unknown) => fields(v, ['worked', 'confirmed']) && fields(v, ['band'], string)
const need = (v: unknown) => fields(v, ['entity'], string) && object(v) && list(v.bands, string, 64)
function awards(v: unknown, total: number): v is AwardSummary {
  if (!object(v) || v.qsos !== total || !fields(v, ['qsos', 'confirmedQsos', 'dxccWorked', 'dxccConfirmed', 'dxccCredited',
    'readyToSubmit', 'slotsWorked', 'slotsConfirmed', 'fiveBandWorked', 'fiveBandConfirmed', 'satDxccWorked', 'satDxccConfirmed', 'wazWorked', 'wazConfirmed'])) return false
  return Number(v.confirmedQsos) <= total && list(v.bands, band) &&
    list(v.modes, m => fields(m, ['worked', 'confirmed']) && fields(m, ['mode'], string)) &&
    ['needed', 'slotNeeded', 'bandTargets'].every(k => list(v[k], need)) &&
    list(v.achievements, a => fields(a, ['id', 'title', 'detail', 'category'], string) && fields(a, ['current', 'target']) && fields(a, ['unlocked', 'critical'], boolean)) &&
    fields(v.honorRoll, ['currentTotal', 'confirmed', 'threshold', 'needed', 'numberOneNeeded']) && fields(v.honorRoll, ['achieved', 'numberOne'], boolean) &&
    fields(v.was, ['worked', 'confirmed', 'fiveBandWorked', 'fiveBandConfirmed']) && object(v.was) && list(v.was.needed, string, 50) &&
    fields(v.vucc, ['worked', 'confirmed', 'satWorked', 'satConfirmed']) && object(v.vucc) && list(v.vucc.bands, band) &&
    list(v.vucc.awards, a => band(a) && fields(a, ['threshold']) && fields(a, ['achieved'], boolean)) &&
    fields(v.iota, ['worked', 'confirmed', 'cardConfirmed'])
}
function tallies(v: unknown, total: number): boolean {
  if (!list(v, t => fields(t, ['label'], string) && fields(t, ['count']) && Number((t as Record<string, unknown>).count) <= total)) return false
  const rows = v as { label: string; count: number }[]
  return new Set(rows.map(r => r.label)).size === rows.length && rows.reduce((n, r) => n + r.count, 0) <= total
}
function statistics(v: unknown, total: number): v is LogStats {
  if (!object(v) || v.total !== total || !fields(v, ['uniqueCalls', 'confirmed', 'awardConfirmed', 'dxccEntities', 'hourUnknown'])) return false
  return ['uniqueCalls', 'confirmed', 'awardConfirmed', 'dxccEntities', 'hourUnknown'].every(k => Number(v[k]) <= total) &&
    ['byBand', 'byMode', 'byYear', 'byState', 'topEntities'].every(k => tallies(v[k], total)) &&
    Array.isArray(v.topEntities) && v.topEntities.length === v.dxccEntities &&
    list(v.hourUtc, count, 24) && (v.hourUtc as number[]).length === 24 &&
    (v.hourUtc as number[]).reduce((a, b) => a + b, Number(v.hourUnknown)) <= total &&
    fields(v.qsl, ['card', 'lotw', 'eqsl']) && Object.values(v.qsl as object).every(n => Number(n) <= total)
}
function geography(v: unknown, total: number): v is GeoLogStats {
  if (!object(v) || v.total !== total || !fields(v, ['resolved', 'dx', 'domestic']) || Number(v.resolved) > total || Number(v.dx) + Number(v.domestic) !== v.resolved) return false
  if (!list(v.byContinent, r => fields(r, ['continent'], string) && fields(r, ['qsos', 'entities']), 6) ||
    !list(v.byZone, r => fields(r, ['zone', 'qsos']), 40)) return false
  const continents = v.byContinent as GeoLogStats['byContinent'], zones = v.byZone as GeoLogStats['byZone']
  return continents.every(r => ['NA', 'SA', 'EU', 'AS', 'OC', 'AF'].includes(r.continent) && r.entities <= r.qsos) &&
    zones.every(r => r.zone >= 1 && r.zone <= 40) && new Set(continents.map(r => r.continent)).size === continents.length &&
    new Set(zones.map(r => r.zone)).size === zones.length && continents.reduce((n, r) => n + r.qsos, 0) <= Number(v.resolved) &&
    zones.reduce((n, r) => n + r.qsos, 0) <= Number(v.resolved)
}
export function parseInsights(page: QueryPage, kind: InsightCollection): Insights {
  const source: unknown = object(page.meta) ? page.meta.source : null
  if (page.collection !== kind || page.offset !== 0 || page.total !== 0 || page.retained !== 0 || page.rows.length !== 0 || page.nextCursor !== null ||
    !object(page.meta) || !count(page.meta.capturedAgeMs) || page.meta.capturedAgeMs >= 60_000 ||
    !object(source) || !count(source.logCount)) throw new Error('invalidInsights')
  const shared = { logCount: source.logCount, capturedAgeMs: page.meta.capturedAgeMs + page.ageMs }
  if (kind === 'awards' && awards(source.awards, source.logCount)) return { ...shared, kind, awards: source.awards }
  if (kind === 'statistics' && statistics(source.statistics, source.logCount) && geography(source.geography, source.logCount)) {
    const raw = source.statistics
    return { ...shared, kind, geography: source.geography, statistics: { ...raw,
      byBand: [...raw.byBand].sort(compareTallies), byMode: [...raw.byMode].sort(compareTallies),
      byState: [...raw.byState].sort(compareTallies), byYear: [...raw.byYear].sort((a, b) => a.label.localeCompare(b.label)),
      topEntities: [...raw.topEntities].sort(compareTallies).slice(0, 12) } }
  }
  throw new Error('invalidInsights')
}

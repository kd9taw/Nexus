// The rare-DX alerts the station's Pounce detector has raised, as one bounded read-only page
// (application v16). Rows are the desktop's own `pounce` event payloads, oldest first; the meta
// carries the station's threshold so the panel can say when Pounce is off there.
import type { QueryPage } from './application-query-protocol'
import type { PounceAlert } from '../usePounce'

/** The most raised alerts a station keeps for browsers (src-tauri pouncer.rs RECENT). */
export const POUNCE_ROWS = 64
export const POUNCE_THRESHOLDS = ['off', 'atno', 'atnoOrZone', 'atnoZoneOrState'] as const
export type PounceThreshold = typeof POUNCE_THRESHOLDS[number]
export type PounceRead = { threshold: PounceThreshold; alerts: PounceAlert[] }

const KEYS = ['call', 'band', 'mode', 'freqMhz', 'tags', 'entity', 'atUnix']
const CAPTURE_TTL_MS = 60_000
const text = (v: unknown, bytes: number, empty = false): v is string => typeof v === 'string' && (empty || v.length > 0) &&
  new TextEncoder().encode(v).length <= bytes && !/[\p{Cc}\uD800-\uDFFF]/u.test(v)
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

export function parsePounce(page: QueryPage): PounceRead {
  const meta = page.meta
  const source = object(meta) ? meta.source : null
  if (page.collection !== 'pounce' || page.offset !== 0 || page.nextCursor !== null || page.rows.length > POUNCE_ROWS ||
    page.total !== page.rows.length || page.retained !== page.rows.length || !object(meta) || !Number.isSafeInteger(meta.capturedAgeMs) ||
    Number(meta.capturedAgeMs) < 0 || Number(meta.capturedAgeMs) >= CAPTURE_TTL_MS || !object(source) || Object.keys(source).length !== 1 ||
    !POUNCE_THRESHOLDS.includes(source.threshold as PounceThreshold)) throw new Error('invalidPounce')
  for (const a of page.rows) {
    if (!object(a) || Object.keys(a).length !== KEYS.length || !KEYS.every(k => Object.prototype.hasOwnProperty.call(a, k)) ||
      !text(a.call, 32) || !text(a.band, 16) || !text(a.mode, 16) || !text(a.entity, 128, true) ||
      (a.freqMhz !== null && (typeof a.freqMhz !== 'number' || !Number.isFinite(a.freqMhz) || a.freqMhz <= 0 || a.freqMhz > 1_000_000)) ||
      !Array.isArray(a.tags) || a.tags.length > 8 || !a.tags.every(tag => text(tag, 32)) ||
      !Number.isSafeInteger(a.atUnix) || Number(a.atUnix) < 0) throw new Error('invalidPounce')
  }
  return { threshold: source.threshold as PounceThreshold, alerts: page.rows as unknown as PounceAlert[] }
}

import type { ObservedOta, OtaFeedStatus } from '../otaHunt'
import type { QueryPage } from './application-query-protocol'

export const OTA_SOURCE_TTL_MS = 15 * 60 * 1000
export const OTA_CAPTURE_TTL_MS = 60_000
const integer = (v: unknown) => Number.isSafeInteger(v) && Number(v) >= 0
const text = (v: unknown): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= 1024 && !/[\uD800-\uDFFF]/u.test(v)
function object(v: unknown, keys: string[]): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v) || Object.keys(v).length !== keys.length || !keys.every(k => Object.prototype.hasOwnProperty.call(v, k))) throw new Error('invalidOta')
  return v as Record<string, unknown>
}
const program = (v: unknown) => v === 'POTA' || v === 'SOTA'
const nullableText = (v: unknown) => v === null || text(v)
function spot(v: unknown, p: unknown): void {
  const s = object(v, ['program', 'reference', 'name', 'activator', 'freqKhz', 'mode', 'spotter', 'comment', 'grid', 'lat', 'lon', 'spotTimeUnix', 'newPark', 'bandOpen'])
  if (s.program !== p || ![s.reference, s.name, s.activator, s.mode].every(text) || !s.reference || !s.activator ||
    typeof s.freqKhz !== 'number' || !Number.isFinite(s.freqKhz) || s.freqKhz <= 0 || s.freqKhz > 1_000_000_000 ||
    ![s.spotter, s.comment, s.grid].every(nullableText) || (s.spotTimeUnix !== null && !integer(s.spotTimeUnix)) ||
    typeof s.newPark !== 'boolean' || typeof s.bandOpen !== 'boolean' ||
    (s.lat !== null && (typeof s.lat !== 'number' || !Number.isFinite(s.lat) || Math.abs(s.lat) > 90)) ||
    (s.lon !== null && (typeof s.lon !== 'number' || !Number.isFinite(s.lon) || Math.abs(s.lon) > 180))) throw new Error('invalidOta')
}
export function parseOta(page: QueryPage): ObservedOta & { capturedAgeMs: number } {
  const meta = page.meta as Record<string, unknown> | null
  if (page.collection !== 'ota' || page.offset !== 0 || page.total !== 0 || page.retained !== 0 || page.rows.length !== 0 || page.nextCursor !== null ||
    !meta || !integer(meta.capturedAgeMs) || Number(meta.capturedAgeMs) >= OTA_CAPTURE_TTL_MS) throw new Error('invalidOta')
  const value = object(meta.source, ['feeds', 'activation', 'hunt', 'parkCount', 'huntedCount'])
  if (new TextEncoder().encode(JSON.stringify(value)).length > 192 * 1024 || !integer(value.parkCount) || !integer(value.huntedCount) ||
    !Array.isArray(value.feeds) || value.feeds.length !== 2) throw new Error('invalidOta')
  value.feeds.forEach((raw, i) => {
    const feed = object(raw, ['program', 'status', 'sourceAgeMs', 'spots'])
    if (feed.program !== ['POTA', 'SOTA'][i] || !['ready', 'expired', 'unavailable'].includes(feed.status as OtaFeedStatus) ||
      !Array.isArray(feed.spots) || feed.spots.length > 512) throw new Error('invalidOta')
    if (feed.status === 'ready' || feed.status === 'expired') {
      if (!integer(feed.sourceAgeMs) || (Number(feed.sourceAgeMs) < OTA_SOURCE_TTL_MS) !== (feed.status === 'ready')) throw new Error('invalidOta')
    } else if (feed.sourceAgeMs !== null) throw new Error('invalidOta')
    if (feed.status !== 'ready' && feed.spots.length) throw new Error('invalidOta')
    feed.spots.forEach(s => spot(s, feed.program))
  })
  const activation = object(value.activation, ['program', 'reference', 'qsoCount'])
  if (!integer(activation.qsoCount) || (activation.program === null ? activation.reference !== null || activation.qsoCount !== 0 :
    !program(activation.program) || !text(activation.reference) || !activation.reference)) throw new Error('invalidOta')
  if (value.hunt !== null) {
    const hunt = object(value.hunt, ['program', 'reference', 'call'])
    if (!program(hunt.program) || !text(hunt.reference) || !hunt.reference || !text(hunt.call) || !hunt.call) throw new Error('invalidOta')
  }
  return { ...value as ObservedOta, capturedAgeMs: Number(meta.capturedAgeMs) }
}

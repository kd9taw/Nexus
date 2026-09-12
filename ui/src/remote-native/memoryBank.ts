// A display contract over the existing UI-owned bank, not another persistence
// schema or coercion/migration path. Unknown future fields require review here.
import type { MemoriesBank } from '../features/memories'

export const MEMORY_BANK_BYTES = 128 * 1024
export const MEMORY_BANK_TTL_MS = 60_000
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const number = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const positive = (v: unknown) => number(v) && v > 0
const text = (v: unknown): v is string => typeof v === 'string' && v.length <= 1024 && new TextEncoder().encode(v).length <= 1024
const named = (v: unknown) => text(v) && v.trim().length > 0
const list = (v: unknown, max: number, check: (item: unknown) => boolean): v is unknown[] => Array.isArray(v) && v.length <= max && v.every(check)
const keys = (v: Record<string, unknown>, required: string[], optional: string[] = []) =>
  required.every(k => Object.prototype.hasOwnProperty.call(v, k)) && Object.keys(v).every(k => required.includes(k) || optional.includes(k))
const optional = (v: Record<string, unknown>, key: string, check: (item: unknown) => boolean) => !Object.prototype.hasOwnProperty.call(v, key) || check(v[key])
const oneOf = (values: string[]) => (v: unknown) => typeof v === 'string' && values.includes(v)

function net(v: unknown): boolean {
  return object(v) && keys(v, ['days', 'utcTime', 'alertEnabled', 'alertLeadMin'], ['netControl', 'description', 'netloggerName']) &&
    list(v.days, 7, d => number(d) && d >= 0 && d <= 6) && text(v.utcTime) && /^\d{1,2}:\d{2}$/.test(v.utcTime) &&
    typeof v.alertEnabled === 'boolean' && positive(v.alertLeadMin) &&
    ['netControl', 'description', 'netloggerName'].every(k => optional(v, k, text))
}
function memory(v: unknown): boolean {
  return object(v) && keys(v, ['id', 'name', 'kind', 'rxMhz', 'mode', 'groups', 'favorite', 'source'],
    ['offsetDir', 'offsetMhz', 'txMhz', 'toneMode', 'ctcssEncHz', 'ctcssDecHz', 'dtcsCode', 'dtcsRxCode', 'dtcsPol',
      'notes', 'callsign', 'grid', 'lat', 'lon', 'skip', 'lastUsedUtc', 'net']) &&
    ['id', 'name', 'mode', 'source'].every(k => named(v[k])) && positive(v.rxMhz) && typeof v.favorite === 'boolean' &&
    oneOf(['repeater', 'simplex', 'hfnet', 'calling', 'pota', 'digital', 'satellite', 'emcomm', 'reference', 'other'])(v.kind) &&
    list(v.groups, 64, named) &&
    optional(v, 'offsetDir', oneOf(['simplex', 'plus', 'minus', 'split'])) &&
    optional(v, 'toneMode', oneOf(['none', 'tone', 'tsql', 'dtcs', 'cross'])) &&
    ['offsetMhz', 'txMhz', 'ctcssEncHz', 'ctcssDecHz', 'dtcsCode', 'dtcsRxCode', 'lastUsedUtc'].every(k => optional(v, k, positive)) &&
    ['dtcsPol', 'notes', 'callsign', 'grid'].every(k => optional(v, k, text)) &&
    optional(v, 'lat', x => number(x) && x >= -90 && x <= 90) && optional(v, 'lon', x => number(x) && x >= -180 && x <= 180) &&
    optional(v, 'skip', x => typeof x === 'boolean') && optional(v, 'net', net)
}
function group(v: unknown): boolean {
  return object(v) && keys(v, ['id', 'name', 'order']) && named(v.id) && named(v.name) && number(v.order)
}
export function memoryBank(v: unknown): v is MemoriesBank {
  if (!object(v) || !keys(v, ['version', 'memories', 'groups']) || v.version !== 2 ||
    !list(v.memories, 512, memory) || !list(v.groups, 64, group)) return false
  const bank = v as unknown as MemoriesBank
  if (new Set(bank.memories.map(m => m.id)).size !== bank.memories.length || new Set(bank.groups.map(g => g.id)).size !== bank.groups.length) return false
  const json = JSON.stringify(v)
  return json.length <= MEMORY_BANK_BYTES && new TextEncoder().encode(json).length <= MEMORY_BANK_BYTES
}

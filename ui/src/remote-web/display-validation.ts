// Closed schemas for station display reads. Keep native event timestamps intact;
// their capture clock is separate from the browser's wall clock and row keys.
export const integer = (v: unknown): v is number => Number.isSafeInteger(v) && Number(v) >= 0
export const finite = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
export const text = (v: unknown, max = 1024): v is string => typeof v === 'string' && new TextEncoder().encode(v).length <= max && !/[\uD800-\uDFFF]/u.test(v)
export const bool = (v: unknown) => typeof v === 'boolean'
export const nullableTime = (v: unknown) => v === null || integer(v)
export const nullableNumber = (v: unknown) => v === null || finite(v)
/** Like `object`, but tolerates keys it does not know while still REQUIRING the listed ones.
 *
 *  For a document whose shape grows with the station's version, an exact key count makes every
 *  addition a two-way break: an older station sends fewer keys and is refused, a newer one sends
 *  more and is refused too. Eleven settings drifted exactly that way and Remote's settings and
 *  Field Day surfaces went dark for every operator.
 *
 *  ⚠️ Callers MUST check separately that nothing withheld appears. With an exact count that was
 *  incidental - a leaked key made the arithmetic wrong - and relaxing the count removes it. The
 *  caller states it instead, which is a better home for a rule that load-bearing. */
export function openObject(v: unknown, keys: string[]): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v) || !keys.every(k => Object.prototype.hasOwnProperty.call(v,k))) throw new Error('invalidStationDisplay')
  return v as Record<string, unknown>
}
export function object(v: unknown, keys: string[]): Record<string, unknown> {
  if (!v || typeof v !== 'object' || Array.isArray(v) || Object.keys(v).length !== keys.length || !keys.every(k => Object.prototype.hasOwnProperty.call(v,k))) throw new Error('invalidStationDisplay')
  return v as Record<string,unknown>
}
export function rows(v: unknown, max: number): unknown[] {
  if (!Array.isArray(v) || v.length > max) throw new Error('invalidStationDisplay')
  return v
}
const clocks = new WeakMap<object,{stationAt:number;receivedAt:number}>()
export function captureClock(value: object, stationAt: number, ageMs: number, receivedAt = performance.now()): void {
  if (!integer(stationAt) || !finite(ageMs) || ageMs < 0) throw new Error('invalidStationDisplay')
  clocks.set(value,{stationAt:stationAt+ageMs,receivedAt})
}
export function displayNow(value: object|null, browserNow = performance.now()): number {
  const c = value && clocks.get(value)
  return c ? c.stationAt+Math.max(0,browserNow-c.receivedAt) : Date.now()
}

import { expect, it } from 'vitest'
import { parseOta } from './ota'
import { queryPage, type QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/ota.json'

export function otaPage(): QueryPage {
  return { type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(), collection: 'ota',
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [], meta: { capturedAgeMs: 0, source: structuredClone(fixture) } }
}
it('accepts station feed provenance and complete activation counts only through v9', () => {
  const page = otaPage()
  expect(queryPage(page, 9).collection).toBe('ota')
  for (const version of [3, 4, 5, 6, 7, 8]) expect(() => queryPage(page, version)).toThrow()
  expect(parseOta(page)).toEqual({ ...fixture, capturedAgeMs: 0 })
})
it('distinguishes successful empty, missing and expired feeds', () => {
  for (const status of ['ready', 'unavailable', 'expired']) {
    const value = structuredClone(fixture)
    value.feeds = value.feeds.map(f => ({ ...f, status, spots: [], sourceAgeMs: (status === 'ready' ? 0 : status === 'expired' ? 900_000 : null) as unknown as number }))
    const raw = { ...value, hunt: value.hunt }
    const result = parseOta({ ...otaPage(), meta: { capturedAgeMs: 10, source: raw } })
    expect(result.feeds[0].status).toBe(status)
    expect(result.feeds[0].spots).toEqual([])
  }
})
it('refuses malformed, oversized, stale and falsely fresh observations', () => {
  const page = otaPage()
  for (const patch of [{ total: 1 }, { retained: 1 }, { offset: 1 }, { rows: [{}] }, { collection: 'memories' }, { nextCursor: `${page.snapshotId}:1` }]) {
    expect(() => parseOta({ ...page, ...patch } as QueryPage)).toThrow()
  }
  for (const mutate of [
    (v: typeof fixture) => { v.feeds[0].spots[0].freqKhz = Infinity },
    (v: typeof fixture) => { v.feeds[0].spots[0].name = 'x'.repeat(1025) },
    (v: typeof fixture) => { v.feeds[0].spots = Array(513).fill(v.feeds[0].spots[0]) },
    (v: typeof fixture) => { v.feeds[0].spots[0].program = 'SOTA' },
    (v: typeof fixture) => { v.feeds[0].sourceAgeMs = 900_000 },
    (v: typeof fixture) => { v.feeds[0].status = 'expired' },
    (v: typeof fixture) => { v.activation.qsoCount = -1 },
    (v: typeof fixture) => { Object.assign(v.feeds[0].spots[0], { command: 'set_frequency' }) },
    (v: typeof fixture) => { Object.assign(v, { path: 'log.adi' }) },
  ]) {
    const value = structuredClone(fixture); mutate(value)
    expect(() => parseOta({ ...page, meta: { capturedAgeMs: 0, source: value } })).toThrow()
  }
  expect(() => parseOta({ ...page, meta: { capturedAgeMs: 60_000, source: fixture } })).toThrow()
})
/** A v18 station's board: each spot also says where the activator is and which of those states a
 *  contact would add to Worked All States on the spot's band, as the desktop board's rows do. */
function placed(states: string[][], needed: string[][]) {
  const value = structuredClone(fixture)
  value.feeds.forEach((feed, f) => feed.spots.forEach((s, i) => Object.assign(s, f ? { states: [], neededStates: [] } : { states: states[i] ?? [], neededStates: needed[i] ?? [] })))
  return value
}
const parsePlaced = (value: unknown) => parseOta({ ...otaPage(), meta: { capturedAgeMs: 0, source: value } as QueryPage['meta'] })
it('accepts each activator\'s states and the needed ones from a v18 station, and the board without them from an older one', () => {
  const value = placed([['US-ND'], ['US-MT', 'US-ND'], ['CA-ON']], [['US-ND'], ['US-ND'], []])
  expect(parsePlaced(value)).toEqual({ ...value, capturedAgeMs: 0 })
  expect(parsePlaced(value).feeds[0].spots.map(s => [s.states, s.neededStates])).toEqual([[['US-ND'], ['US-ND']], [['US-MT', 'US-ND'], ['US-ND']], [['CA-ON'], []]])
  // A station older than v18 sends neither key, and its board parses exactly as before.
  expect(parseOta(otaPage())).toEqual({ ...fixture, capturedAgeMs: 0 })
  expect(parseOta(otaPage()).feeds[0].spots.every(s => !('states' in s) && !('neededStates' in s))).toBe(true)
})
it('refuses a malformed state, a need outside the spot\'s states, either key alone, and every other key as before', () => {
  const ok = () => placed([['US-MT', 'US-ND']], [['US-ND']])
  expect(() => parsePlaced(ok())).not.toThrow()
  type Spot = Record<string, unknown>
  for (const mutate of [
    (s: Spot) => { delete s.neededStates },
    (s: Spot) => { delete s.states },
    // Each with no need named, so the code alone is what is refused.
    (s: Spot) => { s.states = 'US-ND'; s.neededStates = [] },
    (s: Spot) => { s.states = null; s.neededStates = [] },
    (s: Spot) => { s.states = ['ND']; s.neededStates = [] },
    (s: Spot) => { s.states = ['us-nd']; s.neededStates = [] },
    (s: Spot) => { s.states = ['US-NDX']; s.neededStates = [] },
    (s: Spot) => { s.states = ['MX-SON']; s.neededStates = [] },
    (s: Spot) => { s.states = [1]; s.neededStates = [] },
    (s: Spot) => { s.states = Array(65).fill('US-ND'); s.neededStates = [] },
    (s: Spot) => { s.neededStates = 'US-ND' },
    (s: Spot) => { s.neededStates = ['US-WY'] },
    (s: Spot) => { s.neededStates = ['US-ND', 'US-ND', 'US-ND'] },
    (s: Spot) => { s.command = 'set_frequency' },
    (s: Spot) => { s.statesNeeded = [] },
  ]) {
    const value = ok(); mutate(value.feeds[0].spots[0] as unknown as Spot)
    expect(() => parsePlaced(value), String(mutate)).toThrow()
  }
  // The longest list there can be is every state, DC and every province: 64 codes.
  const most = ok(); Object.assign(most.feeds[0].spots[0], { states: Array(64).fill('US-ND'), neededStates: [] })
  expect(() => parsePlaced(most)).not.toThrow()
})

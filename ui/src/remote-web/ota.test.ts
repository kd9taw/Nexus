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

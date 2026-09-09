import { expect, it } from 'vitest'
import { parseDxpeditions } from './dxpeditions'
import type { QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/dxpeditions.json'
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'dxpeditions', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 12, rows: [],
  meta: { capturedAgeMs: 20, source: structuredClone(fixture) } })
it('accepts the exact board and nullable forecasts, retaining source and transit age separately', () => {
  const p = page(), v = parseDxpeditions(p)
  expect(v.dxpeditions).toEqual(fixture.dxpeditions); expect(v.windows).toEqual(fixture.windows)
  expect(v.capturedAgeMs).toBe(32); expect(v.sourceAgeMs).toBe(2000)
  const source = (p.meta as { source: { windows: unknown; windowAgeMs: unknown; windowValidForMs: unknown } }).source
  source.windows = null; source.windowAgeMs = null; source.windowValidForMs = null
  expect(parseDxpeditions(p).windows).toBeNull()
})
it('refuses truncation, expired ages, bad shapes and unrelated forecast calls', () => {
  for (const patch of [{ total: 1 }, { rows: [{}] }, { collection: 'awards' }, { offset: 1 }]) expect(() => parseDxpeditions({ ...page(), ...patch } as QueryPage)).toThrow()
  for (const mutate of [
    (v: typeof fixture) => { v.sourceAgeMs = 300000 },
    (v: typeof fixture) => { v.source = 'offline' },
    (v: typeof fixture) => { v.windowAgeMs = 21600000 },
    (v: typeof fixture) => { v.windows[0].call = 'UNRELATED' },
    (v: typeof fixture) => { v.windows.push(v.windows[0]) },
    (v: typeof fixture) => { v.dxpeditions.workableNow[0].status = 'Unknown' },
    (v: typeof fixture) => { v.dxpeditions.upcoming[0].outlook[0].hourly = [1] },
    (v: typeof fixture) => { v.dxpeditions.upcoming[0].startUnix = Infinity },
    (v: typeof fixture) => { v.dxpeditions.upcoming[0].outlook[0].score = 2 },
  ]) {
    const p = page(); mutate((p.meta as unknown as { source: typeof fixture }).source)
    expect(() => parseDxpeditions(p)).toThrow('invalidDxpeditions')
  }
})

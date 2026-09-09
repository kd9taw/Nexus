import { expect, it } from 'vitest'
import { parseInsights } from './insights'
import { queryPage, type InsightCollection, type QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/insights.json'

export function insightPage(kind: InsightCollection = 'statistics'): QueryPage {
  return { type: 'applicationPage', collection: kind, requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
    offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
    meta: { capturedAgeMs: 0, source: { logCount: 2301, ...structuredClone(fixture) } } }
}
it('accepts complete summaries beyond the log window and keeps the existing chart ordering', () => {
  for (const kind of ['awards', 'statistics'] as const) {
    const page = insightPage(kind)
    expect(queryPage(page, 6).collection).toBe(kind)
    for (const version of [3, 4, 5]) expect(() => queryPage(page, version)).toThrow()
    const result = parseInsights(page, kind)
    expect(result.logCount).toBe(2301)
    if (result.kind === 'statistics') expect(result.statistics).toEqual(fixture.statistics)
  }
})
it('refuses mismatched, incomplete, malformed and expired insight pages', () => {
  const base = insightPage()
  for (const patch of [{ total: 2301 }, { retained: 1 }, { nextCursor: `${base.snapshotId}:1` },
    { offset: 1 }, { rows: [{}] }, { collection: 'awards' }]) {
    expect(() => parseInsights({ ...base, ...patch } as QueryPage, 'statistics')).toThrow()
  }
  for (const mutate of [
    (v: typeof fixture) => { v.statistics.total = 2000 },
    (v: typeof fixture) => { v.statistics.hourUtc = [] },
    (v: typeof fixture) => { v.statistics.byBand[0].count = 2302 },
    (v: typeof fixture) => { v.statistics.byBand.push(v.statistics.byBand[0]) },
    (v: typeof fixture) => { v.geography.domestic = -1 },
  ]) {
    const values = structuredClone(fixture); mutate(values)
    expect(() => parseInsights({ ...base, meta: { capturedAgeMs: 0, source: { logCount: 2301, ...values } } }, 'statistics')).toThrow()
  }
  expect(() => parseInsights({ ...base, meta: { capturedAgeMs: 60000, source: { logCount: 2301, ...fixture } } }, 'statistics')).toThrow()
  const broken = insightPage('awards')
  ;(broken.meta as { source: { awards: { vucc: unknown } } }).source.awards.vucc = null
  expect(() => parseInsights(broken, 'awards')).toThrow()
})

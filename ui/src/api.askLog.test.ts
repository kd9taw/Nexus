// @vitest-environment jsdom
//
// `askLog` is the transport C17b's asking source takes (SPEC-2 v3 C17a): the question goes to the
// engine as it is, under the one key the Rust command destructures (`q`), and the answer comes
// back as it is — except the statistics, which the engine counts and the window orders. Pinned
// the way the other api wrappers are: stub the bridge, record the call.
import { afterEach, beforeEach, expect, it } from 'vitest'
import { askLog } from './api'
import type { LogTransport } from './features/askingLogSource'
import { DEFAULT_LOG_QUERY } from './features/logQuery'
import { finishLogStats, type LogStatCounts } from './features/logStats'

type Call = { cmd: string; args: unknown }
let calls: Call[] = []
let answer: unknown = 24

beforeEach(() => {
  calls = []
  answer = 24
  ;(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args })
      return answer
    },
  }
})
afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

it('sends the question as it is to ask_log, and is the asking source’s transport', async () => {
  // A type-level pin: the asking source accepts it as its transport.
  const transport: LogTransport = askLog
  const page = { kind: 'page', query: DEFAULT_LOG_QUERY, offset: 0, limit: 50 } as const
  await transport({ kind: 'logSize' })
  await askLog(page)
  expect(calls).toEqual([
    { cmd: 'ask_log', args: { q: { kind: 'logSize' } } },
    { cmd: 'ask_log', args: { q: page } },
  ])
})

it('orders the statistics the engine counted, as the dashboard always ordered them', async () => {
  // Counted in first-seen order, with ties the window's locale breaks: 'ssb' and 'SSB' count the
  // same, as do the two Ålands, and there are more than twelve entities.
  const counts: LogStatCounts = {
    total: 30, uniqueCalls: 20, confirmed: 3, awardConfirmed: 1, dxccEntities: 14,
    byBand: [{ label: '40m', count: 2 }, { label: '20m', count: 5 }, { label: '20M', count: 2 }],
    byMode: [{ label: 'ssb', count: 3 }, { label: 'SSB', count: 3 }, { label: 'FT8', count: 9 }],
    byYear: [{ label: '2024', count: 1 }, { label: '2019', count: 2 }],
    byState: [{ label: 'WI', count: 1 }, { label: 'CT', count: 1 }],
    entities: [
      { label: 'Åland Islands', count: 2 }, { label: 'Aland Islands', count: 2 },
      ...Array.from({ length: 12 }, (_, i) => ({ label: `Entity ${String.fromCharCode(76 - i)}`, count: 1 })),
    ],
    hourUtc: Array.from({ length: 24 }, (_, h) => h % 3), hourUnknown: 2,
    qsl: { card: 1, lotw: 1, eqsl: 1 },
  }
  answer = counts
  const stats = await askLog({ kind: 'statistics' })
  expect(calls).toEqual([{ cmd: 'ask_log', args: { q: { kind: 'statistics' } } }])
  expect(stats).toEqual(finishLogStats(counts))
  expect(stats.topEntities).toHaveLength(12)
  expect(stats.byMode[0]).toEqual({ label: 'FT8', count: 9 })
  expect(stats.byYear.map((t) => t.label)).toEqual(['2019', '2024'])
  expect('entities' in stats, 'the counted form never reaches a view').toBe(false)
})

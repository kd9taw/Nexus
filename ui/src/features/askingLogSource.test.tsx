// @vitest-environment jsdom
//
// THE ASKING `LogSource` — the window's side of the engine's log queries (SPEC-2 v3 C17b/C17a).
// Its rules, each one pinned against a transport the test answers by hand: one request per
// question however many views show it; a new tick re-asks every shown question once, and a burst of
// ticks costs one follow-up; an answer never goes backwards (v2 R4); a failure waits for the next
// change instead of looping; a view keeps its old answer until the fresh one lands.

import { afterEach, describe, expect, it } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { createAskingLogSource, type LogTransport } from './askingLogSource'
import { questionKey, type AnswerTo, type LogQuestion } from './logAnswers'
import { setLogSource, useLogAnswer, type LogSource } from './logSource'

/** A transport whose every call waits until the test answers it. */
function handTransport() {
  const calls: { q: LogQuestion; resolve: (a: unknown) => void; reject: (e: unknown) => void }[] = []
  const transport: LogTransport = <Q extends LogQuestion>(q: Q) =>
    new Promise<AnswerTo<Q>>((resolve, reject) => {
      calls.push({ q, resolve: resolve as (a: unknown) => void, reject })
    })
  return { transport, calls }
}
const turn = () => act(() => new Promise((r) => setTimeout(r, 0)))
const size = { kind: 'logSize' } as const
const history = (call: string) => ({ kind: 'callHistory', call, band: '20m', mode: 'FT8', matchMode: false }) as const

afterEach(cleanup)

describe('the asking source', () => {
  it('asks once per question however many views want it, and keeps the answer', async () => {
    const { transport, calls } = handTransport()
    const src = createAskingLogSource(transport)
    const release = [src.want(size), src.want(size), src.want({ ...size })]
    expect(calls).toHaveLength(1)
    await act(async () => calls[0].resolve(42))
    expect(src.peek(size)).toBe(42)
    release.forEach((r) => r())
    expect(src.peek(size), 'a released answer is kept a while').toBe(42)
  })

  it('a new tick re-asks every wanted question ONCE; the same tick asks nothing; a burst costs one follow-up', async () => {
    const { transport, calls } = handTransport()
    const src = createAskingLogSource(transport)
    src.follow(1)
    src.want(size)
    src.want(history('W1AW'))
    await turn()
    expect(calls.map((c) => c.q.kind)).toEqual(['logSize', 'callHistory'])
    await act(async () => calls.splice(0).forEach((c) => c.resolve('first')))

    src.follow(1) // the same tick from another view
    await turn()
    expect(calls).toHaveLength(0)

    src.follow(2)
    src.follow(2)
    await turn()
    expect(calls.map((c) => c.q.kind)).toEqual(['logSize', 'callHistory'])
    // Two more ticks land while those are out: ONE follow-up each, after they answer.
    src.follow(3)
    src.follow(4)
    await turn()
    expect(calls).toHaveLength(2)
    await act(async () => calls.splice(0).forEach((c) => c.resolve('second')))
    await turn()
    expect(calls.map((c) => c.q.kind)).toEqual(['logSize', 'callHistory'])
    await act(async () => calls.splice(0).forEach((c) => c.resolve('third')))
    await turn()
    expect(calls).toHaveLength(0)
    expect(src.peek(size)).toBe('third')
  })

  it('an answer never goes backwards: an older request answering last is dropped (v2 R4)', async () => {
    const { transport, calls } = handTransport()
    const src = createAskingLogSource(transport)
    src.want(size)
    const older = calls[0]
    const newer = src.ask(size) // e.g. a read-once view asking fresh
    const fresh = calls[1]
    await act(async () => fresh.resolve(7))
    await newer
    await act(async () => older.resolve(5))
    expect(src.peek(size)).toBe(7)
  })

  it('a failed request waits for the next change instead of retrying in a loop', async () => {
    const { transport, calls } = handTransport()
    const src = createAskingLogSource(transport)
    src.follow(1)
    src.want(size)
    await turn()
    await act(async () => calls.splice(0)[0].reject(new Error('engine busy')))
    await turn()
    expect(calls, 'retried in a loop').toHaveLength(0)
    expect(src.peek(size)).toBeUndefined()
    src.follow(2)
    await turn()
    expect(calls).toHaveLength(1)
  })

  it('a view keeps its old answer until the fresh one lands (and has none before the first)', async () => {
    const { transport, calls } = handTransport()
    setLogSource(createAskingLogSource(transport))
    const View = ({ tick }: { tick: number }) => <p data-testid="n">{String(useLogAnswer(size, tick) ?? 'none')}</p>
    const { rerender } = render(<View tick={1} />)
    expect(screen.getByTestId('n').textContent).toBe('none')
    await turn()
    await act(async () => calls.splice(0).forEach((c) => c.resolve(10)))
    expect(screen.getByTestId('n').textContent).toBe('10')
    rerender(<View tick={2} />)
    await turn()
    expect(calls, 'the new tick was not followed').toHaveLength(1)
    expect(screen.getByTestId('n').textContent, 'the old answer must stay up meanwhile').toBe('10')
    await act(async () => calls.splice(0).forEach((c) => c.resolve(11)))
    expect(screen.getByTestId('n').textContent).toBe('11')
  })
})

// ---- The drop-in C17a relies on: the real Logbook through the ASKING source renders the list it
// renders through the whole-log source, given a transport that answers today's answers. ----
describe('the Logbook through the asking source', () => {
  it('shows the same rows, the same count and the same order as through the whole-log source', async () => {
    const { Logbook } = await import('../components/Logbook')
    const { answerFrom } = await import('./logAnswers')
    const log = Array.from({ length: 40 }, (_, i) => ({
      id: `id-${i}`, call: ['W1AW', 'k1abc', 'DL1ABC', 'JA1ABC'][i % 4] + (i % 7), grid: 'FN31', band: ['20m', '40m'][i % 2],
      freqMhz: i % 2 ? 7.074 : 14.074, mode: ['FT8', 'USB', 'CW'][i % 3], rstSent: String(-i), rstRcvd: null,
      country: ['United States', 'Japan', 'Åland Islands'][i % 3], whenUnix: 1_700_000_000 + Math.floor(i / 2) * 60,
      confirmed: i % 3 === 0, awardConfirmed: i % 6 === 0, qslRcvd: null, qslSent: null, ota: null,
    })) as unknown as import('../types').LoggedQso[]
    const rows = (root: HTMLElement) =>
      [...root.querySelectorAll('.log-rows .logbook-row')].map((r) => r.querySelector('.qrz-link-call')?.textContent ?? '…')
    const badge = (root: HTMLElement) => root.querySelector('.count-badge')?.textContent
    // A 2000 px viewport and 43 px rows (the list measures rows by `offsetHeight`), so all 40 fit.
    Object.defineProperty(HTMLElement.prototype, 'offsetHeight', {
      configurable: true,
      get() {
        return (this as HTMLElement).classList?.contains('logbook-row') ? 43 : 2000
      },
    })
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver

    // The control: today's answers held synchronously, as the whole-log source holds them — and,
    // as the contract requires, the SAME object for a question until something changes.
    const held = new Map<string, unknown>()
    const whole: LogSource = {
      peek: <Q extends LogQuestion>(q: Q) => {
        const key = questionKey(q)
        if (!held.has(key)) held.set(key, answerFrom(log, q, 1))
        return held.get(key) as AnswerTo<Q>
      },
      ask: async (q) => answerFrom(log, q, 1),
      want: () => () => {},
      follow: () => {},
      subscribe: () => () => {},
      refresh: () => {},
    }
    setLogSource(whole)
    const a = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1} />)
    const want = { rows: rows(a.container), badge: badge(a.container) }
    a.unmount()
    expect(want.rows.length, 'the control must show rows').toBe(40)

    // Through the asking source: nothing until the answers land, then the same list.
    setLogSource(createAskingLogSource(async (q) => answerFrom(log, q, 1)))
    const b = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1} />)
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20))
    })
    expect(rows(b.container)).toEqual(want.rows)
    expect(badge(b.container)).toBe(want.badge)
  })
})

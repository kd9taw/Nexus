// @vitest-environment jsdom
//
// THE WHOLE-LOG ADAPTER AND THE HOOK VIEWS READ IT THROUGH (SPEC-2 v3 C17b).
//
// Until the engine answers questions (C17a), `wholeLogSource` answers them from the window's one
// copy of the log. What makes switching a view onto it safe is pinned here:
//   - an answer is TODAY'S answer: `answerFrom` over the copy, which the goldens and the per-view
//     suites pin to the functions the views ran;
//   - it is there in the same render the copy is (`peek` is synchronous), and it is the SAME object
//     until the copy changes — `useSyncExternalStore` needs that, and every memo keyed on it;
//   - the requests the window makes do not change: one `get_log_delta` however many views start,
//     none when a view's question changes (a keystroke in a call box), one per tick;
//   - a view that is not reading (remote mode) asks for nothing.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import type { LogDelta } from '../api'
import type { LoggedQso } from '../types'
import { answerFrom, emptyAnswer, type LogQuestion } from './logAnswers'
import { DEFAULT_LOG_QUERY } from './logQuery'
import { logSource, setLogSource, useLogAnswer, type LogSource } from './logSource'
import { sharedLogRows } from './logStore'
import { wholeLogSource } from './wholeLogSource'

const api = vi.hoisted(() => ({ getLogDelta: vi.fn<(since: number, have: number) => Promise<LogDelta>>() }))
vi.mock('../api', () => ({ getLogDelta: api.getLogDelta }))

const qso = (call: string, whenUnix = 1_700_000_000, extra: Partial<LoggedQso> = {}) =>
  ({ call, band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix, grid: 'FN31', confirmed: false, awardConfirmed: false, ...extra }) as unknown as LoggedQso

const history = (call: string) => ({ kind: 'callHistory' as const, call, band: '20m', mode: 'FT8', matchMode: false })

/** What each reader last rendered, by id. */
const seen = new Map<string, unknown>()
function Reader({ id, q, tick }: { id: string; q: LogQuestion | null; tick?: number }) {
  const answer = useLogAnswer(q, tick)
  seen.set(id, answer)
  const text =
    answer === undefined ? 'none' : q?.kind === 'callHistory' ? String((answer as { count: number }).count) : JSON.stringify(answer)
  return <div data-testid={id}>{text}</div>
}
const text = (id: string) => screen.getByTestId(id).textContent
const settle = () => act(() => new Promise((r) => setTimeout(r, 30)))

afterEach(() => {
  cleanup()
  api.getLogDelta.mockReset()
  seen.clear()
})

describe('the whole-log adapter answers what the views computed', () => {
  it('has no answer before the copy loads, then today’s answer, in the same render as the copy', async () => {
    const rows = [qso('W1AW', 100), qso('K1ABC', 200), qso('w1aw', 300)]
    api.getLogDelta.mockResolvedValue({ revision: 7, full: true, rows })
    render(<Reader id="a" q={history('W1AW')} tick={3} />)
    expect(text('a')).toBe('none')
    await waitFor(() => expect(text('a')).toBe('2'))
    expect(seen.get('a')).toEqual(answerFrom(rows, history('W1AW'), 7))
    // …and it is exactly what a view computing over the shared copy got.
    expect(logSource().peek(history('W1AW'))).toEqual(answerFrom(sharedLogRows()!, history('W1AW'), 7))
  })

  it('keeps the SAME answer object until the copy changes, then a fresh one', async () => {
    const first = [qso('W1AW', 100)]
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: first })
    const { rerender } = render(<Reader id="a" q={history('W1AW')} tick={3} />)
    await waitFor(() => expect(text('a')).toBe('1'))
    const held = seen.get('a')
    rerender(<Reader id="a" q={history('W1AW')} tick={3} />)
    rerender(<Reader id="a" q={{ ...history('W1AW') }} tick={3} />) // a fresh object, the same question
    expect(seen.get('a'), 'a re-render with the log unchanged must not recompute').toBe(held)
    expect(wholeLogSource.peek(history('W1AW'))).toBe(held)

    api.getLogDelta.mockResolvedValueOnce({ revision: 8, full: false, rows: [qso('W1AW', 400)] })
    rerender(<Reader id="a" q={history('W1AW')} tick={4} />)
    await waitFor(() => expect(text('a')).toBe('2'))
    expect(seen.get('a')).not.toBe(held)
  })

  it('views starting together share ONE request; a new question asks for nothing; a new tick asks once', async () => {
    api.getLogDelta.mockResolvedValue({ revision: 7, full: true, rows: [qso('W1AW'), qso('K1ABC')] })
    const { rerender } = render(
      <>
        <Reader id="a" q={history('W1AW')} tick={3} />
        <Reader id="b" q={history('K1ABC')} tick={3} />
        <Reader id="c" q={{ kind: 'lotwBacklog' }} tick={3} />
      </>,
    )
    await waitFor(() => expect(text('b')).toBe('1'))
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
    expect(api.getLogDelta).toHaveBeenCalledWith(0, 0)

    // The operator types another call into a strip: a new question, the same tick.
    rerender(
      <>
        <Reader id="a" q={history('K1AB')} tick={3} />
        <Reader id="b" q={history('K1ABC')} tick={3} />
        <Reader id="c" q={{ kind: 'lotwBacklog' }} tick={3} />
      </>,
    )
    await settle()
    expect(text('a')).toBe('0')
    expect(api.getLogDelta, 'a keystroke must not cost a request').toHaveBeenCalledTimes(1)

    // A contact is logged: the snapshot's tick moves, and the whole window asks once.
    api.getLogDelta.mockResolvedValue({ revision: 8, full: false, rows: [qso('K1AB')] })
    rerender(
      <>
        <Reader id="a" q={history('K1AB')} tick={4} />
        <Reader id="b" q={history('K1ABC')} tick={4} />
        <Reader id="c" q={{ kind: 'lotwBacklog' }} tick={4} />
      </>,
    )
    await waitFor(() => expect(text('a')).toBe('1'))
    await settle()
    expect(api.getLogDelta).toHaveBeenCalledTimes(2)
    expect(api.getLogDelta).toHaveBeenLastCalledWith(7, 2)
  })

  it('a view with no tick (no snapshot yet) asks once on start, not once per question', async () => {
    // A tick-less report means "refresh me" — so it must follow the tick, never the question, or
    // every keystroke in such a view would pull the whole log again.
    api.getLogDelta.mockResolvedValue({ revision: 7, full: true, rows: [qso('W1AW')] })
    const { rerender } = render(<Reader id="a" q={history('W')} />)
    await waitFor(() => expect(text('a')).toBe('0'))
    await settle()
    const started = api.getLogDelta.mock.calls.length
    for (const call of ['W1', 'W1A', 'W1AW']) rerender(<Reader id="a" q={history(call)} />)
    await settle()
    expect(text('a')).toBe('1')
    expect(api.getLogDelta.mock.calls.length, 'requests made by typing into a tick-less view').toBe(started)
  })

  it('a view that is not reading (q = null) asks for nothing and has no answer', async () => {
    api.getLogDelta.mockResolvedValue({ revision: 7, full: true, rows: [qso('W1AW')] })
    render(<Reader id="off" q={null} tick={3} />)
    await settle()
    expect(text('off')).toBe('none')
    expect(api.getLogDelta).not.toHaveBeenCalled()
  })

  it('`ask` is the read-once path: a refresh, then the answer', async () => {
    api.getLogDelta.mockResolvedValue({ revision: 9, full: true, rows: [qso('W1AW'), qso('W1AW', 5)] })
    const answer = await wholeLogSource.ask(history('w1aw'))
    expect(answer.count).toBe(2)
    expect(api.getLogDelta).toHaveBeenCalledTimes(1)
  })

  it('pages cut from one copy carry one revision, even after the store’s revision moves alone', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW', 1), qso('K1ABC', 2), qso('N2XYZ', 3)] })
    await wholeLogSource.ask({ kind: 'lotwBacklog' })
    const page0 = wholeLogSource.peek({ kind: 'page', query: DEFAULT_LOG_QUERY, offset: 0, limit: 2 })!
    // An answer that appends nothing moves the revision and keeps the rows.
    api.getLogDelta.mockResolvedValueOnce({ revision: 8, full: false, rows: [] })
    wholeLogSource.refresh()
    await settle()
    const page1 = wholeLogSource.peek({ kind: 'page', query: DEFAULT_LOG_QUERY, offset: 2, limit: 2 })!
    expect(page0.orderRev).toBe(7)
    expect(page1.orderRev, 'two pages of one copy must name one revision').toBe(7)
    expect(page1.rows.map((r) => r.call)).toEqual(['W1AW'])
    expect(page0.total).toBe(3)
  })

  it('a row without an id is keyed by its log position; a row with one, by its id', async () => {
    api.getLogDelta.mockResolvedValueOnce({ revision: 7, full: true, rows: [qso('W1AW', 1), qso('K1ABC', 2, { id: 'posid:1:2' })] })
    await wholeLogSource.ask({ kind: 'lotwBacklog' })
    const page = wholeLogSource.peek({ kind: 'page', query: DEFAULT_LOG_QUERY, offset: 0, limit: 5 })!
    expect(page.keys).toEqual(['posid:1:2', '#0'])
    expect(wholeLogSource.peek({ kind: 'row', id: '#0' })?.call).toBe('W1AW')
    expect(wholeLogSource.peek({ kind: 'locate', query: DEFAULT_LOG_QUERY, id: 'posid:1:2' })?.index).toBe(0)
  })

  it('the empty log’s answer is one stable object per question — the old `?? NO_LOG` fallback', () => {
    expect(emptyAnswer(history('W1AW'))).toBe(emptyAnswer(history('W1AW')))
    expect(emptyAnswer({ kind: 'entity', entity: 'Japan' })).toEqual({
      newEntity: true, // an empty log calls every known entity new, as it always did while loading
      slots: { workedEver: false, bandsWorked: [], modesWorked: [], bandUnknown: false },
    })
  })
})

describe('useLogAnswer drives any source through the same five calls', () => {
  function spySource(): LogSource & { calls: string[] } {
    const calls: string[] = []
    return {
      calls,
      peek: () => undefined,
      ask: async () => {
        throw new Error('not asked here')
      },
      want: (q) => {
        calls.push(`want ${JSON.stringify(q)}`)
        return () => calls.push(`release ${JSON.stringify(q)}`)
      },
      follow: (t) => {
        calls.push(`follow ${t}`)
      },
      subscribe: () => () => {},
      refresh: () => {},
    }
  }

  it('wants the question it shows, releases it on a new one and on unmount; follows only the tick', () => {
    const spy = spySource()
    setLogSource(spy)
    const { rerender, unmount } = render(<Reader id="a" q={history('W1AW')} tick={3} />)
    rerender(<Reader id="a" q={history('W1AW')} tick={3} />)
    rerender(<Reader id="a" q={history('K1ABC')} tick={3} />)
    rerender(<Reader id="a" q={history('K1ABC')} tick={4} />)
    unmount()
    const w = (c: string) => JSON.stringify(history(c))
    expect(spy.calls).toEqual([
      'follow 3',
      `want ${w('W1AW')}`,
      `release ${w('W1AW')}`,
      `want ${w('K1ABC')}`,
      'follow 4',
      `release ${w('K1ABC')}`,
    ])
  })

  it('a view that stops reading releases its question and reports nothing further', () => {
    const spy = spySource()
    setLogSource(spy)
    const { rerender } = render(<Reader id="a" q={history('W1AW')} tick={3} />)
    rerender(<Reader id="a" q={null} tick={4} />)
    expect(spy.calls).toEqual(['follow 3', `want ${JSON.stringify(history('W1AW'))}`, `release ${JSON.stringify(history('W1AW'))}`])
  })
})

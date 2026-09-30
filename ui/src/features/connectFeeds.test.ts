// @vitest-environment jsdom
//
// CONNECT'S FEEDS, ONE POLL PER WINDOW — the store's contract, driven with fake feeds so every
// number here is one the test chose. The integration half (two real surfaces on screen, one poll)
// is connectFeeds.shared.test.tsx and DashRail.feeds.test.tsx.
//
// Every fake answers a DIFFERENT value on each call (1, 2, 3 …). Two consumers that each polled
// would be told two different numbers, so a second poll cannot hide behind an equal answer: the
// disagreeing-values rule for a "one source" claim.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { createElement } from 'react'
import {
  __resetConnectFeedsForTests,
  peekFeed,
  useFeed,
  useKeyedFeed,
  wantFeed,
  watchFeed,
  type Feed,
  type KeyedFeed,
} from './connectFeeds'

/** A feed answering 1, 2, 3 … — or failing, while `fail` is set. */
function counting(everyMs: number | null = 1000) {
  let n = 0
  const state = { fail: false }
  const load = vi.fn(async () => {
    if (state.fail) throw new Error('down')
    return { n: ++n }
  })
  const feed: Feed<{ n: number }> = { name: 'counting', load, everyMs }
  return { feed, load, state }
}

/** Let the fake feed's promise settle. */
const settle = () => act(async () => {})

beforeEach(() => {
  vi.useFakeTimers()
})
afterEach(() => {
  cleanup()
  __resetConnectFeedsForTests()
  vi.useRealTimers()
})

describe('two consumers of a feed share one poll', () => {
  it('one fetch when the first arrives, one per interval — never one per consumer', async () => {
    const { feed, load } = counting()
    const a = wantFeed(feed)
    await settle()
    // The second consumer arrives LATER, as a second surface does: two consumers arriving in the
    // same instant would hide a poll of their own behind the in-flight guard (control MS1 showed
    // it: two timers firing together are one request).
    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    const b = wantFeed(feed)
    await settle()
    expect(load, 'the second consumer started a poll of its own').toHaveBeenCalledTimes(1)
    expect(peekFeed(feed).value).toEqual({ n: 1 })
    await act(async () => {
      vi.advanceTimersByTime(600)
    })
    expect(load).toHaveBeenCalledTimes(2)
    await act(async () => {
      vi.advanceTimersByTime(400)
    })
    expect(load, 'each consumer polled on its own timer').toHaveBeenCalledTimes(2)
    // Both read the one answer: had each polled, they would have been told different numbers.
    expect(peekFeed(feed).value).toEqual({ n: 2 })
    a()
    b()
  })

  it('stops polling when the last consumer leaves, and forgets what it held', async () => {
    const { feed, load } = counting()
    const a = wantFeed(feed)
    const b = wantFeed(feed)
    await settle()
    a()
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(load, 'one consumer left and the poll stopped for the other').toHaveBeenCalledTimes(2)
    b()
    await act(async () => {
      vi.advanceTimersByTime(5000)
    })
    expect(load, 'nobody shows the feed and it is still being polled').toHaveBeenCalledTimes(2)
    // Nothing held once nobody shows it: the next surface asks afresh, as a mount always did.
    expect(peekFeed(feed)).toEqual({ value: undefined, settled: false })
    const c = wantFeed(feed)
    expect(load, 'a surface arriving later is not asked for').toHaveBeenCalledTimes(3)
    c()
  })

  it('a release called twice counts once', async () => {
    const { feed, load } = counting()
    const a = wantFeed(feed)
    const b = wantFeed(feed)
    await settle()
    a()
    a()
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(load, 'a double release stopped the poll another consumer still wants').toHaveBeenCalledTimes(2)
    b()
  })
})

describe('what a feed holds is honest', () => {
  it('a failed answer keeps what is on screen, and settles a feed that never had one', async () => {
    const { feed, state } = counting()
    state.fail = true
    const release = wantFeed(feed)
    await settle()
    // "Asked and got nothing" is not "still asking": a box can say so.
    expect(peekFeed(feed)).toEqual({ value: undefined, settled: true })
    state.fail = false
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(peekFeed(feed).value).toEqual({ n: 1 })
    state.fail = true
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(peekFeed(feed).value, 'a failed refresh blanked a value the operator was reading').toEqual({ n: 1 })
    release()
  })

  it('an answer that lands after everyone left is dropped', async () => {
    let resolve: (v: { n: number }) => void = () => {}
    const load = vi.fn(() => new Promise<{ n: number }>((r) => (resolve = r)))
    const feed: Feed<{ n: number }> = { name: 'late', load, everyMs: 1000 }
    const release = wantFeed(feed)
    release()
    resolve({ n: 7 })
    await settle()
    expect(peekFeed(feed).value, 'a late answer filled a feed nobody shows').toBeUndefined()
  })

  it('a request that never comes back does not stop the poll', async () => {
    // One hung request must not freeze a feed for the rest of the session: the old per-surface
    // effects asked on every cycle whatever the last one did, and so does the store.
    const load = vi.fn(() => new Promise<number>(() => {}))
    const feed: Feed<number> = { name: 'hung', load, everyMs: 1000 }
    const release = wantFeed(feed)
    await act(async () => {
      vi.advanceTimersByTime(5000)
    })
    expect(load, 'a request that never answered stopped the poll').toHaveBeenCalledTimes(6)
    release()
  })

  it('an older answer that lands last never overwrites a newer one', async () => {
    const pending: Array<(v: { n: number }) => void> = []
    const load = vi.fn(() => new Promise<{ n: number }>((r) => pending.push(r)))
    const feed: Feed<{ n: number }> = { name: 'race', load, everyMs: 1000 }
    const release = wantFeed(feed)
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(pending).toHaveLength(2)
    pending[1]({ n: 2 })
    await settle()
    pending[0]({ n: 1 })
    await settle()
    expect(peekFeed(feed).value, 'a slow earlier answer replaced the newer one on screen').toEqual({ n: 2 })
    release()
  })

  it('a feed that throws before it returns a promise fails like any other', async () => {
    const feed: Feed<number> = {
      name: 'throws',
      load: () => {
        throw new Error('no bridge')
      },
      everyMs: 1000,
    }
    const release = wantFeed(feed)
    await settle()
    expect(peekFeed(feed)).toEqual({ value: undefined, settled: true })
    release()
  })
})

describe('watchFeed — a listener that renders nothing (App’s alert watchers)', () => {
  it('hears each answer once and no failure, and shares the poll with a surface', async () => {
    const { feed, load, state } = counting()
    const heard: number[] = []
    // The first ask FAILS: a feed that has never answered settles with nothing, and that is no
    // answer to hear.
    state.fail = true
    const surface = wantFeed(feed)
    // Recorded, never dereferenced: a watcher handed `undefined` must show up in `heard` (as -1),
    // not throw inside the store's callback where no assertion would see it (control MS7).
    const stop = watchFeed(feed, (v) => heard.push((v as { n: number } | undefined)?.n ?? -1))
    await settle()
    expect(load, 'the watcher polled on its own').toHaveBeenCalledTimes(1)
    expect(heard, 'a failure before any answer was heard as one').toEqual([])
    state.fail = false
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    state.fail = true
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    state.fail = false
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(heard, 'a failure was heard as an answer, or an answer twice').toEqual([1, 2])
    stop()
    surface()
  })
})

describe('a watcher that fails is its own problem', () => {
  it('costs the listeners after it nothing, and nothing escapes the store', async () => {
    const { feed } = counting()
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    const heard: number[] = []
    try {
      const stopA = watchFeed(feed, () => {
        throw new Error('a watcher broke')
      })
      const stopB = watchFeed(feed, (v) => heard.push(v.n))
      await settle()
      expect(heard, 'a failing watcher starved the one after it').toEqual([1])
      expect(err, 'the failure was swallowed silently').toHaveBeenCalled()
      stopA()
      stopB()
    } finally {
      err.mockRestore()
    }
  })
})

describe('a keyed feed (the path outlook for a selection) is asked once per key, never polled', () => {
  it('two consumers of one key share one fetch; another key has its own', async () => {
    const load = vi.fn(async (key: string) => ({ key }))
    const kf: KeyedFeed<{ key: string }> = { name: 'path', load }
    function Reader({ k }: { k: string | null }) {
      const held = useKeyedFeed(kf, k)
      return createElement('span', { 'data-testid': 'r' }, held.value?.key ?? '-')
    }
    const view = render(
      createElement('div', null, createElement(Reader, { k: 'EN52' }), createElement(Reader, { k: 'EN52' })),
    )
    await settle()
    expect(load, 'each consumer asked for the same path').toHaveBeenCalledTimes(1)
    await act(async () => {
      vi.advanceTimersByTime(60 * 60_000)
    })
    expect(load, 'the path outlook was polled').toHaveBeenCalledTimes(1)
    view.rerender(
      createElement('div', null, createElement(Reader, { k: 'JN58' }), createElement(Reader, { k: 'EN52' })),
    )
    await settle()
    expect(load).toHaveBeenCalledTimes(2)
    expect(load).toHaveBeenLastCalledWith('JN58')
    expect(view.getAllByTestId('r').map((e) => e.textContent)).toEqual(['JN58', 'EN52'])
  })

  it('no key asks for nothing', async () => {
    const load = vi.fn(async (key: string) => key)
    const kf: KeyedFeed<string> = { name: 'none', load }
    function Reader() {
      useKeyedFeed(kf, null)
      return null
    }
    render(createElement(Reader))
    await settle()
    expect(load).not.toHaveBeenCalled()
  })
})

describe('useFeed', () => {
  it('two components on one feed: one fetch, the same value on both', async () => {
    const { feed, load } = counting()
    function Reader() {
      const held = useFeed(feed)
      return createElement('span', { 'data-testid': 'v' }, held.value ? String(held.value.n) : '-')
    }
    const view = render(createElement('div', null, createElement(Reader), createElement(Reader)))
    await settle()
    expect(load).toHaveBeenCalledTimes(1)
    expect(view.getAllByTestId('v').map((e) => e.textContent)).toEqual(['1', '1'])
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    expect(load).toHaveBeenCalledTimes(2)
    expect(view.getAllByTestId('v').map((e) => e.textContent)).toEqual(['2', '2'])
  })

  it('a disabled consumer (the hosted Remote page) asks for nothing and reads nothing', async () => {
    const { feed, load } = counting()
    function Reader() {
      const held = useFeed(feed, false)
      return createElement('span', { 'data-testid': 'v' }, String(held.settled))
    }
    const view = render(createElement(Reader))
    await settle()
    expect(load).not.toHaveBeenCalled()
    expect(view.getByTestId('v').textContent).toBe('false')
  })
})

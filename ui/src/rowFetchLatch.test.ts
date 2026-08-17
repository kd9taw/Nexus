// THE SINGLE-FLIGHT LATCH MUST HAVE A WAY OUT (field reports, 2026-08-17: "the waterfall
// stops").
//
// The waterfall and the rig scope both poll their spectrum row behind a single-flight guard, so
// a slow row skips its tick instead of overlapping. `invoke` has NO TIMEOUT, so one call that
// never settles — neither resolves nor rejects — latched that guard for the life of the mount:
// the rAF loop kept spinning and the overlay kept repainting, so the display looked alive while
// no row was ever fetched again. Decodes arrive on a different channel and kept coming, which is
// why this reached us as "the waterfall died but the radio is fine" rather than as a hang.
//
// The escape is a watchdog plus a GENERATION COUNTER, and the generation is the subtle half: a
// call the watchdog gave up on can still resolve afterwards, and it must then neither draw (its
// row is stale, and the history ring has to stay 1:1 with what was blitted) nor release the
// latch (which by then belongs to a live fetch). Both loops used to carry their own copy of
// this; it lives in one tested place now.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { RowFetchLatch, ROW_FETCH_STUCK_MS } from './waterfall'

afterEach(() => vi.restoreAllMocks())

describe('RowFetchLatch', () => {
  it('is single-flight: a second claim is refused while one is in flight', () => {
    const latch = new RowFetchLatch('test')
    const gen = latch.begin(0)
    expect(gen).not.toBeNull()
    expect(latch.inFlight).toBe(true)
    expect(latch.begin(10)).toBeNull()
    latch.end(gen!)
    expect(latch.inFlight).toBe(false)
    expect(latch.begin(20)).not.toBeNull()
  })

  it('holds a merely SLOW fetch — patience is the default, abandonment is the exception', () => {
    // The control for the watchdog below. If this direction is not pinned, a watchdog that
    // fired instantly would pass every other test in this file while breaking the poll.
    const latch = new RowFetchLatch('test')
    latch.begin(0)
    expect(latch.abandonIfStuck(ROW_FETCH_STUCK_MS)).toBe(false)
    expect(latch.inFlight, 'exactly at the limit is still in flight, not stuck').toBe(true)
  })

  it('abandons a fetch that outlives the limit, and says so once', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const latch = new RowFetchLatch('waterfall')
    latch.begin(0)
    expect(latch.abandonIfStuck(ROW_FETCH_STUCK_MS + 1)).toBe(true)
    expect(latch.inFlight, 'the latch is free, so the next tick can poll again').toBe(false)
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0][0]).toMatch(/waterfall/)

    // A wedged bridge must not flood the console at frame rate — that buries the one line
    // that explains the symptom.
    latch.begin(100)
    latch.abandonIfStuck(100 + ROW_FETCH_STUCK_MS + 1)
    expect(warn).toHaveBeenCalledTimes(1)
  })

  it('an abandoned call may not draw when it finally resolves', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    const latch = new RowFetchLatch('test')
    const zombie = latch.begin(0)!
    expect(latch.owns(zombie), 'control: it owned the latch while in flight').toBe(true)
    latch.abandonIfStuck(ROW_FETCH_STUCK_MS + 1)
    expect(
      latch.owns(zombie),
      'a stale row must not reach the history ring or the leading-edge blit',
    ).toBe(false)
  })

  it('an abandoned call may not release a latch that now belongs to a live fetch', () => {
    // The failure this prevents is worse than it looks: the zombie clearing the latch would let
    // a THIRD fetch start while the live one is still out, and two in-flight rows racing to
    // append is exactly the desync the single-flight guard exists to prevent.
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    const latch = new RowFetchLatch('test')
    const zombie = latch.begin(0)!
    latch.abandonIfStuck(ROW_FETCH_STUCK_MS + 1)
    const live = latch.begin(6000)!
    expect(live).not.toBe(zombie)

    latch.end(zombie) // the abandoned call finally settles
    expect(latch.inFlight, 'the live fetch still owns the latch').toBe(true)
    expect(latch.owns(live)).toBe(true)

    latch.end(live)
    expect(latch.inFlight).toBe(false)
  })

  it('nothing to abandon when idle, or when the fetch is young', () => {
    const latch = new RowFetchLatch('test')
    expect(latch.abandonIfStuck(1e9), 'an idle latch is not stuck').toBe(false)
    latch.begin(1000)
    expect(latch.abandonIfStuck(1200)).toBe(false)
    expect(latch.inFlight).toBe(true)
  })

  it('recovers to normal polling after an abandonment — the point of the whole exercise', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    const latch = new RowFetchLatch('test')
    latch.begin(0)
    latch.abandonIfStuck(ROW_FETCH_STUCK_MS + 1)
    // Three ordinary ticks: claim, settle, claim again.
    for (let t = 6000; t < 6300; t += 100) {
      const gen = latch.begin(t)
      expect(gen, `tick ${t}: the latch must be claimable again`).not.toBeNull()
      latch.end(gen!)
    }
    expect(latch.inFlight).toBe(false)
  })
})

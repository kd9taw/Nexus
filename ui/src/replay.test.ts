import { describe, expect, it } from 'vitest'
import {
  clampReplay,
  replayFlux,
  replayWindow,
  REPLAY_SPAN_SECS,
  REPLAY_AHEAD_SECS,
  REPLAY_BUCKET_SECS,
  replayStart,
} from './replay'

const NOW = Date.UTC(2026, 9, 31, 18, 0)
const NOW_S = NOW / 1000

/** 5-minute bucket peaks back to `hours` ago, flux from `f`. */
function series(hours: number, f: (ago: number) => number = () => 1e-6): [number, number][] {
  const out: [number, number][] = []
  for (let ago = hours * 3600; ago >= 0; ago -= 300) out.push([NOW_S - ago, f(ago)])
  return out
}

describe('replayWindow: the last six hours of what is held, never more, never ahead', () => {
  it('spans the held trace, capped at six hours back and at now', () => {
    const w = replayWindow(series(6), NOW)!
    expect(w.loMs).toBe(NOW - REPLAY_SPAN_SECS * 1000)
    expect(w.hiMs).toBe(NOW)
    // A trace that holds only two hours replays only those two.
    const w2 = replayWindow(series(2), NOW)!
    expect(w2.loMs).toBe(NOW - 2 * 3600 * 1000)
    // Older samples than six hours are not offered.
    const w8 = replayWindow(series(8), NOW)!
    expect(w8.loMs).toBe(NOW - REPLAY_SPAN_SECS * 1000)
  })

  it('offers nothing without a held trace', () => {
    expect(replayWindow([], NOW)).toBeNull()
    expect(replayWindow(null, NOW)).toBeNull()
    expect(replayWindow(undefined, NOW)).toBeNull()
    // One sample is not a stretch of time to replay.
    expect(replayWindow([[NOW_S - 600, 1e-6]], NOW)).toBeNull()
  })

  it('ignores samples dated ahead of the clock, undated, or not a flux', () => {
    const s: [number, number][] = [
      [NOW_S - 3600, 1e-6],
      [NOW_S - 1800, 1e-6],
      [NOW_S + REPLAY_AHEAD_SECS + 1, 1e-4],
      [Number.NaN, 1e-4],
      [NOW_S - 900, Number.NaN],
      [NOW_S - 600, 0],
    ]
    const w = replayWindow(s, NOW)!
    expect(w.hiMs).toBe((NOW_S - 1800) * 1000)
    expect(w.loMs).toBe((NOW_S - 3600) * 1000)
    // A trace that is ALL ahead of the clock offers nothing.
    expect(replayWindow([[NOW_S + 3600, 1e-6], [NOW_S + 3900, 1e-6]], NOW)).toBeNull()
  })

  it('never reaches past now even when a sample sits inside the skew allowance', () => {
    const w = replayWindow([...series(1), [NOW_S + 60, 1e-6]], NOW)!
    expect(w.hiMs).toBe(NOW)
  })
})

describe('clampReplay', () => {
  it('holds a replay time inside the window', () => {
    const w = { loMs: NOW - 3600_000, hiMs: NOW }
    expect(clampReplay(NOW - 1800_000, w)).toBe(NOW - 1800_000)
    expect(clampReplay(NOW - 7200_000, w)).toBe(NOW - 3600_000)
    expect(clampReplay(NOW + 60_000, w)).toBe(NOW)
    expect(clampReplay(Number.NaN, w)).toBe(NOW)
  })
})

describe('replayFlux: the reading at the replayed time, or none', () => {
  const s = series(6, (ago) => (ago === 7200 ? 5e-5 : 1e-6))

  it('takes the peak of the replayed time’s own bucket', () => {
    expect(replayFlux(s, NOW - 7200_000, NOW)).toEqual({ flux: 5e-5, at: NOW_S - 7200 })
    expect(replayFlux(s, NOW - 7200_000 + 299_000, NOW)).toEqual({ flux: 5e-5, at: NOW_S - 7200 })
    expect(replayFlux(s, NOW - 7200_000 + 300_000, NOW)?.flux, 'the next bucket is its own').toBe(1e-6)
    expect(replayFlux(s, NOW - 7200_000 - 1_000, NOW)?.flux, 'the bucket before is its own').toBe(1e-6)
  })

  it('reads a peak anywhere in its bucket, however far it sits from its neighbours', () => {
    // The shack stamps each bucket's peak at its own minute: two adjacent buckets' peaks can be
    // nearly ten minutes apart, and neither is a gap.
    const b = Math.floor((NOW_S - 3600) / REPLAY_BUCKET_SECS) * REPLAY_BUCKET_SECS
    const peaks: [number, number][] = [
      [b + 5, 2e-5],
      [b + 595, 3e-5],
    ]
    expect(replayFlux(peaks, (b + 290) * 1000, NOW)).toEqual({ flux: 2e-5, at: b + 5 })
    expect(replayFlux(peaks, (b + 300) * 1000, NOW)).toEqual({ flux: 3e-5, at: b + 595 })
    expect(replayFlux(peaks, (b + 600) * 1000, NOW), 'the bucket after holds no reading').toBeNull()
  })

  it('has no reading where the trace has a gap wider than one bucket', () => {
    const gap = s.filter(([t]) => t < NOW_S - 9000 || t > NOW_S - 5400)
    expect(replayFlux(gap, NOW - 7200_000, NOW)).toBeNull()
    // CONTROL: within one bucket of the last held sample before the gap it has one.
    expect(replayFlux(gap, (NOW_S - 9300 + REPLAY_BUCKET_SECS - 1) * 1000, NOW)).not.toBeNull()
    expect(replayFlux(gap, (NOW_S - 9300 + REPLAY_BUCKET_SECS) * 1000, NOW)).toBeNull()
  })

  it('a single missing bucket draws nothing; the buckets either side still do', () => {
    const missing = NOW_S - 7200
    const one = s.filter(([t]) => t !== missing)
    expect(one.length, 'CONTROL: exactly one bucket is gone').toBe(s.length - 1)
    expect(replayFlux(one, missing * 1000, NOW), 'a neighbour’s peak bridged the gap').toBeNull()
    expect(replayFlux(one, (missing - 300) * 1000, NOW)).toEqual({ flux: 1e-6, at: missing - 300 })
    expect(replayFlux(one, (missing + 300) * 1000, NOW)).toEqual({ flux: 1e-6, at: missing + 300 })
  })

  it('never reads a sample dated ahead of the clock, undated, or not a flux', () => {
    const bad: [number, number][] = [
      [NOW_S + REPLAY_AHEAD_SECS + 30, 1e-4],
      [Number.NaN, 1e-4],
      [NOW_S - 60, -1],
    ]
    expect(replayFlux(bad, NOW, NOW)).toBeNull()
  })

  it('has nothing outside the six-hour window', () => {
    expect(replayFlux(series(8), NOW - 7 * 3600_000, NOW)).toBeNull()
    expect(replayFlux([], NOW, NOW)).toBeNull()
    expect(replayFlux(null, NOW, NOW)).toBeNull()
  })
})

describe('replayStart: the moment most worth seeing again', () => {
  it('starts at the highest reading in the window', () => {
    const s = series(6, (ago) => (ago === 9000 ? 3e-5 : ago === 1800 ? 2e-5 : 1e-6))
    expect(replayStart(s, NOW)).toBe((NOW_S - 9000) * 1000)
  })

  it('never starts on a reading outside the window or ahead of the clock', () => {
    const s: [number, number][] = [
      ...series(2),
      [NOW_S - 7 * 3600, 1e-3],
      [NOW_S + 3600, 1e-3],
    ]
    expect(replayStart(s, NOW)).toBe((NOW_S - 2 * 3600) * 1000) // all equal: the earliest
  })

  it('has no start without a held trace', () => {
    expect(replayStart([], NOW)).toBeNull()
    expect(replayStart(null, NOW)).toBeNull()
  })
})

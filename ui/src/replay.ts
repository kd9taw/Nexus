// The six-hour replay — the pure half (MapView and Globe3D draw it, ReplayBar words it).
//
// WHAT IS REPLAYED is only what this station already holds: the fast lane's six-hour GOES trace
// (the 5-minute bucket peaks, each at its own time) for the flux, and the shared sun ephemeris for
// the sun, the moon and the night side at the replayed time. The flare footprint and its band rings
// are the analytic field from that flux at that time's sun, ALWAYS labelled "estimated from GOES":
// NOAA's D-RAP grid is kept only for now, so a past footprint is never drawn with NOAA's name on it.
// Everything else on the map stays live, and the replay bar says so.
//
// FAIL CLOSED. A sample with no time, a time ahead of this clock (beyond the skew allowance), or a
// flux that is not a positive number is never read. The replay never reaches before six hours ago
// or past now. A replayed time with no held sample within one bucket has NO flux: nothing is drawn
// for the flare, and the bar says so — never the nearest reading from further away.

/** How far back a replay reaches. */
export const REPLAY_SPAN_SECS = 6 * 3600
/** The slider's step: one GOES bucket. */
export const REPLAY_STEP_SECS = 300
/** A sample may sit this far ahead of the clock and still be read (clock skew). */
export const REPLAY_AHEAD_SECS = 120
/** A bucket peak speaks for the replayed time within this distance (one bucket). */
export const REPLAY_MATCH_SECS = 300

export interface ReplayWindow {
  loMs: number
  hiMs: number
}

type Trace = ReadonlyArray<readonly [number, number]> | null | undefined

/** The samples that may be read at `nowMs`: dated, not ahead, a positive flux, inside six hours. */
function usable(series: Trace, nowMs: number): Array<readonly [number, number]> {
  const now = nowMs / 1000
  return (series ?? []).filter(
    ([t, f]) =>
      Number.isFinite(t) &&
      Number.isFinite(f) &&
      f > 0 &&
      t <= now + REPLAY_AHEAD_SECS &&
      t >= now - REPLAY_SPAN_SECS,
  )
}

/** The stretch of time a replay may show at `nowMs`, or null when no trace is held (fewer than
 *  two usable samples). Never earlier than six hours ago, never later than now. */
export function replayWindow(series: Trace, nowMs: number): ReplayWindow | null {
  const s = usable(series, nowMs)
  if (s.length < 2) return null
  const times = s.map(([t]) => t)
  // `usable` already holds every sample inside the six hours.
  const lo = Math.min(...times) * 1000
  const hi = Math.min(Math.max(...times) * 1000, nowMs)
  return hi > lo ? { loMs: lo, hiMs: hi } : null
}

/** A replay time held inside the window (a time that is not a number goes to now's end). */
export function clampReplay(atMs: number, w: ReplayWindow): number {
  if (!Number.isFinite(atMs)) return w.hiMs
  return Math.min(w.hiMs, Math.max(w.loMs, atMs))
}

/** The flux at the replayed time: the usable bucket peak nearest it, within one bucket; else null. */
export function replayFlux(series: Trace, atMs: number, nowMs: number): { flux: number; at: number } | null {
  const at = atMs / 1000
  if (at < nowMs / 1000 - REPLAY_SPAN_SECS) return null
  let best: readonly [number, number] | null = null
  for (const p of usable(series, nowMs)) {
    const d = Math.abs(p[0] - at)
    if (d <= REPLAY_MATCH_SECS && (best == null || d < Math.abs(best[0] - at))) best = p
  }
  return best ? { flux: best[1], at: best[0] } : null
}

/** Where a new replay starts: the time of the highest usable reading in the window (the moment
 *  most worth seeing again; the earliest of equal highs), or null when no trace is held. */
export function replayStart(series: Trace, nowMs: number): number | null {
  const w = replayWindow(series, nowMs)
  if (!w) return null
  let best: readonly [number, number] | null = null
  for (const p of usable(series, nowMs)) if (best == null || p[1] > best[1]) best = p
  return best ? clampReplay(best[0] * 1000, w) : null
}

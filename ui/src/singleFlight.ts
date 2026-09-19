// One call at a time, with a way out — the single-flight guard the UI's pollers share.
//
// `invoke` has NO TIMEOUT and no cancellation, and a poll on `setInterval` fires whether or not
// the last call came back. Against a command that takes the engine mutex that is how #335 froze
// the waterfall a few minutes into a session: the mutex is held across blocking CAT I/O (up to
// 2.5 s on a slow transport) and log saves, every engine-locking command waits for it on a tokio
// worker (one per logical CPU), and a 300 ms snapshot poll — in the main window AND in the Needed
// pop-out — stacked a new waiter every tick until the pool was empty. The waterfall's lock-free
// `get_spectrum_row` then had no worker to run on.
//
// So: a poll that finds its previous call still out skips the tick. And because one call that
// never settles would then stop the poll for good, a WATCHDOG gives such a call up after a limit
// and lets the next tick ask again (the "waterfall stops" fix of 2026-08-17, first written for the
// spectrum row — `RowFetchLatch` in waterfall.ts is this latch with the row's limit and label).

/**
 * How long a poll of an ENGINE-LOCKING command may be in flight before it is given up on.
 *
 * - Past the longest legitimate hold: the engine mutex is held across blocking CAT I/O whose
 *   slow-transport reply window is 2.5 s (tempo-audio `rig.rs`), and across a log save. A poll
 *   waiting behind one of those is SLOW, not stuck; 10 s is four of those windows, so no merely
 *   slow call is abandoned.
 * - Abandoning does not cancel anything: the given-up call keeps its backend worker until the
 *   mutex frees, so every abandonment adds a waiter. A long limit keeps that rare — through a
 *   stall that outlasts it, a poller adds one call per 10 s, where the bare interval added one
 *   per tick (about 33 per 10 s for the snapshot).
 * - Short enough to be the recovery it exists for: a call that never settles at all (a lost IPC
 *   reply) costs the display ten seconds, then the poll resumes on its own.
 *
 * It is the FLOOR: `pollSingleFlight` waits at least four of a poller's own intervals, so a slow
 * poller is not given up at its very next tick (see there).
 */
export const POLL_STUCK_MS = 10_000

/**
 * The single-flight latch, with a watchdog and a generation counter.
 *
 * ⚠️ WHY A GENERATION COUNTER, NOT A FLAG RESET. Once the watchdog gives a call up, that call may
 * still settle later, and it must then neither deliver its (stale) result nor release the latch —
 * which by then belongs to a live call. Both are the same bug, a zombie call touching state it no
 * longer owns, and both are closed by refusing to recognise a generation that has moved on.
 *
 * Nothing here cancels anything. There is no cancellation to have: the promise is whatever the IPC
 * bridge returned. A wedged bridge simply gets re-asked on the next tick instead of waited on
 * forever.
 */
export class SingleFlightLatch {
  private busy = false
  /** Clock reading at which the in-flight call started. Meaningful only while `busy` — `busy` is
   * the sole authority on idleness, because 0 is a legitimate clock reading and a sentinel here
   * would make the watchdog blind at exactly one moment. */
  private since = 0
  /** Bumped by every claim AND by every abandonment, so a generation is never reused. */
  private gen = 0
  private warned = false

  /** `label` names the surface in the one console warning, `what` the call it makes;
   *  `limitMs` is the patience. */
  constructor(
    private readonly label: string,
    private readonly limitMs: number = POLL_STUCK_MS,
    private readonly what: string = 'poll',
  ) {}

  /** True while a call owns the latch. */
  get inFlight(): boolean {
    return this.busy
  }

  /**
   * Give up on a call that has been in flight past the limit, freeing the latch for the next tick.
   * Returns true if it just did — call this BEFORE testing `inFlight` or claiming.
   *
   * Warns at most once per instance: a wedged bridge would otherwise flood the console at poll
   * rate, which buries the one line that explains the symptom.
   */
  abandonIfStuck(now: number): boolean {
    if (!this.busy || now - this.since <= this.limitMs) return false
    this.gen++
    this.busy = false
    this.since = 0
    if (!this.warned) {
      this.warned = true
      console.warn(
        `[${this.label}] ${this.what} did not settle within ${this.limitMs} ms; ` +
          'abandoning it and resuming the poll',
      )
    }
    return true
  }

  /** Claim the latch for a new call. Returns its generation, or `null` if one is in flight. */
  begin(now: number): number | null {
    if (this.busy) return null
    this.busy = true
    this.since = now
    return ++this.gen
  }

  /** Does `gen` still own the latch? A call the watchdog abandoned does not, and must not deliver. */
  owns(gen: number): boolean {
    return gen === this.gen
  }

  /** Release the latch — ignored unless `gen` still owns it. */
  end(gen: number): void {
    if (gen !== this.gen) return
    this.busy = false
    this.since = 0
  }
}

/**
 * Run `poll` every `everyMs`, never more than one at a time, and return the stop function.
 *
 * `poll` gets `owns()`: true while its answer is still wanted — false once the poll is stopped
 * (the component unmounted) or the watchdog gave this call up. Guard every state write with it;
 * it replaces the usual `alive` flag and also keeps a late, abandoned answer from overwriting a
 * newer one. The latch is held until the promise `poll` returns settles, so a poll that makes
 * several calls should return them all (e.g. `Promise.allSettled`).
 *
 * `leading` (default true) runs the first poll now rather than one interval from now.
 *
 * `stuckMs` defaults to `POLL_STUCK_MS` or four intervals, whichever is longer. A call is only
 * presumed lost once it has missed four of its own ticks: at a 15 s cadence a 10 s limit would
 * give up the stalled call at every tick, and the poll would stack exactly as fast as it did on a
 * bare interval.
 */
export function pollSingleFlight(
  label: string,
  everyMs: number,
  poll: (owns: () => boolean) => Promise<unknown>,
  {
    leading = true,
    stuckMs = Math.max(POLL_STUCK_MS, 4 * everyMs),
  }: { leading?: boolean; stuckMs?: number } = {},
): () => void {
  const latch = new SingleFlightLatch(label, stuckMs)
  let stopped = false
  const tick = () => {
    if (stopped) return
    const now = performance.now()
    latch.abandonIfStuck(now)
    const gen = latch.begin(now)
    if (gen === null) return // the last call is still out: skip this tick
    const owns = () => !stopped && latch.owns(gen)
    const release = () => latch.end(gen)
    try {
      void Promise.resolve(poll(owns)).then(release, release)
    } catch {
      release() // a poll that threw before returning a promise
    }
  }
  if (leading) tick()
  const id = window.setInterval(tick, everyMs)
  return () => {
    stopped = true
    window.clearInterval(id)
  }
}

// An idle stream (the operator's picks "15 min + prompt" and "Only clicks, keys, PTT"). Watching
// alone is idle, because the waterfall always moves: what counts is the operator's own input - a
// click, a key or a turn of the wheel anywhere on the page (the wheel by ruling B3: tuning with it
// is the operator at work), and a held PTT for as long as it is re-asserted. The station
// listening, the audio playing and the rig transmitting never count, so a running FT sequence does
// not hold a stream open. Fifteen minutes without any of it brings "Still there?"; a minute more,
// unanswered, ends the stream the way End the stream does.
//
// MEASURED, NEVER COUNTED. The watch reads a monotonic clock each time it is asked, and nothing
// else: a background tab's timers are throttled to about one a minute, so a count of the page's
// ticks would run slow by as much, and the time since the last input would not.

/** Fifteen minutes with no click, key or PTT: the page asks "Still there?". */
export const STREAM_IDLE_PROMPT_MS = 15 * 60_000
/** A minute more, unanswered: the stream ends. */
export const STREAM_IDLE_END_MS = 16 * 60_000

export type IdleState = 'active' | 'prompt' | 'end'

export class IdleWatch {
  private last: number
  constructor(private readonly now: () => number) { this.last = now() }
  /** The operator did something: a click, a key, a held PTT re-asserted. */
  active(): void { this.last = this.now() }
  /** Where the stream stands now, from the time since the operator last did anything. */
  state(): IdleState {
    const idle = this.now() - this.last
    return idle >= STREAM_IDLE_END_MS ? 'end' : idle >= STREAM_IDLE_PROMPT_MS ? 'prompt' : 'active'
  }
}

import { expect, it } from 'vitest'
import { IdleWatch, STREAM_IDLE_END_MS, STREAM_IDLE_PROMPT_MS } from './stream-idle'

const MIN = 60_000

/** A watch on a clock the test moves; `at(ms)` reads it `ms` after the watch was made. */
function watch() {
  let clock = 5000
  const idle = new IdleWatch(() => clock)
  const start = clock
  return { idle, at: (ms: number) => { clock = start + ms; return idle.state() } }
}

it('the operator\'s pick: "Still there?" at 15:00 with no click, key or PTT and not at 14:59; the end at 16:00 and not at 15:59', () => {
  expect([STREAM_IDLE_PROMPT_MS, STREAM_IDLE_END_MS]).toEqual([15 * MIN, 16 * MIN])
  const w = watch()
  expect(w.at(15 * MIN - 1000), '14:59').toBe('active')
  expect(w.at(15 * MIN), '15:00').toBe('prompt')
  expect(w.at(16 * MIN - 1000), '15:59').toBe('prompt')
  expect(w.at(16 * MIN), '16:00').toBe('end')
})

it('activity starts the fifteen minutes again, from the moment it happened', () => {
  const w = watch()
  w.at(10 * MIN)
  w.idle.active()
  expect(w.at(15 * MIN), 'fifteen minutes from the start, five from the activity').toBe('active')
  expect(w.at(25 * MIN - 1000)).toBe('active')
  expect(w.at(25 * MIN), 'fifteen from the activity').toBe('prompt')
  // Answering the prompt is activity like any other.
  w.idle.active()
  expect(w.at(26 * MIN), 'a minute after the answer').toBe('active')
})

it('measured, never counted: with its looks throttled to one a minute (a background tab), the end comes at the first look at or after 16:00, never after 17:00', () => {
  // Chrome runs a hidden tab's timers about once a minute once it has been hidden a while. The
  // looks here land at 0:30, 1:30, ... 16:30, and the one at 16:30 is the end. A watch that
  // counted its looks as seconds would stand at 17 by then, and end some sixteen hours later.
  const w = watch()
  let ended: number | null = null
  for (let look = 30_000; look <= 17 * MIN && ended === null; look += MIN) if (w.at(look) === 'end') ended = look
  expect(ended, 'the first look at or after 16:00').toBe(16 * MIN + 30_000)
  // And two looks sixteen and a half minutes apart are enough: nothing in between is needed.
  const sparse = watch()
  expect(sparse.at(0)).toBe('active')
  expect(sparse.at(16 * MIN + 30_000)).toBe('end')
})

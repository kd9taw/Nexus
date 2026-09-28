import { expect, it } from 'vitest'
import { ID_EVERY_MS, ID_FIRST_MS, ID_RUN_GAP_MS, IdReminder } from './id-reminder'

const MIN = 60_000

/** A run of overs: on the air for `on` ms out of every `period`, from `from` to `to`, looked at
 *  every second as the page does. Returns when the prompt came. */
function run(reminder: IdReminder, from: number, to: number, on: number, period: number): number[] {
  const due: number[] = []
  for (let t = from; t <= to; t += 1000) if (reminder.observe(t, (t - from) % period < on)) due.push(t)
  return due
}

it('M11: the prompt comes nine minutes into a run of overs and every ten minutes after, as the operator keys up', () => {
  // Thirty seconds on the air in every minute and a half: an ordinary phone contact. Each prompt
  // shows at the first moment on the air once it is due: 9 min (on the air then), 19 min (off the
  // air; next on at 19.5), 29 min (off; next on at 30).
  const due = run(new IdReminder(), 0, 30 * MIN, 30_000, 90_000)
  expect(due).toEqual([ID_FIRST_MS, ID_FIRST_MS + ID_EVERY_MS + 30_000, ID_FIRST_MS + 2 * ID_EVERY_MS + MIN])
})

it('M11 control: a 30-second over is not prompted, and nor is the operator who stopped', () => {
  const reminder = new IdReminder()
  expect(run(reminder, 0, 30_000, 30_000, 60_000)).toEqual([])
  // Quiet for longer than a run's gap: the run is over, and a new over starts a new one, whose
  // prompt counts from ITS start.
  expect(run(reminder, 31_000, 31_000 + ID_RUN_GAP_MS + MIN, 0, 1)).toEqual([])
  const restart = 31_000 + ID_RUN_GAP_MS + 2 * MIN
  expect(run(reminder, restart, restart + 12 * MIN, 30_000, 90_000)).toEqual([restart + ID_FIRST_MS])
})

it('M11: the stream ending with a run in progress is the last prompt; with none, there is none', () => {
  const reminder = new IdReminder()
  run(reminder, 0, 2 * MIN, 30_000, 90_000)
  expect(reminder.end(), 'the run in progress was not reported at the end').toBe(true)
  expect(reminder.end(), 'reported twice').toBe(false)
  expect(new IdReminder().end(), 'a stream with no over prompts nothing').toBe(false)
})

it('M11: a page that looks late does not move every later prompt', () => {
  const reminder = new IdReminder()
  reminder.observe(0, true)
  expect(reminder.observe(ID_FIRST_MS + 5 * MIN, true), 'the late look is due').toBe(true)
  expect(reminder.observe(ID_FIRST_MS + ID_EVERY_MS - 1000, true)).toBe(false)
  expect(reminder.observe(ID_FIRST_MS + ID_EVERY_MS, true), 'still counted from the run').toBe(true)
})

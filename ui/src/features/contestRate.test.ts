// The rate meter's arithmetic. Every number below is checked against a hand-worked
// answer, not against whatever the function happens to return.
//
// Two behaviours here are the reason the module exists at all, and both are asserted
// with the contrast that makes them meaningful rather than as a bare value:
//
//   • a window is measured to NOW, so an idle operator's rate FALLS (`< the same log
//     read at the moment of the last contact`), and
//   • an operator who stopped for two hours and came back does NOT read the rate they
//     were running before the gap (`not close to 60`, the run rate that produced the
//     contacts on either side of it).

import { describe, it, expect } from 'vitest'
import { contestRate } from './contestRate'
import type { FieldDayQso } from '../types'

/** A logged contact carrying nothing but its timestamp — the only field a rate reads. */
const q = (whenUnix?: number): FieldDayQso =>
  ({ call: 'W1AW', class: '1A', section: 'IL', band: '20m', mode: 'CW', whenUnix }) as FieldDayQso

/** An arbitrary Unix second to hang the fixtures off. */
const T0 = 1_757_000_000

/** `n` contacts `every` seconds apart, the first at `from`. */
const run = (n: number, from: number, every: number): FieldDayQso[] =>
  Array.from({ length: n }, (_, i) => q(from + i * every))

/** The last contact's timestamp — "now" for a meter read the instant a QSO is logged. */
const lastOf = (log: FieldDayQso[]) => log[log.length - 1].whenUnix!

describe('the window readings', () => {
  it('exactly 10 contacts a minute apart read 60 an hour on both windows', () => {
    // Ten contacts one minute apart span nine minutes and contain NINE intervals. 60/h is
    // the true answer; a count/span reading would say 66.7 and it would be wrong.
    const log = run(10, T0, 60)
    const r = contestRate(log, lastOf(log))
    expect(r.windows[0]).toEqual({ sample: 10, perHour: 60 })
    // The 100-window has only these ten to read, and says so.
    expect(r.windows[1]).toEqual({ sample: 10, perHour: 60 })
    expect(r.lastHour).toBe(10)
  })

  it('fewer than 10 contacts reads over what there is, and reports the real sample', () => {
    // Three contacts, two intervals, two minutes: 60/h. The label reads "Last 3", so the
    // screen never claims a ten-contact window it does not have.
    const log = run(3, T0, 60)
    const r = contestRate(log, lastOf(log))
    expect(r.windows[0]).toEqual({ sample: 3, perHour: 60 })
    expect(r.windows[1]).toEqual({ sample: 3, perHour: 60 })
  })

  it('one contact is not a rate — no interval has been measured yet', () => {
    const r = contestRate([q(T0)], T0)
    expect(r.windows[0]).toEqual({ sample: 1, perHour: null })
    expect(r.windows[1]).toEqual({ sample: 1, perHour: null })
    // …but it IS one contact in the last hour. A count is a count.
    expect(r.lastHour).toBe(1)
  })

  it('an empty log divides nothing by nothing', () => {
    const r = contestRate([], T0)
    expect(r.windows[0]).toEqual({ sample: 0, perHour: null })
    expect(r.windows[1]).toEqual({ sample: 0, perHour: null })
    expect(r.lastHour).toBe(0)
    // The guard that matters: no NaN, no Infinity reaching a tile.
    for (const w of r.windows) expect(Number.isNaN(w.perHour as number)).toBe(false)
  })

  it('reads only the most recent N of a long log', () => {
    // 150 contacts a minute apart. Both windows see a steady 60/h, but the 100-window
    // must report a sample of 100 — not 150, and not 10.
    const log = run(150, T0, 60)
    const r = contestRate(log, lastOf(log))
    expect(r.windows[0]).toEqual({ sample: 10, perHour: 60 })
    expect(r.windows[1]).toEqual({ sample: 100, perHour: 60 })
    expect(r.lastHour).toBe(60)
  })

  it('a doubled pace shows on the twitchy window before the steady one', () => {
    // 100 contacts at 60/h, then 10 at 120/h. This is the whole reason two windows exist.
    const slow = run(100, T0, 60)
    const fast = run(10, lastOf(slow) + 30, 30)
    const log = [...slow, ...fast]
    const r = contestRate(log, lastOf(log))
    expect(r.windows[0].perHour).toBe(120)
    // The 100-window is dragged by the 90 slow contacts still inside it.
    expect(r.windows[1].perHour).toBeGreaterThan(60)
    expect(r.windows[1].perHour).toBeLessThan(80)
  })
})

describe('the reading falls while nothing is logged', () => {
  it('an hour idle after a 60/h run reads well under 60', () => {
    const log = run(10, T0, 60)
    const atTheLastQso = contestRate(log, lastOf(log)).windows[0].perHour!
    // Control: the same log, read the moment the last contact landed, really is 60.
    expect(atTheLastQso).toBe(60)
    const anHourLater = contestRate(log, lastOf(log) + 3600).windows[0].perHour!
    // 9 intervals over (9 + 60) minutes.
    expect(anHourLater).toBeCloseTo((9 * 3600) / (540 + 3600), 6)
    expect(anHourLater).toBeLessThan(atTheLastQso)
    expect(anHourLater).toBeLessThan(10)
    // …and the rolling hour has emptied out completely.
    expect(contestRate(log, lastOf(log) + 3600).lastHour).toBe(0)
  })
})

describe('a gap in operating', () => {
  it('a two-hour break does not leave a rate implying the operator worked through it', () => {
    // Seven contacts at 60/h, two hours off the air, three more at 60/h.
    const before = run(7, T0, 60)
    const after = run(3, lastOf(before) + 7200, 60)
    const log = [...before, ...after]
    const now = lastOf(log)
    const r = contestRate(log, now)

    // The 10-window spans the gap, so it reports the average across it — 9 intervals
    // over 2h08m — and nothing like the 60/h either burst was run at.
    expect(r.windows[0].sample).toBe(10)
    expect(r.windows[0].perHour).toBeCloseTo((9 * 3600) / (360 + 7200 + 120), 6)
    expect(r.windows[0].perHour!).toBeLessThan(10)
    // Said as the contrast, because "some small number" is not the claim: it must not
    // read anywhere near the run rate on either side of the gap.
    expect(Math.abs(r.windows[0].perHour! - 60)).toBeGreaterThan(40)

    // The rolling hour is the reading that recovers immediately: the three post-gap
    // contacts are inside it, the seven pre-gap ones are two hours behind it.
    expect(r.lastHour).toBe(3)
  })
})

describe('the rolling hour is a count of a real 60 minutes', () => {
  it('includes a contact 59:59 back and excludes one exactly an hour back', () => {
    const log = [q(T0 - 3600), q(T0 - 3599), q(T0)]
    expect(contestRate(log, T0).lastHour).toBe(2)
  })

  it('is the clock hour’s answer only by coincidence — it never resets at the top', () => {
    // 30 contacts at 120/h ending now. A clock-hour count five minutes past the hour
    // would read 10; the rolling hour reads the 30 that actually happened.
    const log = run(30, T0 - 29 * 30, 30)
    expect(contestRate(log, T0).lastHour).toBe(30)
  })
})

describe('rows the arithmetic must not trust', () => {
  it('skips a contact with no timestamp instead of dating it 1970', () => {
    // A row logged before `whenUnix` existed. Defaulted to 0 it would put the window's
    // oldest contact in 1970 and report a rate of essentially zero forever.
    const log = [q(undefined), ...run(3, T0, 60), q(undefined)]
    const r = contestRate(log, lastOf(run(3, T0, 60)))
    expect(r.windows[0]).toEqual({ sample: 3, perHour: 60 })
    // Control: the same log with those rows dated 0 would be read very differently.
    const poisoned = [q(0), ...run(3, T0, 60)]
    expect(contestRate(poisoned, T0 + 120).windows[0]).toEqual({ sample: 3, perHour: 60 })
  })

  it('sorts by time — a club merge interleaves logs, so append order is not time order', () => {
    const ordered = run(3, T0, 60)
    const shuffled = [ordered[2], ordered[0], ordered[1]]
    // The absolute answer, not only agreement with the ordered log — two identically
    // wrong readings agree, and that is not what is being claimed here.
    expect(contestRate(shuffled, T0 + 120).windows[0]).toEqual({ sample: 3, perHour: 60 })
    expect(contestRate(shuffled, T0 + 120)).toEqual(contestRate(ordered, T0 + 120))
  })

  it('refuses a window whose oldest contact is not behind now', () => {
    // A clock that stepped back, or a row stamped in the future. Neither may produce a
    // negative rate or an infinite one.
    const log = run(3, T0 + 600, 60)
    const r = contestRate(log, T0)
    expect(r.windows[0]).toEqual({ sample: 3, perHour: null })
    expect(r.lastHour).toBe(0)
  })

  it('two contacts in the same second do not divide by zero', () => {
    const r = contestRate([q(T0), q(T0)], T0)
    expect(r.windows[0]).toEqual({ sample: 2, perHour: null })
    expect(r.lastHour).toBe(2)
  })
})

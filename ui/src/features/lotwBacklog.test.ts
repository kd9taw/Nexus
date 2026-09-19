// The Logbook's LoTW backlog, moved out of the component (so it is computed once per log, not
// once per render) — pinned to what the two inline filters it replaced counted.
import { describe, expect, it } from 'vitest'
import type { LoggedQso } from '../types'
import { lotwBacklog } from './lotwBacklog'

const q = (over: Partial<LoggedQso>) =>
  ({ call: 'W1AW', band: '20m', mode: 'FT8', whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false, ...over }) as LoggedQso
const lotw = (outcome: string) => ({ lotw: { outcome, whenUnix: 1_700_000_100 } })

describe('lotwBacklog', () => {
  it('counts the unconfirmed never-sent contacts, and the date-only ones apart', () => {
    expect(
      lotwBacklog([
        q({}), // never sent, time known
        q({ timeKnown: true }),
        q({ timeKnown: false }), // a date-only import: LoTW can never match it
        q({ awardConfirmed: true }), // already credited — not owed
        q({ awardConfirmed: true, timeKnown: false }),
      ]),
    ).toEqual({ unsent: 2, timeless: 1 })
  })

  it('a bounce is owed again; a send in flight or landed is not', () => {
    expect(
      lotwBacklog([
        q({ upload: lotw('rejected') }),
        q({ upload: lotw('authfail') }),
        q({ upload: lotw('pending') }),
        q({ upload: lotw('accepted') }),
        q({ upload: lotw('duplicate') }),
        q({ upload: { qrz: { outcome: 'accepted', whenUnix: 1 } } }), // another service's stamp only
      ]),
    ).toEqual({ unsent: 3, timeless: 0 })
  })

  it('an empty log owes nothing', () => {
    expect(lotwBacklog([])).toEqual({ unsent: 0, timeless: 0 })
  })
})

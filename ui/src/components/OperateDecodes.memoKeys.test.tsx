// @vitest-environment jsdom
//
// THE DECODE PANE'S MEMO KEYS. The filter/sort pass (`kept`) is memoised on a hand-written deps
// list — the lint rule is off there, because `histVersion` stands for the history's entries — and
// each row is a `memo` component that redraws only when one of its props moves. An input the pass
// reads but the deps forget, or a row prop frozen at its first value, does not fail loudly: the
// pane simply keeps showing the old list until the next decode arrives, which on a quiet band is a
// whole period or more. The renderCost tests pin that an unchanged poll does NO work; these pin
// the other half — that every input which should move the list does, with nothing new heard.
//
// Every step changes ONE input, then re-polls with an EQUAL decode list (fresh objects, same
// fields: what each snapshot delivers mid-period), and asserts the rows on screen directly. The
// steps are ordered so an earlier toggle never already hides the row a later step targets.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup, act } from '@testing-library/react'
import type { DecodeRow, NeedAlert } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

import { OperateDecodes } from './OperateDecodes'
import { saveCountryExclude } from '../features/countryExclude'
import { saveHideCalls } from '../features/hideCalls'
import { DE } from '../i18n/de'
import { installCatalog, setLocale, t } from '../i18n'

const decode = (over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

/** One period, as a FRESH array of FRESH rows — equal to the last poll's, never the same objects. */
const rows = (): DecodeRow[] => [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31', freqHz: 1200, grid: 'FN31', country: 'United States' }),
  decode({ from: 'JA1XYZ', message: 'CQ DX JA1XYZ PM95', freqHz: 900, snr: -18, grid: 'PM95', country: 'Japan', worked: true }),
  decode({ from: 'K1ABC', message: 'K1ABC DL1QQ R-07', freqHz: 1500, isCq: false, country: 'United States', worked: true, workedBand: true }),
  decode({ from: 'DL1QQ', message: 'DL1QQ K1ABC RR73', freqHz: 1230, isCq: false, signoff: true, country: 'Fed. Rep. of Germany', confirmedBand: true }),
  decode({ from: 'VK2AAA', message: 'KD0XX VK2AAA -05', freqHz: 2000, isCq: false, directedToMe: true, country: 'Australia', worked: true }),
  decode({ from: 'F5ZZZ', message: 'CQ F5ZZZ JN18', freqHz: 1700, grid: 'JN18', country: 'France' }),
  decode({ from: 'KD0XX', message: 'CQ KD0XX EN34', freqHz: 1210, mine: true, txAt: 1_470 }),
]
// The own-transmit row was sent in slot 98 (1,470 s on the pane's 15 s clock), the period before
// the one these decodes were heard in, so it sorts first.
const ALL = ['KD0XX', 'W1AW', 'JA1XYZ', 'K1ABC', 'DL1QQ', 'VK2AAA', 'F5ZZZ']

const alert = (call: string, tags: NeedAlert['tags']): NeedAlert =>
  ({ call, entity: 'X', band: '20m', zone: 1, tags, priority: 5, headline: 'h', mode: 'Digital', freqMhz: null }) as NeedAlert

type Props = React.ComponentProps<typeof OperateDecodes>

function pane() {
  let p: Props = {
    decodes: rows(),
    slot: 100,
    rxOffsetHz: 1200,
    band: '20m',
    tier: 'FT8',
    periodSecs: 15,
    harqRescues: 0,
    onCall: () => {},
  }
  const v = render(<OperateDecodes {...p} />)
  /** The next snapshot: equal decodes, plus whatever host prop this step changes. */
  const poll = (change: Partial<Props> = {}) => {
    p = { ...p, ...change, decodes: rows() }
    v.rerender(<OperateDecodes {...p} />)
  }
  /** A gesture or shared setting, then the poll after it. */
  const doThen = (fn: () => void) => {
    act(fn)
    poll()
  }
  const calls = () =>
    Array.from(v.container.querySelectorAll('.decode-row')).map((r) => (r.getAttribute('aria-label') ?? '').split(',')[0])
  const row = (call: string) =>
    Array.from(v.container.querySelectorAll('.decode-row')).find((r) =>
      (r.getAttribute('aria-label') ?? '').startsWith(`${call},`),
    ) as HTMLElement | undefined
  const chip = (cls: string) => v.container.querySelector(`.${cls}`) as HTMLElement
  const without = (...gone: string[]) => ALL.filter((c) => !gone.includes(c))
  return { poll, doThen, calls, row, chip, without }
}

beforeEach(() => {
  localStorage.clear()
})
afterEach(() => {
  cleanup()
  act(() => setLocale('en'))
})

describe('the decode list follows every input with no new decodes', () => {
  it('each pipeline input and row prop moves the list on an equal re-poll', () => {
    const h = pane()
    expect(h.calls()).toEqual(ALL)
    h.poll()
    expect(h.calls()).toEqual(ALL)

    // Wildcard call-hide (a shared setting): VK2AAA goes, and comes back.
    h.doThen(() => saveHideCalls(['VK*']))
    expect(h.calls()).toEqual(h.without('VK2AAA'))
    h.doThen(() => saveHideCalls([]))
    expect(h.calls()).toEqual(ALL)

    // Country exclusion (a shared setting), BEFORE hide-confirmed, which would already hide
    // DL1QQ. The station in the Tx panel is never hidden by it, so the selection is a key too.
    h.doThen(() => saveCountryExclude(['dl']))
    expect(h.calls()).toEqual(h.without('DL1QQ'))
    h.poll({ selectedCall: 'DL1QQ' })
    expect(h.calls()).toEqual(ALL)
    h.poll({ selectedCall: null })
    expect(h.calls()).toEqual(h.without('DL1QQ'))
    h.doThen(() => saveCountryExclude([]))
    expect(h.calls()).toEqual(ALL)

    // Need chips arrive, change and clear with the rows unchanged.
    expect(h.row('W1AW')!.querySelector('.need-chip')).toBeNull()
    h.poll({ needAlertsByCall: new Map([['W1AW', [alert('W1AW', ['NewEntity'])]]]) })
    expect(h.row('W1AW')!.querySelector('.need-chip')).not.toBeNull()
    expect(h.row('JA1XYZ')!.querySelector('.need-chip')).toBeNull()
    h.poll({ needAlertsByCall: new Map([['JA1XYZ', [alert('JA1XYZ', ['NewEntity'])]]]) })
    expect(h.row('W1AW')!.querySelector('.need-chip')).toBeNull()
    expect(h.row('JA1XYZ')!.querySelector('.need-chip')).not.toBeNull()
    h.poll({ needAlertsByCall: new Map() })
    expect(h.row('JA1XYZ')!.querySelector('.need-chip')).toBeNull()

    // The language: a row whose data did not move still redraws its words.
    const enTitle = h.row('W1AW')!.title
    expect(enTitle).toBe(t('operate.decodes.row.title', { call: 'W1AW', highlight: '' }))
    installCatalog('de', DE)
    h.doThen(() => setLocale('de'))
    const deTitle = t('operate.decodes.row.title', { call: 'W1AW', highlight: '' })
    expect(deTitle).not.toBe(enTitle)
    expect(h.row('W1AW')!.title).toBe(deTitle)
    h.doThen(() => setLocale('en'))
    expect(h.row('W1AW')!.title).toBe(enTitle)

    // −B4 hides the worked stations (never my own row or one calling me) …
    h.doThen(() => fireEvent.click(h.chip('od-b4')))
    expect(h.calls()).toEqual(h.without('JA1XYZ', 'K1ABC'))
    // … except the live QSO partner, for exactly as long as it is the partner.
    h.poll({ partnerCall: 'JA1XYZ' })
    expect(h.calls()).toEqual(h.without('K1ABC'))
    h.poll({ partnerCall: null })
    expect(h.calls()).toEqual(h.without('JA1XYZ', 'K1ABC'))
    h.doThen(() => fireEvent.click(h.chip('od-b4')))
    expect(h.calls()).toEqual(ALL)

    // Hide confirmed on this band.
    h.doThen(() => fireEvent.click(h.chip('od-conf')))
    expect(h.calls()).toEqual(h.without('DL1QQ'))
    h.doThen(() => fireEvent.click(h.chip('od-conf')))
    expect(h.calls()).toEqual(ALL)

    // Hide blocked, then the blocked list itself moves under it.
    h.poll({ ignoredCalls: new Set(['F5ZZZ']) })
    expect(h.calls()).toEqual(ALL)
    h.doThen(() => fireEvent.click(h.chip('od-blocked')))
    expect(h.calls()).toEqual(h.without('F5ZZZ'))
    h.poll({ ignoredCalls: new Set(['W1AW']) })
    expect(h.calls()).toEqual(h.without('W1AW'))
    h.doThen(() => fireEvent.click(h.chip('od-blocked')))
    expect(h.calls()).toEqual(ALL)
  })
})

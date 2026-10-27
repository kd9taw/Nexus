// @vitest-environment jsdom
//
// A received line's time is the START of the period it was heard in, as WSJT-X's Band Activity and
// ALL.TXT stamp it, whenever the line reaches the list; when it arrived goes in the tooltip. A
// full-pass decode of the :15 period lands at :31 to :34, and stamped with its arrival it read as
// heard in the :30 period.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow, LateDecodes, Tier } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** 2023-11-14 22:13:00 UTC, in Unix seconds. */
const T221300 = 1_699_999_980

function row(message: string, over: Partial<DecodeRow> = {}): DecodeRow {
  return {
    from: message.split(' ')[1],
    snr: -10,
    dtSec: 0.1,
    freqHz: 1200,
    message,
    isCq: true,
    directedToMe: false,
    worked: false,
    tier: 'FT8',
    rv: -1,
    ...over,
  }
}

/** The clock at `secs` Unix seconds while the pane takes in a poll. */
function at(secs: number) {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime(secs * 1000)
}

/** A Band Activity pane on `tier`, its live period `periodSecs`. */
function pane(tier: Tier, periodSecs: number | undefined) {
  const props = { rxOffsetHz: 1500, band: '20m', tier, periodSecs, harqRescues: 0, onCall: () => {} }
  const view = render(<OperateDecodes decodes={[]} slot={0} {...props} />)
  const poll = (decodes: DecodeRow[], slot: number, late: LateDecodes | null = null) =>
    view.rerender(<OperateDecodes decodes={decodes} slot={slot} late={late} {...props} />)
  const timeOf = (message: string) => {
    const r = [...view.container.querySelectorAll('.decode-row')].find((el) =>
      el.textContent?.includes(message),
    )
    expect(r, `no row for ${message}`).toBeDefined()
    const utc = r!.querySelector('.decode-utc') as HTMLElement
    return { text: utc.textContent, title: utc.getAttribute('title') }
  }
  const separators = () =>
    [...view.container.querySelectorAll('.od-period-sep .od-sep-utc')].map((s) => s.textContent)
  return { poll, timeOf, separators }
}

describe('the time column shows the period a line was heard in', () => {
  it('an FT8 line of the :15 period that arrives at :34 shows 221315, the arrival in its tooltip', () => {
    const p = pane('FT8', 15)
    // The engine files the 22:13:15 period's decodes under the next slot (the audio's + 1).
    const slot = (T221300 + 15) / 15 + 1
    at(T221300 + 34)
    p.poll([row('CQ W1AW FN31')], slot)
    expect(p.timeOf('CQ W1AW FN31')).toEqual({
      text: '221315',
      title: 'Start of the period it was heard in (UTC). Arrived at 221334.',
    })
    // Polled again later, the line keeps both its period and its first arrival.
    at(T221300 + 40)
    p.poll([row('CQ W1AW FN31')], slot)
    expect(p.timeOf('CQ W1AW FN31').text).toBe('221315')
    expect(p.timeOf('CQ W1AW FN31').title).toContain('221334')
  })

  it('an early-pass line, inside its own period, shows the same period start', () => {
    const p = pane('FT8', 15)
    at(T221300 + 27)
    p.poll([row('CQ W1AW FN31')], (T221300 + 15) / 15 + 1)
    expect(p.timeOf('CQ W1AW FN31').text).toBe('221315')
  })

  it('dates a 30 s MSK144 period by the live period, the separator too, not the table 15 s', () => {
    const p = pane('MSK144', 30)
    const slot = T221300 / 30 + 1 // the 22:13:00 period
    at(T221300 + 31)
    p.poll([row('CQ W1AW FN31', { tier: 'MSK144' })], slot)
    at(T221300 + 61)
    p.poll([row('CQ K2DEF FN20', { tier: 'MSK144' })], slot + 1)
    expect(p.timeOf('CQ W1AW FN31').text).toBe('221300')
    expect(p.timeOf('CQ K2DEF FN20').text).toBe('221330')
    expect(p.separators()).toEqual(['221330'])
  })

  it('without a live period (a station that sends none) the tier period stands in', () => {
    const p = pane('FT8', 0)
    at(T221300 + 34)
    p.poll([row('CQ W1AW FN31')], (T221300 + 15) / 15 + 1)
    expect(p.timeOf('CQ W1AW FN31').text).toBe('221315')
  })

  it('a line heard before a band or mode change shows the period it was heard in', () => {
    // Heard on FT8 in the 22:13:15 period, shown after a change to FT4. The engine sends its slot
    // in FT4's numbering, which dates it 22:13:22: only the period it carries is right.
    const p = pane('FT4', 7.5)
    at(T221300 + 49)
    p.poll([], (T221300 + 45) / 7.5 + 1, {
      band: '40m',
      dialMhz: 7.074,
      tier: 'FT8',
      periodStartMs: (T221300 + 15) * 1000,
      slot: (((T221300 + 15) / 15 + 1) * 15) / 7.5,
      rows: [row('CQ W9XYZ EN52')],
    })
    expect(p.timeOf('CQ W9XYZ EN52')).toEqual({
      text: '221315',
      title: 'Start of the period it was heard in (UTC). Arrived at 221349.',
    })
  })

  it('an own transmission still shows when it was sent', () => {
    const p = pane('FT8', 15)
    at(T221300 + 40)
    p.poll([row('CQ K9ABC EN52', { mine: true, txAt: T221300 + 30, from: 'K9ABC' })], (T221300 + 15) / 15 + 1)
    expect(p.timeOf('CQ K9ABC EN52')).toEqual({ text: '221330', title: 'UTC heard' })
  })
})

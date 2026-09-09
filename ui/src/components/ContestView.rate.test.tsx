// @vitest-environment jsdom
//
// THE RATE METER ON SCREEN — the real `ContestView`, with the props `App` gives it
// (`App.tsx:2486`: fieldDay, onSetMode, fdActive, fdRuleset, tier).
//
// ⚠️ NOT A STUBBED COMPONENT. A stubbed child proves prop-passing and nothing else, and
// this project shipped a broken Parks toggle through five green reviews exactly that way.
// Every assertion below reads the rendered tile — its accessible name and the number
// inside it — off a full ContestView render.
//
// `features/contestRate.test.ts` owns the arithmetic. What is checked here is what the
// arithmetic cannot be: the tile is drawn, in the scoreboard, for a Field Day event and
// for a contest with multipliers alike; the label names the REAL sample; and an empty log
// draws a dash rather than a zero rate.

import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  fdClubExport: vi.fn(async () => ''),
  fdSetUpload: vi.fn(async () => ({})),
  saveTextToDownloads: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

/** The component reads the real clock, so the fixtures are built backwards from it. */
const nowSecs = () => Math.floor(Date.now() / 1000)

const qso = (whenUnix: number, i: number): FieldDayQso =>
  ({
    call: `W1AW/${i}`,
    class: '1A',
    section: 'IL',
    band: '20m',
    mode: 'CW',
    whenUnix,
  }) as FieldDayQso

/** `n` contacts `every` seconds apart, the last of them `endsAgo` seconds before now. */
const run = (n: number, every: number, endsAgo = 0): FieldDayQso[] => {
  const last = nowSecs() - endsAgo
  return Array.from({ length: n }, (_, i) => qso(last - (n - 1 - i) * every, i))
}

const FD = (log: FieldDayQso[], over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    composing: [
      { key: 'CLASS', raw: '1A' },
      { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
    ],
    running: true,
    state: 'sp',
    event: 'arrlfd',
    qsoCount: log.length,
    sections: 1,
    points: log.length * 2,
    workedSections: ['IL'],
    log,
    boards: [
      { id: 'SECTION', slot: 'SECTION', domain: 'fd_sections', scope: 'perLog', worked: ['IL'] },
    ],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    ...over,
  }) as unknown as FieldDayStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

/** Render with the props App passes, and hand back the tile with this accessible name. */
const showFd = async (fd: FieldDayStatus) => {
  render(
    <ContestView fieldDay={fd} onSetMode={() => {}} fdActive fdRuleset={null} tier="none" />,
  )
  await settle()
}

/** The number inside a tile — `.fd-score-val`, the shipped tile idiom. */
const tileValue = (el: HTMLElement) => el.querySelector('.fd-score-val')!.textContent

afterEach(cleanup)

describe('the rate meter renders on the contest scoreboard', () => {
  it('shows the last-10 and rolling-hour readings for a Field Day event', async () => {
    // Ten contacts a minute apart, the last of them just now: 9 intervals in 9 minutes.
    await showFd(FD(run(10, 60)))
    const tile = screen.getByLabelText('Rate over the last 10 contacts')
    expect(tileValue(tile)).toBe('60')
    expect(tile.textContent).toContain('Last 10')
    expect(tileValue(screen.getByLabelText('Contacts in the last 60 minutes'))).toBe('10')
    // In the scoreboard, beside the counts — not floated off somewhere of its own.
    expect(tile.closest('.fd-scoreboard')).not.toBeNull()
    expect(tile.closest('.fd-scoreboard')!.textContent).toContain('QSOs')
  })

  it('renders for a contest WITH multipliers too — every contest the picker offers', async () => {
    // The multiplier arm of the scoreboard (a QSO party): the meter is not Field-Day-only.
    await showFd(FD(run(10, 60), { event: 'tnqp', multCount: 4, sections: 0 }))
    expect(tileValue(screen.getByLabelText('Rate over the last 10 contacts'))).toBe('60')
    expect(screen.getByLabelText('Contacts in the last 60 minutes')).not.toBeNull()
  })

  it('shows the 100-window as its own tile once the log can tell them apart', async () => {
    // ONE timeline: 100 contacts a minute apart, then 10 more every 30 s ending now.
    // The twitchy window sees the new pace (120/h); the steady one is still dragged by
    // the 90 slow contacts inside it. Two windows, two different numbers on screen —
    // which is the only reason the second tile exists.
    const now = nowSecs()
    const gaps = [...Array(99).fill(60), ...Array(10).fill(30)]
    const stamps: number[] = [now]
    for (const g of [...gaps].reverse()) stamps.unshift(stamps[0] - g)
    await showFd(FD(stamps.map(qso)))
    expect(tileValue(screen.getByLabelText('Rate over the last 10 contacts'))).toBe('120')
    const hundred = screen.getByLabelText('Rate over the last 100 contacts')
    expect(Number(tileValue(hundred))).toBeGreaterThan(60)
    expect(Number(tileValue(hundred))).toBeLessThan(90)
  })
})

describe('too few contacts', () => {
  it('labels the tile with the REAL sample — "Last 3", never "Last 10"', async () => {
    await showFd(FD(run(3, 60)))
    // The claim, stated both ways: the honest label is present…
    const tile = screen.getByLabelText('Rate over the last 3 contacts')
    expect(tile.textContent).toContain('Last 3')
    expect(tileValue(tile)).toBe('60')
    // …and the fabricated one is not. A "Last 10" here would be a claim about seven
    // contacts that were never made.
    expect(screen.queryByLabelText('Rate over the last 10 contacts')).toBeNull()
  })

  it('draws ONE window tile while both windows hold the same contacts', async () => {
    await showFd(FD(run(3, 60)))
    // Two tiles with the same label and the same number are one reading shown twice.
    expect(screen.getAllByLabelText(/^Rate over the last /)).toHaveLength(1)
  })

  it('one contact is a dash, not a rate — and one contact in the hour', async () => {
    await showFd(FD(run(1, 60)))
    expect(tileValue(screen.getByLabelText('Rate over the last 1 contacts'))).toBe('—')
    expect(tileValue(screen.getByLabelText('Contacts in the last 60 minutes'))).toBe('1')
  })

  it('an empty log draws a dash and a real zero, and no NaN', async () => {
    await showFd(FD([]))
    const tile = screen.getByLabelText('Rate over the last 0 contacts')
    expect(tileValue(tile)).toBe('—')
    // The rolling hour is a COUNT: zero contacts in the last hour is an answer.
    expect(tileValue(screen.getByLabelText('Contacts in the last 60 minutes'))).toBe('0')
    expect(screen.queryByText('NaN')).toBeNull()
    expect(screen.queryByText('Infinity')).toBeNull()
  })
})

describe('after a break in operating', () => {
  it('does not show the rate the operator was running before a two-hour gap', async () => {
    // Seven contacts at 60/h, two hours off the air, three more at 60/h ending now.
    const before = run(7, 60, 7200 + 120)
    const after = run(3, 60)
    await showFd(FD([...before, ...after]))
    const rate = Number(tileValue(screen.getByLabelText('Rate over the last 10 contacts')))
    // The contrast is the assertion: it must not read anything like the 60/h either
    // burst was actually run at.
    expect(rate).toBeLessThan(10)
    expect(Math.abs(rate - 60)).toBeGreaterThan(40)
    // The rolling hour is the reading that recovered — the three post-gap contacts.
    expect(tileValue(screen.getByLabelText('Contacts in the last 60 minutes'))).toBe('3')
  })

  it('a run that stopped an hour ago has decayed, because the window ends at NOW', async () => {
    // The same ten contacts as the 60/h case, but the last one landed an hour ago. A
    // meter that measured only between logged contacts would still be showing 60.
    await showFd(FD(run(10, 60, 3600)))
    const rate = Number(tileValue(screen.getByLabelText('Rate over the last 10 contacts')))
    expect(rate).toBeLessThan(10)
    expect(tileValue(screen.getByLabelText('Contacts in the last 60 minutes'))).toBe('0')
  })
})

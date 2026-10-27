// @vitest-environment jsdom
//
// THE CONTEST STATS TAB — the Contest screen's second view, by value, from a fixture contest log: the
// totals, the hourly bars, the countries the station places each call in and the distances to them,
// the contest's own multiplier board (worked and needed), and "Share stats" saving the drawn card to
// Downloads. Read-only: nothing here may log, remove, merge or key anything.
//
// ⚠️ jsdom lays out nothing; that the tab fits the window is the browser measurement's to say.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, act, cleanup, fireEvent, within } from '@testing-library/react'
import { ContestView } from './ContestView'
import { t } from '../i18n'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  contestStatsPlaces: vi.fn(async (calls: string[]) =>
    calls.map((c) =>
      c.startsWith('DL')
        ? { entity: 'Fed. Rep. of Germany', lat: 51, lon: 10 }
        : c.startsWith('JA')
          ? { entity: 'Japan', lat: 36, lon: 138 }
          : c.startsWith('W') || c.startsWith('K')
            ? { entity: 'United States', lat: 37.53, lon: -91.67 }
            : null,
    ),
  ),
  savePngToDownloads: vi.fn(async (name: string) => `/home/op/Downloads/${name}`),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

/** 2026-11-28 00:00 UTC. */
const T0 = 1_795_824_000
const q = (call: string, min: number, band: string, county: string, extra: Partial<FieldDayQso> = {}): FieldDayQso => ({
  call,
  class: '',
  section: '',
  band,
  mode: 'CW',
  whenUnix: T0 + min * 60,
  rcvd: ['599', county],
  ...extra,
})

/** An Illinois QSO Party: six contacts, one a dupe, over three UTC hours of a four-hour window. */
const LOG = [
  q('K9AAA', 5, '40m', 'COOK'),
  q('W9BBB', 20, '40m', 'KANE'),
  q('K9AAA', 21, '40m', 'COOK', { dupe: true }),
  q('DL1ABC', 70, '20m', 'DX'),
  q('JA1ABC', 75, '20m', 'DX'),
  q('DL2XYZ', 190, '20m', 'DX', { mode: 'PH', submode: 'USB' }),
]
const FD = {
  running: true,
  state: 'running',
  event: 'ilqp',
  qsoCount: 5,
  sections: 0,
  points: 9,
  multCount: 3,
  totalScore: 27,
  log: LOG,
  composing: [],
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  boards: [{ id: 'QTH', slot: 'QTH', domain: 'il_counties', scope: 'perLog', worked: ['COOK', 'KANE'] }],
  eventStartUnix: T0,
  eventEndUnix: T0 + 4 * 3600,
} as unknown as FieldDayStatus

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] })
  vi.setSystemTime((T0 + 200 * 60) * 1000)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.clearAllMocks()
})

async function settle(): Promise<void> {
  await act(async () => {
    for (let i = 0; i < 10; i++) await Promise.resolve()
  })
}

async function openStats(): Promise<HTMLElement> {
  render(
    <main className="layout single">
      <ContestView fieldDay={FD} onSetMode={() => {}} fdActive />
    </main>,
  )
  await settle()
  const tab = screen.getByRole('tab', { name: t('contestStats.tab.stats') })
  expect(tab.getAttribute('aria-selected')).toBe('false')
  fireEvent.click(tab)
  await settle()
  expect(tab.getAttribute('aria-selected')).toBe('true')
  return screen.getByRole('tabpanel')
}

const tile = (panel: HTMLElement, label: string, value: string | number) =>
  within(panel).getByRole('group', { name: t('contestStats.tile.aria', { label, value }) })

describe('the Contest screen’s Stats tab', () => {
  it('starts on the scoreboard and opens the stats in its place, the log staying below', async () => {
    const panel = await openStats()
    // The scoreboard's operator box is gone with the scoreboard…
    expect(screen.queryByLabelText(t('fieldDay.operator.aria'))).toBeNull()
    // …and the log is still there.
    expect(screen.getAllByText('DL2XYZ').length).toBeGreaterThan(0)
    expect(panel).toBeTruthy()
  })

  it('counts the totals by value, the dupe left out', async () => {
    const panel = await openStats()
    tile(panel, t('contestStats.contacts'), '5')
    // Gaps 15, 1, 49, 5, 115 minutes (the dupe's time counts: the station was on the air): 49 and
    // 115 are breaks of 30 minutes or more, the rest 21 minutes on.
    tile(panel, t('contestStats.onTime'), '0:21')
    tile(panel, t('contestStats.breaks', { count: 2 }), '2:44')
    // 5 contacts in 21 minutes on: 14.3/h, shown as 14.
    tile(panel, t('contestStats.overall'), t('contestStats.perHour', { rate: 14 }))
    // 5, 20, 70, 75 → the best hour from 0:20 holds 20, 70, 75: three.
    tile(panel, t('contestStats.best60'), '3')
    tile(panel, t('contestStats.sinceLast'), '0:10')
    tile(panel, t('contestStats.today'), '5')
    expect(within(panel).getByText(t('contestStats.offRule.default', { minutes: 30 }))).toBeTruthy()
  })

  it('draws one bar per UTC hour of the window', async () => {
    const panel = await openStats()
    const bars = within(panel).getAllByRole('listitem').filter((li) => li.closest('.contest-stats-hours'))
    expect(bars.map((b) => b.getAttribute('aria-label'))).toEqual([
      t('contestStats.hours.bar', { hour: '00:00', count: 2 }),
      t('contestStats.hours.bar', { hour: '01:00', count: 2 }),
      t('contestStats.hours.bar', { hour: '02:00', count: 0 }),
      t('contestStats.hours.bar', { hour: '03:00', count: 1 }),
    ])
  })

  it('places the calls in one ask and counts bands, modes, countries, DX and distance', async () => {
    const panel = await openStats()
    expect(api.contestStatsPlaces).toHaveBeenCalledTimes(1)
    const [calls, event] = api.contestStatsPlaces.mock.calls[0]
    expect(event).toBe('ilqp')
    expect([...calls].sort()).toEqual(['DL1ABC', 'DL2XYZ', 'JA1ABC', 'K9AAA', 'W9BBB', 'W9XYZ'])

    const bands = within(panel).getByRole('list', { name: t('contestStats.bands.head') })
    expect(within(bands).getAllByRole('listitem').map((li) => li.getAttribute('aria-label'))).toEqual([
      t('contestStats.bar.aria', { name: '20m', count: 3 }),
      t('contestStats.bar.aria', { name: '40m', count: 2 }),
    ])
    const modes = within(panel).getByRole('list', { name: t('contestStats.modes.head') })
    expect(within(modes).getAllByRole('listitem').map((li) => li.getAttribute('aria-label'))).toEqual([
      t('contestStats.bar.aria', { name: 'CW', count: 4 }),
      t('contestStats.bar.aria', { name: 'USB', count: 1 }),
    ])
    const countries = within(panel).getByRole('list', { name: t('contestStats.dx.head') })
    expect(within(countries).getAllByRole('listitem').map((li) => li.getAttribute('aria-label'))).toEqual([
      t('contestStats.bar.aria', { name: 'Fed. Rep. of Germany', count: 2 }),
      t('contestStats.bar.aria', { name: 'United States', count: 2 }),
      t('contestStats.bar.aria', { name: 'Japan', count: 1 }),
    ])
    tile(panel, t('contestStats.dx.entities'), 3)
    tile(panel, t('contestStats.dx.dx'), 3)
    // The longest is Japan from EN52, about 10,100 km (6,280 mi): the units are the app's.
    expect(within(panel).getByText('JA1ABC').closest('p')?.textContent).toBe('JA1ABC · Japan')
  })

  it('shows the contest’s own multiplier board, worked and needed', async () => {
    const panel = await openStats()
    const mults = within(panel).getByRole('region', { name: t('contestStats.mults.head') })
    const cells = Array.from(mults.querySelectorAll('[data-worked]'))
    const worked = cells.filter((c) => c.getAttribute('data-worked') !== 'none').map((c) => c.textContent?.replace('✓', ''))
    expect(worked.sort()).toEqual(['COOK', 'KANE'])
    expect(cells.length).toBeGreaterThan(100)
    // The score: points × mults = total, the engine's numbers.
    const score = within(panel).getByRole('region', { name: t('contestStats.score.head') })
    expect(score.textContent).toContain('9')
    expect(score.textContent).toContain('27')
  })

  it('saves the drawn card to Downloads when Share is pressed, and posts nothing', async () => {
    const drawn: string[] = []
    const ctx = new Proxy(
      {},
      {
        get: (_o, k) =>
          k === 'fillText'
            ? (s: string) => drawn.push(s)
            : k === 'createLinearGradient'
              ? () => ({ addColorStop: () => {} })
              : () => {},
        set: () => true,
      },
    )
    const getContext = vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx as never)
    const toDataURL = vi.spyOn(HTMLCanvasElement.prototype, 'toDataURL').mockReturnValue('data:image/png;base64,Q0FSRA==')
    try {
      const panel = await openStats()
      fireEvent.click(within(panel).getByRole('button', { name: t('contestStats.share') }))
      await settle()
      expect(api.savePngToDownloads).toHaveBeenCalledTimes(1)
      expect(api.savePngToDownloads).toHaveBeenCalledWith('W9XYZ-ilqp-stats-2026-11-28.png', 'Q0FSRA==')
      // The card drawn is this contest's: the call, the contacts and the score on it.
      expect(drawn).toContain('W9XYZ')
      expect(drawn).toContain('5')
      expect(drawn).toContain('27')
      expect(drawn).toContain(t('contestStats.contacts'))
    } finally {
      getContext.mockRestore()
      toDataURL.mockRestore()
    }
  })

  it('in CQ WW takes the contest’s own 60-minute off time: a 45-minute gap is on time, 61 minutes a break', async () => {
    // 0 → 45 (45 min: on in CQ WW, a break under the 30-minute default) → 106 (61 min: a break).
    const cqww = {
      ...FD,
      event: 'cqww_cw',
      receives: [{ key: 'RST', kind: 'rst', required: true }, { key: 'ZONE', kind: 'number', required: true, min: 1, max: 40 }],
      boards: [],
      log: [q('DL1ABC', 0, '20m', '14'), q('JA1ABC', 45, '20m', '25'), q('W9BBB', 106, '20m', '4')],
      qsoCount: 3,
    } as unknown as FieldDayStatus
    render(
      <main className="layout single">
        <ContestView fieldDay={cqww} onSetMode={() => {}} fdActive />
      </main>,
    )
    await settle()
    // The scoreboard's own tiles follow the same rule.
    expect(screen.getByRole('group', { name: t('fieldDay.opTime.on.aria', { time: '0:45' }) })).toBeTruthy()
    fireEvent.click(screen.getByRole('tab', { name: t('contestStats.tab.stats') }))
    await settle()
    const panel = screen.getByRole('tabpanel')
    tile(panel, t('contestStats.onTime'), '0:45')
    tile(panel, t('contestStats.breaks', { count: 1 }), '1:01')
    expect(within(panel).getByText(t('contestStats.offRule', { minutes: 60 }))).toBeTruthy()
    expect(within(panel).queryByText(t('contestStats.offRule.default', { minutes: 30 }))).toBeNull()
  })

  it('names the 30-minute default as a default where the contest has no minimum on record', async () => {
    const panel = await openStats()
    expect(within(panel).getByText(t('contestStats.offRule.default', { minutes: 30 }))).toBeTruthy()
  })
})

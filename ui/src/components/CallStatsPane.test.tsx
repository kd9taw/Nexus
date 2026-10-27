// @vitest-environment jsdom
// THE STATS PANE draws what the station counted for the call typed in the log entry
// (`get_call_stats`): the three flags, the confirmations per service for the country and for the
// call, and one grid per award whose slots are unworked / worked / confirmed / no credit. It asks
// once per call (and again when the log changes), and never shows an answer to an older question.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, waitFor, act } from '@testing-library/react'
import { CallStatsPane } from './CallStatsPane'
import type { CallStats, QslChannels, StatsSlot } from '../types'
import { EN } from '../i18n'
import { getCallStats } from '../api'

vi.mock('../api', () => ({ getCallStats: vi.fn() }))

const BANDS = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '70cm', '33cm', '23cm']
/** The bands DXCC keeps no slot on, as the station answers them. */
const NO_DXCC_SLOT = new Set(['60m', '4m', '70cm', '33cm', '23cm'])
const NONE: QslChannels = { card: false, lotw: false, eqsl: false, eqslAg: false, qrz: false }

/** A row of slots: every band unworked but the ones named (no credit where DXCC keeps no slot, when `dxcc`). */
function row(dxcc: boolean, filled: Record<string, StatsSlot> = {}): StatsSlot[] {
  return BANDS.map((b) => filled[b] ?? (dxcc && NO_DXCC_SLOT.has(b) ? 'noCredit' : 'unworked'))
}

/** Navassa, as the station answers for KP1AA on 20 m in CW after K1N's 2015 contact (DXCC 182). */
const NAVASSA: CallStats = {
  call: 'KP1AA',
  bands: BANDS,
  modes: ['CW', 'Phone', 'Digital'],
  newOne: false,
  newBand: true,
  newMode: false,
  dxcc: {
    label: 'Navassa Island',
    code: 182,
    rows: [row(true, { '20m': 'confirmed' }), row(true, { '40m': 'worked' }), row(true)],
  },
  wasFamily: false,
  was: null,
  waz: { label: '8', code: null, rows: [row(false, { '20m': 'confirmed', '60m': 'worked' }), row(false), row(false)] },
  countryQsl: { ...NONE, lotw: true, eqsl: true, eqslAg: true },
  callQsl: { ...NONE, card: true },
}

// A block, not an arrow's value: a function a beforeEach returns is run as its cleanup, and that would call
// the mock once more after every test.
beforeEach(() => {
  vi.mocked(getCallStats).mockReset()
})
afterEach(cleanup)

/** The slots one award's grid draws, row by row, by its `data-slot`. */
function slots(award: string): string[][] {
  const head = [...document.querySelectorAll('.cstats-award-head')].find((h) => h.textContent?.startsWith(award))
  const body = head?.closest('tbody')
  expect(body, `no ${award} grid`).toBeTruthy()
  return [...body!.querySelectorAll('tr')].map((tr) => [...tr.querySelectorAll('td.cstats-slot')].map((td) => td.getAttribute('data-slot')!))
}

/** Each award's head: its name, what stands beside it (the entity's number, the state, the zone, or
 *  a mark), and why it has no grid (its title). */
function heads(): [string, string, string | null][] {
  return [...document.querySelectorAll('.cstats-award-head')].map((h) => [
    h.firstChild!.textContent!,
    h.querySelector('.cstats-award-of')!.textContent!,
    h.getAttribute('title'),
  ])
}

/** The lines the pane draws, counted: the top line's parts and flags, the sources' rows, the grid's
 *  rows and slots. */
function shape(): number[] {
  return [
    document.querySelectorAll('.cstats-top > *').length,
    document.querySelectorAll('.cstats-flag').length,
    document.querySelectorAll('.cstats-src tr').length,
    document.querySelectorAll('.cstats-grid tr').length,
    document.querySelectorAll('td.cstats-slot').length,
  ]
}
/** Two parts and three flags; a head row and two source rows; a head row and three awards' three mode
 *  rows; nine rows of sixteen bands. */
const SHAPE = [2, 3, 3, 10, 9 * BANDS.length]

describe('the Stats pane', () => {
  it('asks nothing and says what it is for while the log entry holds no call', () => {
    render(<CallStatsPane call="" band="20m" mode="CW" />)
    expect(screen.getByText(EN['callStats.empty'])).toBeTruthy()
    expect(getCallStats).not.toHaveBeenCalled()
  })

  it('asks the station for the call typed, on the band and in the mode, and draws its answer by value', async () => {
    vi.mocked(getCallStats).mockResolvedValue(NAVASSA)
    render(<CallStatsPane call=" kp1aa" band="20m" mode="CW" logTick={3} />)
    await screen.findByText('Navassa Island · DXCC 182')
    expect(getCallStats).toHaveBeenCalledWith('KP1AA', '20m', 'CW')
    // The flags: lit and unlit, by value.
    const flags = [...document.querySelectorAll('.cstats-flag')].map((f) => [f.getAttribute('data-kind'), f.getAttribute('data-on')])
    expect(flags).toEqual([
      ['one', 'false'],
      ['band', 'true'],
      ['mode', 'false'],
    ])
    // The grids: DXCC (no credit where it keeps no band slot) and WAZ; WAS keeps its lines for a station outside the
    // family, marked and explained.
    expect(slots('DXCC')).toEqual([row(true, { '20m': 'confirmed' }), row(true, { '40m': 'worked' }), row(true)])
    expect(slots('WAZ')).toEqual([row(false, { '20m': 'confirmed', '60m': 'worked' }), row(false), row(false)])
    expect(slots('WAS')).toEqual(Array(3).fill(Array(BANDS.length).fill('none')))
    expect(heads()).toEqual([
      ['DXCC', '182', null],
      ['WAS', '—', 'WAS: KP1AA is not a station in the United States.'],
      ['WAZ', '8', null],
    ])
    // The band the cockpit is on is marked down its column: one head and the nine slots under it.
    expect([...document.querySelectorAll('[data-here]')].map((e) => e.tagName)).toEqual(['TH', ...Array(9).fill('TD')])
    // Every slot says its state in words.
    const confirmed = document.querySelector('td.cstats-slot[data-slot="confirmed"]')!
    expect(confirmed.textContent).toBe(EN['callStats.slot.confirmed'])
    expect(confirmed.getAttribute('title')).toBe('DXCC 20m CW: confirmed')
    // The sources: the country holds LoTW and an AG eQSL, this call a card.
    const held = (rowHead: string) =>
      [...document.querySelectorAll('.cstats-src tbody tr')]
        .find((tr) => tr.querySelector('th')?.textContent === rowHead)!
        .querySelectorAll('td')
    const country = [...held(EN['callStats.src.country'])].map((td) => [td.getAttribute('data-held'), td.getAttribute('data-ag')])
    expect(country).toEqual([
      ['true', null],
      ['false', null],
      ['true', 'true'],
      ['false', null],
    ])
    expect([...held(EN['callStats.src.call'])].map((td) => td.getAttribute('data-held'))).toEqual(['false', 'true', 'false', 'false'])
  })

  it('marks an award the call earns no grid for, and says why', async () => {
    vi.mocked(getCallStats).mockResolvedValue({
      ...NAVASSA,
      call: 'W9XYZ',
      dxcc: { label: 'United States', code: 291, rows: [row(true), row(true), row(true)] },
      wasFamily: true,
      was: null,
    })
    render(<CallStatsPane call="W9XYZ" band="20m" mode="CW" />)
    await screen.findByText('United States · DXCC 291')
    expect(heads()[1]).toEqual(['WAS', '?', 'WAS: no state on file for W9XYZ.'])
    expect(screen.getByText('WAS: no state on file for W9XYZ.').className, 'and a screen reader hears it').toBe('sr-only')
    cleanup()
    vi.mocked(getCallStats).mockResolvedValue({ ...NAVASSA, call: 'Q1Q', dxcc: null, waz: null, countryQsl: null })
    render(<CallStatsPane call="Q1Q" band="20m" mode="CW" />)
    await waitFor(() => expect(document.querySelector('.cstats-who')?.textContent).toBe('Q1Q names no DXCC entity.'))
    expect(heads()).toEqual([
      ['DXCC', '—', 'Q1Q names no DXCC entity.'],
      ['WAS', '—', 'WAS: Q1Q is not a station in the United States.'],
      ['WAZ', '?', 'WAZ: no CQ zone for Q1Q.'],
    ])
    // No country, so nothing to say for it: its cells stay, empty; the call's own row says what it holds.
    const cells = (rowHead: string) =>
      [...[...document.querySelectorAll('.cstats-src tbody tr')].find((tr) => tr.querySelector('th')?.textContent === rowHead)!.querySelectorAll('td')].map(
        (td) => td.getAttribute('data-held'),
      )
    expect(cells(EN['callStats.src.country'])).toEqual(['none', 'none', 'none', 'none'])
    expect(cells(EN['callStats.src.call'])).toEqual(['false', 'true', 'false', 'false'])
  })

  it('heads each band column with its number, a centimetre band keeping its unit', () => {
    render(<CallStatsPane call="" band="70cm" mode="CW" />)
    const heads = [...document.querySelectorAll('.cstats-grid thead th')].map((th) => th.textContent)
    expect(heads).toEqual(['160', '80', '60', '40', '30', '20', '17', '15', '12', '10', '6', '4', '2', '70cm', '33cm', '23cm'])
    expect(document.querySelector('.cstats-grid thead th[data-here]')?.textContent, 'the band on the dial').toBe('70cm')
  })

  it('draws the same lines whatever it has to say, so nothing beside it moves', async () => {
    let answer: (s: CallStats) => void = () => {}
    vi.mocked(getCallStats)
      .mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)))
      .mockResolvedValueOnce({
        ...NAVASSA,
        call: 'W6XYZ',
        dxcc: { label: 'United States', code: 291, rows: [row(true), row(true), row(true)] },
        wasFamily: true,
        was: { label: 'CA', code: null, rows: [row(false), row(false), row(false)] },
      })
      .mockRejectedValueOnce(new Error('the logbook could not be read'))
    const { rerender } = render(<CallStatsPane call="" band="20m" mode="CW" />)
    expect(shape(), 'no call yet').toEqual(SHAPE)
    rerender(<CallStatsPane call="KP1AA" band="20m" mode="CW" />)
    expect(screen.getByText(EN['callStats.loading'])).toBeTruthy()
    expect(shape(), 'asking').toEqual(SHAPE)
    await act(async () => answer(NAVASSA))
    expect(shape(), 'a station outside the WAS family').toEqual(SHAPE)
    rerender(<CallStatsPane call="W6XYZ" band="20m" mode="CW" />)
    await screen.findByText('United States · DXCC 291')
    expect(shape(), 'a station with all three grids').toEqual(SHAPE)
    rerender(<CallStatsPane call="K1ABC" band="20m" mode="CW" />)
    await screen.findByText('Stats are not available: the logbook could not be read')
    expect(shape(), 'no answer').toEqual(SHAPE)
    rerender(<CallStatsPane call="" band="20m" mode="CW" />)
    expect(shape(), 'the call logged and cleared').toEqual(SHAPE)
    // The flags keep their room and say nothing while there is nothing to flag.
    expect(document.querySelector('.cstats-flags')!.getAttribute('aria-hidden')).toBe('true')
  })

  it('follows the call, and never shows the answer to an older one', async () => {
    let first: (s: CallStats) => void = () => {}
    vi.mocked(getCallStats)
      .mockImplementationOnce(() => new Promise((resolve) => (first = resolve)))
      .mockResolvedValueOnce({ ...NAVASSA, call: 'JA1XYZ', dxcc: { label: 'Japan', code: 339, rows: [row(true), row(true), row(true)] } })
    const { rerender } = render(<CallStatsPane call="KP1AA" band="20m" mode="CW" />)
    rerender(<CallStatsPane call="JA1XYZ" band="20m" mode="CW" />)
    await screen.findByText('Japan · DXCC 339')
    // KP1AA's answer lands late: it answers a question nobody is asking any more.
    await act(async () => first(NAVASSA))
    expect(screen.queryByText('Navassa Island · DXCC 182')).toBeNull()
    expect(screen.getByText('Japan · DXCC 339')).toBeTruthy()
    expect(vi.mocked(getCallStats).mock.calls).toEqual([
      ['KP1AA', '20m', 'CW'],
      ['JA1XYZ', '20m', 'CW'],
    ])
  })

  it('never draws one call’s answer under the next while that one’s is on its way', async () => {
    let second: (s: CallStats) => void = () => {}
    vi.mocked(getCallStats)
      .mockResolvedValueOnce(NAVASSA)
      .mockImplementationOnce(() => new Promise((resolve) => (second = resolve)))
    const { rerender } = render(<CallStatsPane call="KP1AA" band="20m" mode="CW" />)
    await screen.findByText('Navassa Island · DXCC 182')
    rerender(<CallStatsPane call="JA1XYZ" band="20m" mode="CW" />)
    expect(screen.queryByText('Navassa Island · DXCC 182'), 'KP1AA’s answer drawn for JA1XYZ').toBeNull()
    expect(screen.getByText(EN['callStats.loading'])).toBeTruthy()
    await act(async () => second({ ...NAVASSA, call: 'JA1XYZ', dxcc: { label: 'Japan', code: 339, rows: [row(true), row(true), row(true)] } }))
    expect(screen.getByText('Japan · DXCC 339')).toBeTruthy()
  })

  it('asks again when the log changes, and keeps the answer on screen until the new one lands', async () => {
    let second: (s: CallStats) => void = () => {}
    vi.mocked(getCallStats)
      .mockResolvedValueOnce(NAVASSA)
      .mockImplementationOnce(() => new Promise((resolve) => (second = resolve)))
    const { rerender } = render(<CallStatsPane call="KP1AA" band="20m" mode="CW" logTick={1} />)
    await screen.findByText('Navassa Island · DXCC 182')
    rerender(<CallStatsPane call="KP1AA" band="20m" mode="CW" logTick={2} />)
    expect(getCallStats).toHaveBeenCalledTimes(2)
    expect(screen.getByText('Navassa Island · DXCC 182'), 'the old answer stays while the new one is asked').toBeTruthy()
    await act(async () => second({ ...NAVASSA, newBand: false }))
    await waitFor(() => expect(document.querySelector('.cstats-flag[data-kind="band"]')?.getAttribute('data-on')).toBe('false'))
  })

  it('says the station could not answer', async () => {
    vi.mocked(getCallStats).mockRejectedValue(new Error('the logbook could not be read'))
    render(<CallStatsPane call="KP1AA" band="20m" mode="CW" />)
    await screen.findByText('Stats are not available: the logbook could not be read')
  })
})

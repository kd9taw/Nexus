// @vitest-environment jsdom
// THE AWARDS COUNTED BESIDE DXCC, AND THE STATION'S OWN AWARD PANELS: the WAP, WPX, WAE and WWFF
// cards read the summary; the whole-log DXCC chart shades each entity × band cell by its standing
// under a mode filter; What to QSL lists the contacts in the order the station ranked them, with
// their QSL route; and a custom award defined in the editor is kept as typed, imported from a
// file, and exported to one.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, waitFor, fireEvent, within } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import { chartCell, CustomAwardsPanel, DxccChartPanel, PotaActivatorPanel, WhatToQslPanel } from './AwardsMore'
import type { AwardSummary, CustomAward, CustomAwardsView, DxccChart, QslWant } from '../types'
import { EN } from '../i18n'
import {
  getAwards,
  getCustomAwards,
  getDxccChart,
  getWhatToQsl,
  getPotaActivator,
  readCustomAwardsFile,
  saveTextToDownloads,
  setCustomAwards,
  exportCustomAwards,
} from '../api'

vi.mock('../api', () => ({
  getAwards: vi.fn(),
  getConfirmationDiagnostics: vi.fn(() => Promise.resolve(null)),
  uploadLotwReportByIds: vi.fn(),
  qrzPushQso: vi.fn(),
  clublogPushQso: vi.fn(),
  eqslPushQso: vi.fn(),
  getDxccChart: vi.fn(),
  getWhatToQsl: vi.fn(),
  getPotaActivator: vi.fn(),
  getCustomAwards: vi.fn(),
  setCustomAwards: vi.fn(),
  readCustomAwardsFile: vi.fn(),
  exportCustomAwards: vi.fn(),
  saveTextToDownloads: vi.fn(),
}))

const BASE: AwardSummary = {
  qsos: 5,
  confirmedQsos: 3,
  dxccWorked: 3,
  dxccConfirmed: 2,
  dxccCredited: 0,
  readyToSubmit: 2,
  slotsWorked: 4,
  slotsConfirmed: 2,
  bands: [],
  modes: [],
  needed: [],
  slotNeeded: [],
  achievements: [],
  fiveBandWorked: 0,
  fiveBandConfirmed: 0,
  satDxccWorked: 0,
  satDxccConfirmed: 0,
  wazWorked: 4,
  wazConfirmed: 3,
  honorRoll: { currentTotal: 340, confirmed: 2, threshold: 331, achieved: false, needed: 329, numberOne: false, numberOneNeeded: 338 },
  was: { worked: 2, confirmed: 1, needed: [], fiveBandWorked: 0, fiveBandConfirmed: 0 },
  vucc: { worked: 0, confirmed: 0, bands: [], satWorked: 0, satConfirmed: 0, awards: [] },
  iota: { worked: 0, confirmed: 0, cardConfirmed: 0 },
  bandTargets: [],
}

const MORE: AwardSummary = {
  ...BASE,
  wap: { worked: 3, confirmed: 2, needed: ['AB', 'BC'] },
  wpx: { worked: 120, confirmed: 80, bands: [], unconfirmed: ['W8'] },
  wae: {
    worked: 3,
    confirmed: 2,
    total: 73,
    points: 6,
    pointsWorked: 7,
    class: null,
    trophy: false,
    unworked: [{ prefix: '1A', name: 'Sov Mil Order of Malta' }],
    unconfirmed: [{ prefix: 'DL', name: 'Fed. Rep. of Germany' }],
  },
  wwff: { worked: 12, entities: 3, programmes: 2, level: 10, nextLevel: 44 },
}

// Confirmed CW + worked digital on 20 m; worked phone on 40 m; nothing on 15 m.
const CHART: DxccChart = {
  bands: ['20m', '40m', '15m'],
  rows: [
    { entity: 'Japan', any: 1 | 2 | 16 | 4, cells: [1 | 2 | 16, 4, 0] },
    { entity: 'Spain', any: 1 | 2, cells: [1 | 2, 1 | 2, 1 | 2] },
  ],
}

const WANTS: QslWant[] = [
  {
    call: 'JA1ABC',
    whenUnix: 1767268800,
    band: '15m',
    mode: 'FT8',
    qslVia: 'JA1QSL',
    slots: [
      { award: 'dxccEntity', item: 'Japan' },
      { award: 'wpx', item: 'JA1' },
    ],
  },
  { call: 'K1ABC', whenUnix: 1767182400, band: '20m', mode: 'CW', qslVia: null, slots: [{ award: 'was', item: 'MA' }] },
]

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(getAwards).mockReset()
  vi.mocked(getDxccChart).mockResolvedValue(CHART)
  vi.mocked(getWhatToQsl).mockResolvedValue(WANTS)
  vi.mocked(getPotaActivator).mockResolvedValue({ calls: [], parks: [] })
  vi.mocked(getCustomAwards).mockResolvedValue({ awards: [], progress: [] })
  vi.mocked(setCustomAwards).mockReset()
  vi.mocked(readCustomAwardsFile).mockReset()
})
afterEach(cleanup)

/** A card's value line and note, by its award name. */
function card(name: string): { value: string; note: string } | null {
  const k = [...document.querySelectorAll('.aw-card .aw-k')].find((el) => el.textContent?.trim() === name)
  const c = k?.closest('.aw-card')
  if (!c) return null
  return {
    value: c.querySelector('.aw-v')!.textContent!.replace(/\s+/g, ' ').trim(),
    note: c.querySelector('.aw-note')!.textContent!,
  }
}

async function renderView(aw: AwardSummary) {
  vi.mocked(getAwards).mockResolvedValue(aw)
  render(<AwardsView showGamification={false} />)
  await waitFor(() => expect(screen.queryByText(EN['awards.loading.title'])).toBeNull())
  await screen.findAllByText(/DXCC/i)
}

describe('Awards: WAP, WPX, WAE, WWFF', () => {
  it('shows each card from the summary, and the WAP and WAE chase lists', async () => {
    await renderView(MORE)
    expect(card('WAP')).toEqual({
      value: '2 / 13',
      note: EN['awards.wap.note.toGo'].replace('{{remaining}}', '11').replace('{{worked}}', '3'),
    })
    expect(card('WPX')).toEqual({ value: '80', note: EN['awards.wpx.note'].replace('{{worked}}', '120') })
    expect(card('WAE')).toEqual({
      value: '2 / 73',
      note: EN['awards.wae.note.toGo'].replace('{{points}}', '6').replace('{{worked}}', '3'),
    })
    expect(card('WWFF')?.note).toBe(
      EN['awards.wwff.note.level'].replace('{{level}}', '10').replace('{{next}}', '44').replace('{{entities}}', '3'),
    )
    const wap = document.querySelector('.aw-wap')!
    expect([...wap.querySelectorAll('.aw-chip')].map((c) => c.textContent)).toEqual(['AB', 'BC'])
    const wae = document.querySelector('.aw-wae')!
    expect([...wae.querySelectorAll('[data-kind="unconfirmed"] .aw-chip')].map((c) => c.textContent)).toEqual(['DL'])
    expect(wae.querySelector('details')!.open).toBe(false)
  })

  it('shows none of them for a station older than them, and no station panels when observed', async () => {
    vi.mocked(getAwards).mockResolvedValue(BASE)
    render(<AwardsView showGamification={false} observation={BASE} />)
    await screen.findAllByText(/DXCC/i)
    for (const name of ['WAP', 'WPX', 'WAE', 'WWFF']) expect(card(name), name).toBeNull()
    expect(document.querySelector('.aw-chart-panel')).toBeNull()
    expect(document.querySelector('.aw-qsl-panel')).toBeNull()
    expect(getDxccChart).not.toHaveBeenCalled()
  })
})

describe('Awards: the whole-log DXCC chart', () => {
  it('reads each cell under a mode filter', () => {
    expect(chartCell(1 | 2 | 16, 'all')).toBe('confirmed')
    expect(chartCell(1 | 2 | 16, 'digital')).toBe('worked')
    expect(chartCell(1 | 2 | 16, 'phone')).toBe('needed')
    expect(chartCell(4, 'phone')).toBe('worked')
    expect(chartCell(0, 'all')).toBe('needed')
  })

  it('shades the cells, follows the mode filter, and filters to entities still needing a band', async () => {
    render(<DxccChartPanel />)
    await screen.findByText('Japan')
    const states = (entity: string) =>
      [...screen.getByText(entity).closest('tr')!.querySelectorAll('td')].map((c) => c.getAttribute('data-s'))
    expect(states('Japan')).toEqual(['confirmed', 'worked', 'needed'])
    fireEvent.click(screen.getByRole('button', { name: EN['awards.chart.mode.digital'] }))
    expect(states('Japan')).toEqual(['worked', 'needed', 'needed'])
    expect(states('Spain')).toEqual(['needed', 'needed', 'needed'])
    fireEvent.click(screen.getByRole('button', { name: EN['awards.chart.mode.cw'] }))
    fireEvent.click(screen.getByRole('checkbox'))
    expect(screen.queryByText('Spain'), 'Spain is confirmed on every band in CW').toBeNull()
    expect(screen.getByText('Japan')).toBeTruthy()
  })
})

describe('Awards: What to QSL', () => {
  it('lists the contacts in the order the station ranked them, with each route and slot', async () => {
    render(<WhatToQslPanel />)
    await screen.findByText(/JA1ABC/)
    const rows = [...document.querySelectorAll('.aw-qsl li')]
    expect(rows.map((r) => r.querySelector('.aw-entity')!.textContent!.split(' · ')[0])).toEqual(['JA1ABC', 'K1ABC'])
    expect(rows[0].textContent).toContain(EN['awards.qsl.via'].replace('{{route}}', 'JA1QSL'))
    expect(rows[1].textContent).toContain(EN['awards.qsl.noRoute'])
    expect([...rows[0].querySelectorAll('.aw-chip')].map((c) => c.textContent)).toEqual(['DXCC Japan', 'WPX JA1'])
  })

  it('says so when nothing is wanted', async () => {
    vi.mocked(getWhatToQsl).mockResolvedValue([])
    render(<WhatToQslPanel />)
    expect(await screen.findByText(EN['awards.qsl.empty'])).toBeTruthy()
  })
})

describe('Awards: POTA activator', () => {
  const stats = {
    callsign: 'KD9TAW',
    activations: 23,
    attempts: 2,
    qsos: 1240,
    parksActivated: 3,
    nextParkTier: 10,
    roverBest: 5,
    roverBestDate: '2026-05-02',
    roverDays: [2, 0, 0, 0, 0, 0],
    parks: [
      { reference: 'US-0001', name: 'Acadia', activations: 21, attempts: 1, qsos: 1100, lastDate: '2026-10-01' },
      { reference: 'US-0002', name: '', activations: 1, attempts: 1, qsos: 110, lastDate: '2026-05-02' },
      { reference: 'US-0003', name: '', activations: 1, attempts: 0, qsos: 30, lastDate: '2026-05-02' },
    ],
  }

  it('shows each achievement from the station’s counts, as POTA names them', async () => {
    vi.mocked(getPotaActivator).mockResolvedValue({ calls: [stats], parks: [] })
    render(<PotaActivatorPanel />)
    await screen.findByText(/Acadia/)
    const ladder = (name: string) =>
      [...document.querySelectorAll('.aw-pota-ladders li')].find(
        (li) => li.querySelector('.aw-entity')!.textContent === name,
      )!.querySelector('.aw-needbands')!.textContent
    expect(ladder(EN['awards.pota.parksLabel'])).toBe('3 · next achievement at 10')
    // 21 activations at one park: past the first tier (20), the next at 40.
    expect(ladder('Repeat Offender')).toBe('US-0001: 21 activations · next tier at 40')
    expect(ladder('Kilo')).toBe('US-0001: 1100 of 1000 contacts · reached at 1 park')
    expect(ladder('Rover')).toBe('most parks in one UTC day: 5 (2026-05-02)Warthog × 2')
    expect([...document.querySelectorAll('.aw-pota-parks tbody tr')].map((r) => r.firstElementChild!.textContent)).toEqual([
      'US-0001 · Acadia',
      'US-0002',
      'US-0003',
    ])
    expect(screen.queryByText(/Worked as/)).toBeNull()
  })

  it('keeps two calls apart, and says so when the log holds no activation', async () => {
    vi.mocked(getPotaActivator).mockResolvedValue({ calls: [stats, { ...stats, callsign: 'W1AW' }], parks: [] })
    render(<PotaActivatorPanel />)
    expect(await screen.findByText('Worked as KD9TAW')).toBeTruthy()
    expect(screen.getByText('Worked as W1AW')).toBeTruthy()
    cleanup()
    vi.mocked(getPotaActivator).mockResolvedValue({ calls: [], parks: [] })
    render(<PotaActivatorPanel />)
    expect(await screen.findByText(EN['awards.pota.empty'])).toBeTruthy()
  })

  it('is a station read: an observed summary shows none of it', async () => {
    vi.mocked(getAwards).mockResolvedValue(BASE)
    render(<AwardsView showGamification={false} observation={BASE} />)
    await screen.findAllByText(/DXCC/i)
    expect(document.querySelector('.aw-pota-panel')).toBeNull()
    expect(getPotaActivator).not.toHaveBeenCalled()
  })
})

describe('Awards: custom awards', () => {
  const kept: CustomAward = {
    id: 'custom-1',
    name: 'Club',
    kind: 'call',
    field: '',
    targets: ['W1AW', 'K1ABC'],
    bands: ['20m'],
    modes: ['CW'],
    confirmation: 'lotwCard',
  }
  const view: CustomAwardsView = {
    awards: [kept],
    progress: [
      { id: 'custom-1', name: 'Club', kind: 'call', total: 2, worked: 1, confirmed: 0, unworked: ['K1ABC'], unconfirmed: ['W1AW'] },
    ],
  }

  it('keeps a new award as the operator typed it', async () => {
    vi.mocked(setCustomAwards).mockImplementation(async (a) => a)
    render(<CustomAwardsPanel />)
    fireEvent.click(await screen.findByRole('button', { name: EN['awards.custom.add'] }))
    const form = document.querySelector('.aw-custom-form') as HTMLElement
    fireEvent.change(within(form).getByLabelText(EN['awards.custom.name']), { target: { value: 'Club' } })
    fireEvent.change(within(form).getByLabelText(EN['awards.custom.targets']), { target: { value: 'W1AW, K1ABC\nN0CALL' } })
    fireEvent.change(within(form).getByLabelText(EN['awards.custom.bands']), { target: { value: '20m 40m' } })
    fireEvent.click(within(form).getByRole('button', { name: EN['awards.custom.save'] }))
    await waitFor(() => expect(setCustomAwards).toHaveBeenCalled())
    expect(vi.mocked(setCustomAwards).mock.calls[0][0]).toEqual([
      { ...kept, id: '', targets: ['W1AW', 'K1ABC', 'N0CALL'], bands: ['20m', '40m'], modes: [] },
    ])
  })

  it("shows each award's standing, edits one in place and removes one", async () => {
    vi.mocked(getCustomAwards).mockResolvedValue(view)
    vi.mocked(setCustomAwards).mockImplementation(async (a) => a)
    render(<CustomAwardsPanel />)
    expect(
      await screen.findByText(
        `Club · ${EN['awards.custom.standing'].replace('{{confirmed}}', '0').replace('{{worked}}', '1').replace('{{total}}', '2')}`,
      ),
    ).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: EN['awards.custom.edit'] }))
    const form = document.querySelector('.aw-custom-form') as HTMLElement
    fireEvent.change(within(form).getByLabelText(EN['awards.custom.name']), { target: { value: 'Club members' } })
    fireEvent.click(within(form).getByRole('button', { name: EN['awards.custom.save'] }))
    await waitFor(() => expect(setCustomAwards).toHaveBeenCalledTimes(1))
    expect(vi.mocked(setCustomAwards).mock.calls[0][0]).toEqual([{ ...kept, name: 'Club members' }])
    fireEvent.click(await screen.findByRole('button', { name: EN['awards.custom.remove'] }))
    await waitFor(() => expect(setCustomAwards).toHaveBeenCalledTimes(2))
    expect(vi.mocked(setCustomAwards).mock.calls[1][0]).toEqual([])
  })

  it('imports a file beside the awards already kept, and exports them', async () => {
    vi.mocked(getCustomAwards).mockResolvedValue(view)
    vi.mocked(readCustomAwardsFile).mockResolvedValue([{ ...kept, id: 'custom-1', name: 'Theirs' }])
    vi.mocked(setCustomAwards).mockImplementation(async (a) => a)
    vi.mocked(exportCustomAwards).mockResolvedValue('{"format":"nexus-custom-awards"}')
    vi.mocked(saveTextToDownloads).mockResolvedValue('/dl/x.json')
    render(<CustomAwardsPanel />)
    await screen.findByText(/Club ·/)
    const input = document.querySelector('.aw-custom-panel input[type="file"]') as HTMLInputElement
    const file = new File(['{"format":"nexus-custom-awards"}'], 'theirs.json')
    fireEvent.change(input, { target: { files: [file] } })
    await waitFor(() => expect(setCustomAwards).toHaveBeenCalled())
    expect(readCustomAwardsFile).toHaveBeenCalledWith('{"format":"nexus-custom-awards"}')
    expect(vi.mocked(setCustomAwards).mock.calls[0][0]).toEqual([kept, { ...kept, id: '', name: 'Theirs' }])
    fireEvent.click(screen.getByRole('button', { name: EN['awards.custom.export'] }))
    await waitFor(() => expect(saveTextToDownloads).toHaveBeenCalled())
    expect(vi.mocked(saveTextToDownloads).mock.calls[0][1]).toBe('{"format":"nexus-custom-awards"}')
  })
})

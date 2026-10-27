// @vitest-environment jsdom
// NEEDED BY BAND: the Awards view lists, for DXCC, WAS and WAZ, each band's count never worked
// there and worked there but not confirmed — the station's own count (`AwardSummary.bandNeeds`),
// each count opening its list. A station older than the Stats pane sends none, and the view shows
// no panel.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import type { AwardSummary } from '../types'
import { EN } from '../i18n'
import { getAwards } from '../api'

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

const NEEDS: AwardSummary = {
  ...BASE,
  bandNeeds: {
    dxcc: [
      { band: '20m', unworked: ['Albania', 'Andorra', 'Italy'], unconfirmed: ['Japan'] },
      { band: '40m', unworked: ['Albania'], unconfirmed: [] },
    ],
    was: [{ band: '20m', unworked: ['AK', 'AL'], unconfirmed: ['WA'] }],
    waz: [{ band: '20m', unworked: ['1', '2', '3'], unconfirmed: ['25'] }],
  },
}

vi.mock('../api', () => ({
  // The Awards view's own panels (AwardsMore) read these; left reading, they change nothing here.
  getDxccChart: vi.fn(() => new Promise(() => {})),
  getWhatToQsl: vi.fn(() => new Promise(() => {})),
  getPotaActivator: vi.fn(() => new Promise(() => {})),
  getCustomAwards: vi.fn(() => new Promise(() => {})),
  getAwards: vi.fn(),
  getConfirmationDiagnostics: vi.fn(() => Promise.resolve(null)),
  uploadLotwReportByIds: vi.fn(),
  qrzPushQso: vi.fn(),
  clublogPushQso: vi.fn(),
  eqslPushQso: vi.fn(),
}))

// A block, not an arrow's value: a function a beforeEach returns is run as its cleanup, and that would call
// the mock once more after every test.
beforeEach(() => {
  vi.mocked(getAwards).mockReset()
})
afterEach(cleanup)

async function renderWith(aw: AwardSummary) {
  vi.mocked(getAwards).mockResolvedValue(aw)
  render(<AwardsView showGamification={false} />)
  await waitFor(() => expect(screen.queryByText(EN['awards.loading.title'])).toBeNull())
  await screen.findAllByText(/DXCC/i)
}

/** The panel's rows: each band and its two counts, by value. */
function rows(): [string, string, string][] {
  return [...document.querySelectorAll('.aw-bandneeds tbody tr')]
    .filter((tr) => tr.querySelector('th'))
    .map((tr) => {
      const [unworked, unconfirmed] = [...tr.querySelectorAll('button.aw-bn-count')]
      return [tr.querySelector('th')!.textContent!, unworked.textContent!, unconfirmed.textContent!]
    })
}

describe('Needed by band', () => {
  it('counts each band never worked and worked not confirmed, for the award picked', async () => {
    await renderWith(NEEDS)
    expect(screen.getByText(EN['awards.bandNeeds.head'])).toBeTruthy()
    expect(rows()).toEqual([
      ['20m', '3', '1'],
      ['40m', '1', '0'],
    ])
    fireEvent.click(screen.getByRole('button', { name: 'WAS' }))
    expect(rows()).toEqual([['20m', '2', '1']])
    expect(screen.getByText(EN['awards.bandNeeds.note.was'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'WAZ' }))
    expect(rows()).toEqual([['20m', '3', '1']])
    expect(screen.getByRole('button', { name: 'WAZ' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('opens a band’s list from its count, and a zero opens nothing', async () => {
    await renderWith(NEEDS)
    const [unworked20, unconfirmed20] = [...document.querySelectorAll('.aw-bandneeds button.aw-bn-count')]
    fireEvent.click(unworked20)
    expect(unworked20.getAttribute('aria-expanded')).toBe('true')
    expect(document.querySelector('.aw-bn-list')!.textContent).toBe('Never worked on 20m (3) Albania · Andorra · Italy')
    fireEvent.click(unconfirmed20)
    expect(document.querySelector('.aw-bn-list')!.textContent).toBe('Worked on 20m, not confirmed (1) Japan')
    fireEvent.click(unconfirmed20)
    expect(document.querySelector('.aw-bn-list')).toBeNull()
    const zero = [...document.querySelectorAll('.aw-bandneeds button.aw-bn-count')][3] as HTMLButtonElement
    expect([zero.textContent, zero.disabled]).toEqual(['0', true])
  })

  it('draws no panel for a station that sends none', async () => {
    await renderWith(BASE)
    expect(screen.queryByText(EN['awards.bandNeeds.head'])).toBeNull()
    // Control: the view itself rendered.
    expect(screen.getByText(EN['awards.bands.head'])).toBeTruthy()
  })
})

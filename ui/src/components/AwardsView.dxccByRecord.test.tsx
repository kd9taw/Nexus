// @vitest-environment jsdom
// DXCC counts each contact by the DXCC number its record carries — LoTW's where it differs from
// the log's — so the Awards view shows the current count beside the total with the deleted
// entities, and lists every contact LoTW confirmed under another entity than the log has. A
// station older than that sends neither, and the view shows neither.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import type { AwardSummary } from '../types'
import { EN } from '../i18n'
import { getAwards } from '../api'

const BASE: AwardSummary = {
  qsos: 52521,
  confirmedQsos: 40000,
  dxccWorked: 340,
  dxccConfirmed: 340,
  dxccCredited: 330,
  readyToSubmit: 10,
  slotsWorked: 2000,
  slotsConfirmed: 1900,
  bands: [],
  modes: [],
  needed: [],
  slotNeeded: [],
  achievements: [],
  fiveBandWorked: 300,
  fiveBandConfirmed: 290,
  satDxccWorked: 0,
  satDxccConfirmed: 0,
  wazWorked: 40,
  wazConfirmed: 40,
  honorRoll: {
    currentTotal: 340,
    confirmed: 340,
    threshold: 331,
    achieved: true,
    needed: 0,
    numberOne: true,
    numberOneNeeded: 0,
  },
  was: { worked: 50, confirmed: 50, needed: [], fiveBandWorked: 50, fiveBandConfirmed: 50 },
  vucc: { worked: 0, confirmed: 0, bands: [], satWorked: 0, satConfirmed: 0, awards: [] },
  iota: { worked: 0, confirmed: 0, cardConfirmed: 0 },
  bandTargets: [],
}

/** The first report's log, counted by each record's own number: 340 current, 349 total. */
const BY_RECORD: AwardSummary = {
  ...BASE,
  dxccWorkedTotal: 349,
  dxccConfirmedTotal: 349,
  dxccDisagreements: [
    {
      call: 'K1N',
      whenUnix: 1_423_571_640,
      band: '20m',
      mode: 'CW',
      loggedDxcc: 291,
      loggedEntity: 'United States',
      lotwDxcc: 182,
      lotwEntity: 'Navassa Island',
    },
    {
      call: 'K9XYZ',
      whenUnix: 1_700_000_000,
      band: '40m',
      mode: 'FT8',
      loggedDxcc: 291,
      loggedEntity: 'United States',
      lotwDxcc: 999,
      lotwEntity: null,
    },
  ],
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

beforeEach(() => vi.mocked(getAwards).mockReset())
afterEach(cleanup)

async function renderWith(aw: AwardSummary) {
  vi.mocked(getAwards).mockResolvedValue(aw)
  render(<AwardsView showGamification={false} />)
  await waitFor(() => expect(screen.queryByText(EN['awards.loading.title'])).toBeNull())
  await screen.findAllByText(/DXCC/i)
}

describe('the DXCC count by each record’s own number', () => {
  it('shows the current count beside the total with the deleted entities', async () => {
    await renderWith(BY_RECORD)
    expect(screen.getByText('340 current · 349 total, deleted entities included')).toBeTruthy()
  })

  it('lists every contact LoTW counts as another entity, with both entities named', async () => {
    await renderWith(BY_RECORD)
    expect(screen.getByText('LoTW counts as another entity (2)')).toBeTruthy()
    expect(screen.getByText('K1N · 2015-02-10 · 20m CW')).toBeTruthy()
    expect(screen.getByText('Your log: United States (291) · LoTW: Navassa Island (182)')).toBeTruthy()
    // A number the station's table does not know is shown as the number.
    expect(screen.getByText('Your log: United States (291) · LoTW: DXCC 999')).toBeTruthy()
  })

  it('shows neither for a station that sends neither, and no list when nothing disagrees', async () => {
    await renderWith(BASE)
    expect(screen.queryByText(/deleted entities included/)).toBeNull()
    expect(screen.queryByText(/LoTW counts as another entity/)).toBeNull()
    // Control: the DXCC card itself rendered.
    expect(screen.getByText(/DXCC achieved ✓/)).toBeTruthy()

    cleanup()
    await renderWith({ ...BY_RECORD, dxccDisagreements: [] })
    expect(screen.getByText('340 current · 349 total, deleted entities included')).toBeTruthy()
    expect(screen.queryByText(/LoTW counts as another entity/)).toBeNull()
  })
})

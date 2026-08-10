// @vitest-environment jsdom
// The award-tile semantics fixed by the N9UM audit (2026-08-09): a tester put his
// 31k-QSO log next to his official LoTW account and every discrepancy traced to a
// tile claiming an ARRL semantic the number didn't have. These render the tiles
// with N9UM-shaped data and pin the corrected claims.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import type { AwardSummary } from '../types'

const N9UM: Partial<AwardSummary> = {
  qsos: 31648,
  confirmedQsos: 15881,
  dxccWorked: 332,
  dxccConfirmed: 330,
  dxccCredited: 197,
  readyToSubmit: 133,
  slotsWorked: 2702,
  slotsConfirmed: 2552,
  bands: [],
  modes: [],
  needed: [],
  slotNeeded: [],
  achievements: [],
  fiveBandWorked: 244,
  fiveBandConfirmed: 225,
  satDxccWorked: 16,
  satDxccConfirmed: 16,
  wazWorked: 40,
  wazConfirmed: 40,
  honorRoll: {
    currentTotal: 340,
    confirmed: 330,
    threshold: 331,
    achieved: false,
    needed: 1,
    numberOne: false,
    numberOneNeeded: 10,
  },
  was: { worked: 50, confirmed: 50, needed: [], fiveBandWorked: 50, fiveBandConfirmed: 50 },
  vucc: {
    worked: 1602,
    confirmed: 1489,
    bands: [],
    satWorked: 63,
    satConfirmed: 3,
    awards: [
      { band: '6m', worked: 139, confirmed: 132, threshold: 100, achieved: true },
      { band: '2m', worked: 13, confirmed: 7, threshold: 100, achieved: false },
    ],
  },
  iota: { worked: 302, confirmed: 286, cardConfirmed: 41 },
  bandTargets: [],
}

vi.mock('../api', () => ({
  getAwards: vi.fn(() => Promise.resolve(N9UM)),
  getConfirmationDiagnostics: vi.fn(() => Promise.resolve(null)),
  getLog: vi.fn(() => Promise.resolve([])),
  uploadLotw: vi.fn(),
  uploadQrz: vi.fn(),
  uploadClublog: vi.fn(),
  uploadEqsl: vi.fn(),
  pushQsoQrz: vi.fn(),
  pushQsoClublog: vi.fn(),
  pushQsoEqsl: vi.fn(),
}))

afterEach(cleanup)

async function renderAwards() {
  render(<AwardsView showGamification={false} />)
  await screen.findAllByText(/DXCC/i)
}

describe('award-tile semantics (N9UM audit)', () => {
  it('VUCC headline is the best real per-band standing, never the all-band total', async () => {
    await renderAwards()
    // 6m is achieved at its own threshold; the 1489 all-band count is labeled a tracker.
    expect(screen.getByText(/VUCC ✓ 6m/)).toBeTruthy()
    expect(screen.getByText(/1489 grids confirmed on all bands \(tracker\)/)).toBeTruthy()
    // The old claim — the all-band total presented AS VUCC — must be gone.
    expect(screen.queryByText(/1489 grids to go|worked \(terrestrial, all bands\)/)).toBeNull()
  })

  it('Honor Roll names the shortfall and the entry threshold, unambiguously', async () => {
    await renderAwards()
    expect(screen.getByText(/1 more confirmed needed — Honor Roll entry at 331/)).toBeTruthy()
  })

  it('the IOTA checkmark gates on card confirmations, and says why', async () => {
    await renderAwards()
    // 286 log-confirmed but only 41 on cards → no ✓, and the card count is on screen.
    expect(screen.queryByText(/^IOTA ✓$/)).toBeNull()
    expect(screen.getByText(/41 on cards — IOTA credits cards \/ Club Log, not\s+LoTW/)).toBeTruthy()
  })

  it('5-Band DXCC presents the weakest-band standing with its meaning', async () => {
    await renderAwards()
    expect(screen.getByText(/weakest of the 5 classic bands/)).toBeTruthy()
  })

  it('the Sat VUCC card states auto-tagging and the Satellite DXCC count', async () => {
    await renderAwards()
    expect(screen.getByText(/Sat DXCC 16 confirmed/)).toBeTruthy()
    // The tagging is LIVE (2026-08-10): the card says pass contacts tag automatically,
    // with the honest ISS residual — not the old edit-your-ADIF-by-hand caveat.
    expect(screen.getByText(/tagged automatically when logged on the bird/)).toBeTruthy()
    expect(screen.queryByText(/tag the ADIF by hand/)).toBeNull()
  })
})

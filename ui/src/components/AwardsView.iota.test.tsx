// @vitest-environment jsdom
// IOTA: the programme has credited LoTW-matched contacts since 2020, beside QSL cards, so the card's
// tick follows the island groups confirmed by LoTW OR card (`AwardSummary.iota.confirmed`; eQSL is
// never in it). The card-only count stays on the note.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
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

// A block, not an arrow's value: a function a beforeEach returns is run as its cleanup.
beforeEach(() => {
  vi.mocked(getAwards).mockReset()
})
afterEach(cleanup)

async function renderWith(iota: AwardSummary['iota']) {
  vi.mocked(getAwards).mockResolvedValue({ ...BASE, iota })
  render(<AwardsView showGamification={false} />)
  await waitFor(() => expect(screen.queryByText(EN['awards.loading.title'])).toBeNull())
  await screen.findAllByText(/DXCC/i)
}

/** The IOTA card: whether it carries the award tick, its value line and its note. */
function card(): { achieved: boolean; value: string; note: string } {
  const k = [...document.querySelectorAll('.aw-card .aw-k')].find((el) => el.textContent?.trim() === 'IOTA')
  const c = k!.closest('.aw-card')!
  return {
    achieved: c.classList.contains('aw-card-elite'),
    value: c.querySelector('.aw-v')!.textContent!.replace(/\s+/g, ' ').trim(),
    note: c.querySelector('.aw-note')!.textContent!,
  }
}

describe('Awards: IOTA', () => {
  it('ticks the award on 100 groups confirmed by LoTW, with no card among them', async () => {
    await renderWith({ worked: 120, confirmed: 100, cardConfirmed: 0 })
    expect(card()).toEqual({
      achieved: true,
      value: '100 / 100',
      note: EN['awards.iota.note.achieved'].replace('{{cards}}', '0'),
    })
  })

  it('still ticks it on 100 card-confirmed groups', async () => {
    await renderWith({ worked: 100, confirmed: 100, cardConfirmed: 100 })
    expect(card().achieved).toBe(true)
  })

  it('does not tick it one group short, and keeps the card count on the note', async () => {
    await renderWith({ worked: 150, confirmed: 99, cardConfirmed: 40 })
    expect(card()).toEqual({
      achieved: false,
      value: '99 / 100',
      note: EN['awards.iota.note.worked'].replace('{{worked}}', '150').replace('{{cards}}', '40'),
    })
  })
})

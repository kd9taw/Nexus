// @vitest-environment jsdom
// THE FRED FISH MEMORIAL AWARD: the Awards view shows the station's count of the 488 squares of the
// 48 contiguous states confirmed on 6 m (`AwardSummary.ffma`), and lists the squares still needed —
// worked but not confirmed first, then the never-worked ones folded away. A station older than FFMA
// sends none, and the view shows neither.
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

const FFMA: AwardSummary = {
  ...BASE,
  ffma: { worked: 3, confirmed: 2, total: 488, unworked: ['CM79', 'CM86'], unconfirmed: ['EN52'] },
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

async function renderWith(aw: AwardSummary) {
  vi.mocked(getAwards).mockResolvedValue(aw)
  render(<AwardsView showGamification={false} />)
  await waitFor(() => expect(screen.queryByText(EN['awards.loading.title'])).toBeNull())
  await screen.findAllByText(/DXCC/i)
}

/** The FFMA card: its value line and its note, by text. */
function card(): { value: string; note: string } | null {
  const k = [...document.querySelectorAll('.aw-card .aw-k')].find((el) => el.textContent?.trim() === 'FFMA')
  const c = k?.closest('.aw-card')
  if (!c) return null
  return {
    value: c.querySelector('.aw-v')!.textContent!.replace(/\s+/g, ' ').trim(),
    note: c.querySelector('.aw-note')!.textContent!,
  }
}

describe('Awards: FFMA', () => {
  it('counts the squares confirmed of 488 and says how many are left', async () => {
    await renderWith(FFMA)
    expect(card()).toEqual({
      value: '2 / 488',
      note: EN['awards.ffma.note.toGo'].replace('{{remaining}}', '486').replace('{{worked}}', '3'),
    })
  })

  it('lists the worked-but-unconfirmed squares, then the never-worked ones folded away', async () => {
    await renderWith(FFMA)
    const panel = document.querySelector('.aw-ffma')
    expect(panel, 'the FFMA chase panel').toBeTruthy()
    expect(panel!.querySelector('h3')!.textContent).toContain(EN['awards.chase.ffma.head'].replace('{{count}}', '3'))
    const unconfirmed = panel!.querySelector('[data-kind="unconfirmed"]')!
    expect([...unconfirmed.querySelectorAll('.aw-chip')].map((c) => c.textContent)).toEqual(['EN52'])
    const unworked = panel!.querySelector('details')!
    expect(unworked.open, 'folded by default').toBe(false)
    expect(unworked.querySelector('summary')!.textContent).toBe(EN['awards.chase.ffma.unworked'].replace('{{count}}', '2'))
    expect([...unworked.querySelectorAll('.aw-chip')].map((c) => c.textContent)).toEqual(['CM79', 'CM86'])
  })

  it('says the award is done when all 488 are confirmed', async () => {
    await renderWith({ ...BASE, ffma: { worked: 488, confirmed: 488, total: 488, unworked: [], unconfirmed: [] } })
    expect(card()?.note).toBe(EN['awards.ffma.note.achieved'].replace('{{total}}', '488'))
    expect(document.querySelector('.aw-ffma')!.textContent).toContain(EN['awards.chase.ffma.empty'].replace('{{total}}', '488'))
  })

  it('shows neither card nor list for a station that sends no FFMA', async () => {
    await renderWith(BASE)
    expect(card()).toBeNull()
    expect(document.querySelector('.aw-ffma')).toBeNull()
  })
})

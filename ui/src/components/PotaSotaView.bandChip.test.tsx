// @vitest-environment jsdom
//
// A REMEMBERED BAND ALWAYS HAS ITS CHIP. The board's band filter outlives the run, and its chips
// were built only from the bands the current spots are on. A hunter who last picked 40 m and comes
// back to a board with nobody on 40 m got an empty list and no lit chip saying why, and with no
// spots at all not even the band row, so the filter could not be seen or cleared.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  setHuntTarget: vi.fn(async () => null),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setActivation: vi.fn(),
  setActivationState: vi.fn(), clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(),
  importHuntedParksCsv: vi.fn(), selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'

const spot20m: OtaSpot = {
  program: 'POTA', reference: 'US-0001', name: 'Sample park', activator: 'K1ABC', freqKhz: 14_244, mode: 'SSB',
  spotter: null, comment: null, grid: null, newPark: true, bandOpen: false, huntedToday: false,
}
const snap = { hunt: null, radio: { dialMhz: 14.244 }, logTick: 1 } as unknown as AppSnapshot
const bandRow = () => screen.queryByRole('group', { name: t('ota.filter.band.aria') })
const chip = (name: string) => screen.queryByRole('button', { name })

beforeEach(() => {
  localStorage.setItem('nexus.ota.program', 'POTA')
  localStorage.setItem('nexus.ota.bandFilter', JSON.stringify(['40m']))
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

describe('the remembered band on the hunter board', () => {
  it('has its own lit chip while nobody is on it, and one click on it brings the other bands back', async () => {
    api.getOtaSpots.mockImplementation(async (program: string) => (program === 'POTA' ? [spot20m] : []))
    render(<PotaSotaView snap={snap} onHunt={vi.fn()} onSnap={vi.fn()} />)
    await waitFor(() => expect(chip('20m')).not.toBeNull())
    const forty = chip('40m')
    expect(forty, 'the remembered band has no chip').not.toBeNull()
    expect(forty!.className).toContain('active')
    expect(screen.queryByText('K1ABC'), 'the 20 m spot shows through a 40 m filter').toBeNull()
    fireEvent.click(forty!)
    expect(screen.queryByText('K1ABC')).not.toBeNull()
  })

  it('keeps the band row, with All, when there are no spots at all', async () => {
    api.getOtaSpots.mockImplementation(async () => [])
    render(<PotaSotaView snap={snap} onHunt={vi.fn()} onSnap={vi.fn()} />)
    await waitFor(() => expect(api.getOtaSpots).toHaveBeenCalled())
    expect(bandRow(), 'no band row, so the filter cannot be seen').not.toBeNull()
    expect(chip('40m')?.className).toContain('active')
    fireEvent.click(chip(t('ota.filter.all'))!)
    expect(chip('40m'), 'a band cleared with no spots on it keeps a chip').toBeNull()
  })

  // CONTROL: with nothing remembered the row is built from the spots alone, as before.
  it('offers only the bands on the air when no band is remembered', async () => {
    localStorage.removeItem('nexus.ota.bandFilter')
    api.getOtaSpots.mockImplementation(async (program: string) => (program === 'POTA' ? [spot20m] : []))
    render(<PotaSotaView snap={snap} onHunt={vi.fn()} onSnap={vi.fn()} />)
    await waitFor(() => expect(chip('20m')).not.toBeNull())
    expect(chip('40m')).toBeNull()
  })
})

// @vitest-environment jsdom
//
// WWFF (World Wide Flora & Fauna) JOINS THE HUNTER BOARD (#363). Its own tab reads WWFF Spotline's
// list (the station's `get_ota_spots("WWFF")`), "All" reads every programme, and HUNT on a WWFF row
// sets the hunt target under WWFF with the area's reference, the way a POTA row does, so the next
// contact with the activator is filed as WWFF and the log strip fills the reference.
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

const spot = (program: string, activator: string, reference: string): OtaSpot => ({
  program, reference, name: 'Sample area', activator, freqKhz: 14_244, mode: 'SSB',
  spotter: null, comment: null, grid: null, newPark: true, bandOpen: false, huntedToday: false,
})
const snap = { hunt: null, radio: { dialMhz: 14.244 }, logTick: 1 } as unknown as AppSnapshot
const fetched = (program: string) => api.getOtaSpots.mock.calls.filter((c) => c.length === 1 && c[0] === program).length
const tab = (name: string) => screen.getByRole('tab', { name })

beforeEach(() => {
  api.getOtaSpots.mockReset()
  api.getOtaSpots.mockImplementation(async (program: string) =>
    program === 'WWFF' ? [spot('WWFF', 'DL0XYZ/P', 'DLFF-0001')] : program === 'POTA' ? [spot('POTA', 'K1ABC', 'US-0001')] : [],
  )
  api.setHuntTarget.mockClear()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

describe('WWFF on the POTA/SOTA board', () => {
  it('has its own tab, which reads WWFF Spotline and shows only WWFF rows', async () => {
    render(<PotaSotaView snap={snap} onHunt={vi.fn()} onSnap={vi.fn()} />)
    fireEvent.click(tab('WWFF'))
    await waitFor(() => expect(screen.queryByText('DL0XYZ/P')).not.toBeNull())
    expect(fetched('WWFF')).toBeGreaterThan(0)
    expect(screen.queryByText('K1ABC'), 'a POTA row on the WWFF tab').toBeNull()
  })

  it('"All" reads POTA, SOTA and WWFF together', async () => {
    localStorage.setItem('nexus.ota.program', 'Both')
    render(<PotaSotaView snap={snap} onHunt={vi.fn()} onSnap={vi.fn()} />)
    expect(tab(t('ota.program.all')).getAttribute('aria-selected')).toBe('true')
    await waitFor(() => expect(screen.queryByText('DL0XYZ/P') && screen.queryByText('K1ABC')).not.toBeNull())
    expect([fetched('POTA'), fetched('SOTA'), fetched('WWFF')].every((n) => n > 0)).toBe(true)
  })

  it('HUNT on a WWFF row hunts the area under WWFF, as POTA hunts a park', async () => {
    localStorage.setItem('nexus.ota.program', 'WWFF')
    const onHunt = vi.fn()
    render(<PotaSotaView snap={snap} onHunt={onHunt} onSnap={vi.fn()} />)
    await waitFor(() => expect(screen.queryByText('DL0XYZ/P')).not.toBeNull())
    fireEvent.click(document.querySelector('.pota-hunt-btn') as HTMLElement)
    await waitFor(() => expect(onHunt).toHaveBeenCalled())
    expect(api.setHuntTarget).toHaveBeenCalledWith('DL0XYZ/P', 'WWFF', 'DLFF-0001')
    expect(onHunt.mock.calls[0][0]).toMatchObject({ call: 'DL0XYZ/P', program: 'WWFF', reference: 'DLFF-0001' })
  })

  // CONTROL: an observed station's board shows that station's feeds, which carry POTA and SOTA
  // only, so it offers no WWFF tab and its every-programme tab still reads "Both".
  it('an observed station board has no WWFF tab', () => {
    const observation = {
      feeds: [{ program: 'POTA' as const, status: 'ready' as const, sourceAgeMs: 0, spots: [] }],
      activation: { program: null, reference: null, qsoCount: 0 },
      hunt: null,
      parkCount: 0,
      huntedCount: 0,
    }
    render(<PotaSotaView snap={snap} observation={observation as never} />)
    expect(screen.queryByRole('tab', { name: 'WWFF' })).toBeNull()
    expect(tab(t('ota.program.both'))).toBeTruthy()
  })
})

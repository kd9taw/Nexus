// @vitest-environment jsdom
//
// THE POTA/SOTA VIEW STILL HUNTS, AND STILL CLEARS ITS HUNT. A park clicked on the Needed board no
// longer sets a hunt (App.neededPark.test.tsx), and the operator's rule is that "Hunted should only
// be used if you chase a park from the POTA tab". So this view is now where a hunt comes from, by
// value: HUNT sets the row's own call and park, and the banner's ✕ ends it.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  setHuntTarget: vi.fn(async (): Promise<unknown> => null),
  clearHuntTarget: vi.fn(async (): Promise<unknown> => null),
}))
vi.mock('../api', () => ({
  ...api,
  openPanelWindow: vi.fn(), setActivation: vi.fn(), clearActivation: vi.fn(), downloadParks: vi.fn(),
  importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(), selfSpot: vi.fn(),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import { PotaSotaView } from './PotaSotaView'
import { t } from '../i18n'

const K9ABC: OtaSpot = {
  program: 'POTA', reference: 'US-1000', name: 'Test park', activator: 'K9ABC', freqKhz: 14_285, mode: 'SSB',
  spotter: null, comment: null, grid: null, newPark: true, bandOpen: false, huntedToday: false,
}
const snap = (hunt: { program: string; reference: string; call: string } | null): AppSnapshot =>
  ({ hunt, radio: { dialMhz: 14.285 }, logTick: 1 }) as unknown as AppSnapshot

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the POTA/SOTA view’s own hunt', () => {
  it('HUNT sets the row’s call and park, and hands the snapshot on', async () => {
    api.getOtaSpots.mockResolvedValue([K9ABC])
    const hunted = snap({ program: 'POTA', reference: 'US-1000', call: 'K9ABC' })
    api.setHuntTarget.mockResolvedValue(hunted)
    const onSnap = vi.fn()
    const onHunt = vi.fn()
    render(<PotaSotaView snap={snap(null)} onSnap={onSnap} onHunt={onHunt} />)
    fireEvent.click(await screen.findByRole('button', { name: t('ota.hunt.button.aria', { call: 'K9ABC' }) }))
    await waitFor(() => expect(onHunt).toHaveBeenCalled())
    expect(api.setHuntTarget.mock.calls).toEqual([['K9ABC', 'POTA', 'US-1000']])
    expect(onSnap).toHaveBeenCalledWith(hunted)
    expect(onHunt.mock.calls[0][0]).toMatchObject({ call: 'K9ABC', program: 'POTA', reference: 'US-1000' })
  })

  it('the banner’s ✕ ends the hunt, and the banner goes with it', async () => {
    const cleared = snap(null)
    api.clearHuntTarget.mockResolvedValue(cleared)
    const onSnap = vi.fn()
    const view = render(
      <PotaSotaView snap={snap({ program: 'POTA', reference: 'US-1000', call: 'K9ABC' })} onSnap={onSnap} onHunt={vi.fn()} />,
    )
    expect(document.querySelector('.pota-hunt-banner')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: t('ota.hunt.clear') }))
    await waitFor(() => expect(onSnap).toHaveBeenCalledWith(cleared))
    expect(api.clearHuntTarget).toHaveBeenCalledTimes(1)
    view.rerender(<PotaSotaView snap={cleared} onSnap={onSnap} onHunt={vi.fn()} />)
    expect(document.querySelector('.pota-hunt-banner')).toBeNull()
  })
})

// @vitest-environment jsdom
//
// The Passes pane's ★/All filter (operator: "when I turn on satellites in
// connect, it should track the birds I have set as favorites"). Pinned:
//
//  - Default is ★ (favorites) — but with ZERO stars the pane shows every pass
//    (a fresh install must never render an empty sky).
//  - With stars set, only chased birds' passes render; the chip flips to All
//    and back, persisting the choice under the ONE key the map + globe layers
//    also read (nexus.sats.favOnly — surface-scoped, satChase.ts).
//  - Stars with no upcoming passes → an honest empty line, chip still
//    reachable (the escape hatch to All must never disappear).
//  - STATUS HONESTY: a ★ bird that went dead/re-entered is marked on its row,
//    and one the view could not place at all gets a named line instead of
//    disappearing — Connect is where the operator plans, and a bird silently
//    missing from it reads as "no pass", not as "no elements".
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import type { SatView } from '../../types'

const api = vi.hoisted(() => ({
  getSatellites: vi.fn((): Promise<SatView | null> => Promise.resolve(null)),
}))
vi.mock('../../api', () => api)

import { SatPassesPane } from './SatPassesPane'
import { setSatFavOnly, toggleSatChasing } from '../../features/satChase'

const NOW = Math.floor(Date.now() / 1000)
const pass = (name: string, norad: number, aosInS: number) => ({
  name,
  norad,
  aosUnix: NOW + aosInS,
  losUnix: NOW + aosInS + 600,
  maxElDeg: 45,
  aosAzDeg: 20,
  losAzDeg: 200,
})
const view: SatView = {
  tleAgeDays: 1,
  tleFetchedAt: NOW,
  tleSource: 'mirror',
  birds: [],
  passes: [pass('RS-44', 44909, 600), pass('AO-7', 7530, 1200)],
  excluded: [],
}

const chip = () => screen.getByRole('button', { name: 'Filter to ★ birds' })

beforeEach(() => {
  localStorage.clear()
  api.getSatellites.mockResolvedValue(view)
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('SatPassesPane ★/All filter', () => {
  it('zero stars → shows all passes despite the ★ default (never an empty sky)', async () => {
    render(<SatPassesPane />)
    expect(await screen.findByText('RS-44')).toBeTruthy()
    expect(screen.getByText('AO-7')).toBeTruthy()
  })

  it('with stars set, filters to the chased birds only', async () => {
    toggleSatChasing('RS-44', 44909)
    render(<SatPassesPane />)
    expect(await screen.findByText('RS-44')).toBeTruthy()
    expect(screen.queryByText('AO-7')).toBeNull()
  })

  it('the chip flips to All (both birds show) and PERSISTS the shared key', async () => {
    toggleSatChasing('RS-44', 44909)
    render(<SatPassesPane />)
    await screen.findByText('RS-44')
    fireEvent.click(chip())
    expect(await screen.findByText('AO-7')).toBeTruthy()
    expect(localStorage.getItem('nexus.sats.favOnly')).toBe('0')
    // A remount (fresh window) reads the persisted choice back.
    cleanup()
    render(<SatPassesPane />)
    expect(await screen.findByText('AO-7')).toBeTruthy()
  })

  it('reflects a chip flip made on ANOTHER surface (map/globe Layers chip) without waiting for the poll', async () => {
    toggleSatChasing('RS-44', 44909)
    render(<SatPassesPane />)
    await screen.findByText('RS-44')
    expect(screen.queryByText('AO-7')).toBeNull()
    // The map's Layers-panel chip flips the shared key; the pane must follow
    // NOW (same-window change event), not on the next 60 s poll.
    setSatFavOnly(false)
    expect(await screen.findByText('AO-7')).toBeTruthy()
  })

  it('stars with no upcoming passes → honest empty line, chip still present', async () => {
    toggleSatChasing('SO-50', 27607) // starred, but no pass in the fixture
    render(<SatPassesPane />)
    await waitFor(() => expect(api.getSatellites).toHaveBeenCalled())
    expect(await screen.findByText(/No passes for your ★ birds/)).toBeTruthy()
    expect(chip()).toBeTruthy()
    expect(screen.queryByText('RS-44')).toBeNull()
  })
})

describe('SatPassesPane status honesty', () => {
  it("marks a pass whose bird SatNOGS calls dead — the row is not silently normal", async () => {
    api.getSatellites.mockResolvedValue({
      ...view,
      passes: [{ ...pass('AO-85', 40967, 600), status: 'dead' }, pass('RS-44', 44909, 1200)],
    })
    render(<SatPassesPane />)
    expect(await screen.findByText('AO-85')).toBeTruthy()
    expect(screen.getByText('dead')).toBeTruthy()
    // The working bird stays unmarked — the chip must mean something.
    expect(screen.queryAllByText('alive')).toEqual([])
  })

  it('names a ★ bird the view could not place, instead of dropping it', async () => {
    toggleSatChasing('AO-85', 40967)
    api.getSatellites.mockResolvedValue({
      ...view,
      excluded: [
        { name: 'AO-85', norad: 40967, status: 'alive', reason: 'noElements' },
        // Not starred: the operator's pane must not grow by the whole catalog.
        { name: 'PACSAT', norad: 20439, status: 'alive', reason: 'noElements' },
      ],
    })
    render(<SatPassesPane />)
    expect(await screen.findByText(/AO-85/)).toBeTruthy()
    expect(screen.getByText(/no elements/)).toBeTruthy()
    expect(screen.queryByText(/PACSAT/)).toBeNull()
  })

  it('renders for a ★ bird that has ONLY an exclusion — never falls back to nothing', async () => {
    // No passes at all in the window. The pane used to return null here, so
    // PaneFrame showed the Basic line and the missing bird was invisible.
    toggleSatChasing('AO-85', 40967)
    api.getSatellites.mockResolvedValue({
      ...view,
      passes: [],
      excluded: [{ name: 'AO-85', norad: 40967, status: 'alive', reason: 'staleElements' }],
    })
    const { container } = render(<SatPassesPane />)
    await waitFor(() => expect(container.querySelector('.sat-pane')).toBeTruthy())
    expect(screen.getByText(/stale elements/)).toBeTruthy()
  })
})

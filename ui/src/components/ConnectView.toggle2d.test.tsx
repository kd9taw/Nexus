// @vitest-environment jsdom
//
// "2D" MEANS THE FLAT MAP (operator ruling, 2026-09-13).
//
// Tester report: "after running 3D, switching back to 2D shows '2D' at the top but the 3D image
// stays on screen". The WebGL globe DID unmount — the 2-D renderer came up in its orthographic
// Globe projection, a sphere, so "2D" still looked like a globe. Ruling: choosing 2D opens the flat
// World projection; Globe stays available as a choice inside 2D.
//
// THE RULE when it meets per-intent settings: the 3D → 2D toggle is an explicit request for the
// flat map, so it lands on World even for an intent that remembers Globe (and that becomes what the
// intent remembers). Every OTHER way into the 2-D map — an intent switch, a relaunch — restores the
// projection the operator left, Globe included.
//
// Globe3D is stubbed ONLY to observe mount/unmount (jsdom has no WebGL); MapView is real.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, fireEvent, screen, act } from '@testing-library/react'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))
vi.mock('./Globe3D', () => ({ default: () => <div data-testid="globe3d-stub" /> }))
import { ConnectView } from './ConnectView'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const props = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  needAlerts: [],
  amp: null,
}

const projection = () =>
  Array.from(document.querySelectorAll('.map-toolbar .map-proj button.active'))
    .map((b) => b.textContent)
    .filter((t) => t === 'Globe' || t === 'Beam' || t === 'World')
const toggle = () => fireEvent.click(screen.getByRole('button', { name: /[23]D/ }))
const click = (name: string) => fireEvent.click(screen.getByRole('button', { name }))

async function mount() {
  await act(async () => {
    render(<ConnectView {...props} />)
  })
}

describe('3D → 2D toggle', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('unmounts the WebGL globe and opens the flat World map', async () => {
    localStorage.setItem('nexus.connect.map3d', '1')
    await mount()
    expect(await screen.findByTestId('globe3d-stub')).toBeTruthy()
    toggle()
    expect(screen.queryByTestId('globe3d-stub')).toBeNull()
    expect(document.querySelector('.map-view')).not.toBeNull()
    expect(screen.getByRole('button', { name: /2D/ })).toBeTruthy()
    expect(projection()).toEqual(['World'])
  })

  it('lands on World even for an intent that remembers the Globe projection', async () => {
    localStorage.setItem('nexus.connect.map3d', '0')
    await mount()
    click('Globe') // Globe stays a choice inside 2D…
    expect(projection(), 'CONTROL: Chase DX really is on Globe').toEqual(['Globe'])
    toggle() // → 3D
    expect(await screen.findByTestId('globe3d-stub'), 'CONTROL: the 3-D globe mounted').toBeTruthy()
    toggle() // → 2D: an explicit request for the flat map
    expect(projection()).toEqual(['World'])
  })

  it('an intent switch inside 2D still restores Globe — only the toggle forces flat', async () => {
    localStorage.setItem('nexus.connect.map3d', '0')
    await mount()
    click('Globe')
    click('POTA/SOTA')
    click('Chase DX')
    expect(projection()).toEqual(['Globe'])
  })

  it('a first-time 2-D map opens flat for every intent preset', async () => {
    localStorage.setItem('nexus.connect.map3d', '0')
    await mount()
    for (const name of ['Chase DX', 'POTA/SOTA', 'Ragchew', '6m/VHF']) {
      click(name)
      expect(projection(), name).toEqual(['World'])
    }
  })
})

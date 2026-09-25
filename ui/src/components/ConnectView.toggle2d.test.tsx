// @vitest-environment jsdom
//
// LEAVING 3D SHOWS EXACTLY THE MAP THE OPERATOR PICKED.
//
// Tester report: "after running 3D, switching back to 2D shows '2D' at the top but the 3D image
// stays on screen". The WebGL globe DID unmount — the 2-D renderer came up in its orthographic
// Globe projection, a sphere, under a label that said "2D". The first fix forced the flat World
// map on every 3D → 2D toggle. The operator's later ruling (2026-09-13, late) removed the toggle
// instead: one picker — Globe · 3D · Flat · Beam — so there is no "2D" whose meaning has to be
// guessed, and the "3D → 2D lands on World" rule is gone with it. Leaving 3D means choosing one of
// the other three, and that choice is exactly what renders.
//
// Globe3D is stubbed ONLY to observe mount/unmount (jsdom has no WebGL) and the GPU probe answers
// "capable"; the ConnectView and MapView are real. The projection is read off the real map.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, fireEvent, screen, act, within } from '@testing-library/react'

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
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))
vi.mock('./Globe3D', () => ({ default: () => <div data-testid="globe3d-stub" /> }))
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => true }))
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

const choose = (name: string) =>
  fireEvent.click(within(screen.getByRole('group', { name: 'Map view' })).getByRole('button', { name }))
const projection = () => document.querySelector('.map-view')?.getAttribute('data-projection') ?? null

async function mount() {
  await act(async () => {
    render(<ConnectView {...props} />)
  })
}

describe('leaving the 3D globe', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('unmounts the WebGL globe and shows the 2-D map in the projection picked', async () => {
    localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { map: '3d' } }))
    await mount()
    expect(await screen.findByTestId('globe3d-stub'), 'CONTROL: it opened on 3D').toBeTruthy()
    for (const [name, kind] of [
      ['Flat', 'world'],
      ['3D', null],
      ['Beam', 'aeqd'],
      ['3D', null],
      ['Globe', 'globe'],
    ] as const) {
      choose(name)
      if (kind === null) {
        expect(await screen.findByTestId('globe3d-stub'), name).toBeTruthy()
        expect(document.querySelector('.map-view'), name).toBeNull()
      } else {
        expect(screen.queryByTestId('globe3d-stub'), `${name}: the WebGL globe stayed on screen`).toBeNull()
        expect(projection(), name).toBe(kind)
      }
    }
  })

  it('3D → Globe lands on Globe — the old "always World" rule is gone', async () => {
    await mount()
    choose('3D')
    expect(await screen.findByTestId('globe3d-stub')).toBeTruthy()
    choose('Globe')
    expect(screen.queryByTestId('globe3d-stub')).toBeNull()
    expect(projection()).toBe('globe')
  })

  it('an intent switch restores that intent’s projection, never a forced flat map', async () => {
    await mount()
    choose('Beam') // Chase DX on Beam
    fireEvent.click(screen.getByRole('button', { name: 'POTA/SOTA' }))
    choose('3D')
    expect(await screen.findByTestId('globe3d-stub')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Chase DX' }))
    expect(screen.queryByTestId('globe3d-stub')).toBeNull()
    expect(projection()).toBe('aeqd')
  })
})

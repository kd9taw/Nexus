// @vitest-environment jsdom
//
// EACH CONNECT INTENT KEEPS ITS OWN MAP SETUP.
//
// Tester report, 2026-09-13: "in Chase DX on the 2D map I switched to POTA/SOTA, liked it, went back
// to Chase DX and all my Chase DX settings had reverted". The map held ONE projection and ONE layer
// table, the colour mode was not stored at all, and every intent switch wrote that intent's preset
// over them. Now a preset is a first-use default and each intent restores what the operator left.
//
// Drives the REAL ConnectView + REAL MapView. Globe3D is stubbed only so a test that flips to 3-D can
// see which renderer mounted (jsdom has no WebGL); nothing about the 2-D map is stubbed.
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

const STORE = 'nexus.connect.intents'
const heat = () => screen.getByLabelText('Band heat (openings)') as HTMLInputElement
const parks = () => screen.getByLabelText('Parks on the air') as HTMLInputElement
/** The pressed buttons in the map toolbar: the projection and the colour mode. */
const pressed = () =>
  Array.from(document.querySelectorAll('.map-toolbar .map-proj button.active')).map((b) => b.textContent)
const intent = (name: string) => fireEvent.click(screen.getByRole('button', { name }))
const stored = () => JSON.parse(localStorage.getItem(STORE) ?? 'null')

async function mount() {
  await act(async () => {
    render(<ConnectView {...props} />)
  })
}

describe('Connect intent round-trip keeps the operator’s per-intent map setup', () => {
  beforeEach(() => {
    localStorage.clear()
    localStorage.setItem('nexus.connect.map3d', '0') // the 2-D map, as in the report
  })
  afterEach(() => cleanup())

  it('Chase DX → POTA/SOTA → Chase DX restores the Chase DX projection, layers and colour mode', async () => {
    await mount()
    // Operator tunes Chase DX: the Beam map, band heat off, coloured by signal.
    fireEvent.click(screen.getByRole('button', { name: 'Beam' }))
    fireEvent.click(heat())
    fireEvent.click(screen.getByRole('button', { name: 'Signal' }))
    // CONTROL: the picks really landed (state + persisted store) before the intent switch.
    expect(pressed()).toEqual(['Beam', 'Signal'])
    expect(heat().checked).toBe(false)
    expect(stored().dx).toMatchObject({ kind: 'aeqd', colorBy: 'snr' })
    expect(stored().dx.layers.heat.visible).toBe(false)

    intent('POTA/SOTA')
    // CONTROL: the switch really applied POTA's preset (parks on, need colouring) — otherwise the
    // return trip below could pass by never having left.
    expect(parks().checked).toBe(true)
    expect(pressed()).toContain('Need')

    intent('Chase DX')
    expect(pressed()).toEqual(['Beam', 'Signal'])
    expect(heat().checked).toBe(false)
  })

  it('the preset applies only on first use: POTA/SOTA keeps Parks OFF once the operator turned it off', async () => {
    await mount()
    intent('POTA/SOTA')
    expect(parks().checked, 'CONTROL: first use of POTA applies its preset').toBe(true)
    fireEvent.click(parks())
    intent('Chase DX')
    intent('POTA/SOTA')
    expect(parks().checked).toBe(false)
  })

  it('each intent remembers 2-D or 3-D', async () => {
    await mount()
    expect(document.querySelector('.map-view'), 'CONTROL: Chase DX starts on the 2-D map').not.toBeNull()
    intent('POTA/SOTA')
    fireEvent.click(screen.getByRole('button', { name: /2D/ }))
    // Globe3D is lazy-loaded, so the renderer arrives a tick after the click.
    expect(await screen.findByTestId('globe3d-stub'), 'POTA is now on the 3-D globe').toBeTruthy()

    intent('Chase DX')
    expect(screen.queryByTestId('globe3d-stub')).toBeNull()
    expect(document.querySelector('.map-view')).not.toBeNull()

    intent('POTA/SOTA')
    expect(await screen.findByTestId('globe3d-stub')).toBeTruthy()
  })
})

describe('upgrading from the shared map settings', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('migrates the old shared projection + layers into the intent active on first load, and only that one', async () => {
    // What an older build left on disk: ONE projection and ONE layer table, Chase DX active.
    localStorage.setItem('nexus.connect.intent', 'dx')
    localStorage.setItem('nexus.connect.map3d', '0')
    localStorage.setItem('nexus.connect.projection', 'aeqd')
    const { DEFAULT_LAYERS } = await import('./MapView')
    localStorage.setItem(
      'nexus.connect.layers',
      JSON.stringify({ ...DEFAULT_LAYERS, heat: { visible: false, opacity: 0.55 } }),
    )
    await mount()
    // Nobody loses their setup on upgrade.
    expect(pressed()).toContain('Beam')
    expect(heat().checked).toBe(false)

    // POSITIVE CONTROL — the other intents did NOT inherit it: POTA gets its own preset projection.
    intent('POTA/SOTA')
    expect(pressed()).not.toContain('Beam')

    intent('Chase DX')
    expect(pressed()).toContain('Beam')
    expect(heat().checked).toBe(false)
  })
})

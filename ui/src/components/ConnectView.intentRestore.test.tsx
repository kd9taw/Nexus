// @vitest-environment jsdom
//
// EACH CONNECT INTENT KEEPS ITS OWN MAP SETUP.
//
// Tester report, 2026-09-13: "in Chase DX on the 2D map I switched to POTA/SOTA, liked it, went back
// to Chase DX and all my Chase DX settings had reverted". The map held ONE projection and ONE layer
// table, the colour mode was not stored at all, and every intent switch wrote that intent's preset
// over them. Now a preset is a first-use default and each intent restores what the operator left:
// its map pick (Globe · 3D · Flat · Beam — the one picker), its layers and its colour mode.
//
// Drives the REAL ConnectView + REAL MapView. Globe3D is stubbed only so a test that picks 3D can
// see which renderer mounted (jsdom has no WebGL), and the GPU probe answers "capable".
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
  getLog: vi.fn(async () => []),
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

const STORE = 'nexus.connect.intents'
const heat = () => screen.getByLabelText('Band heat (openings)') as HTMLInputElement
const parks = () => screen.getByLabelText('Parks on the air') as HTMLInputElement
const picker = () => screen.getByRole('group', { name: 'Map view' })
const choose = (name: string) => fireEvent.click(within(picker()).getByRole('button', { name }))
/** The map pick, and the colour mode, as pressed on screen. */
const pick = () =>
  within(picker())
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
const colour = () =>
  Array.from(document.querySelectorAll('.map-toolbar .map-proj button.active')).map((b) => b.textContent)
const projection = () => document.querySelector('.map-view')?.getAttribute('data-projection') ?? null
const intent = (name: string) => fireEvent.click(screen.getByRole('button', { name }))
const stored = () => JSON.parse(localStorage.getItem(STORE) ?? 'null')

async function mount() {
  await act(async () => {
    render(<ConnectView {...props} />)
  })
}

describe('Connect intent round-trip keeps the operator’s per-intent map setup', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  it('Chase DX → POTA/SOTA → Chase DX restores the Chase DX map pick, layers and colour mode', async () => {
    await mount()
    // Operator tunes Chase DX: the Beam map, band heat off, coloured by signal.
    choose('Beam')
    fireEvent.click(heat())
    fireEvent.click(screen.getByRole('button', { name: 'Signal' }))
    // CONTROL: the picks really landed (state + persisted store + the map itself) before the switch.
    expect(pick()).toEqual(['Beam'])
    expect(projection()).toBe('aeqd')
    expect(colour()).toEqual(['Signal'])
    expect(heat().checked).toBe(false)
    expect(stored().dx).toMatchObject({ map: 'aeqd', colorBy: 'snr' })
    expect(stored().dx.layers.heat.visible).toBe(false)

    intent('POTA/SOTA')
    // CONTROL: the switch really applied POTA's preset (Globe, parks on, need colouring) — otherwise
    // the return trip below could pass by never having left.
    expect(pick()).toEqual(['Globe'])
    expect(parks().checked).toBe(true)
    expect(colour()).toEqual(['Need'])

    intent('Chase DX')
    expect(pick()).toEqual(['Beam'])
    expect(projection()).toBe('aeqd')
    expect(colour()).toEqual(['Signal'])
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

  it('each intent remembers 3D', async () => {
    await mount()
    expect(document.querySelector('.map-view'), 'CONTROL: Chase DX starts on the 2-D map').not.toBeNull()
    intent('POTA/SOTA')
    choose('3D')
    // Globe3D is lazy-loaded, so the renderer arrives a tick after the click.
    expect(await screen.findByTestId('globe3d-stub'), 'POTA is now on the 3-D globe').toBeTruthy()

    intent('Chase DX')
    expect(screen.queryByTestId('globe3d-stub')).toBeNull()
    expect(projection()).toBe('globe')

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
    expect(pick()).toEqual(['Beam'])
    expect(projection()).toBe('aeqd')
    expect(heat().checked).toBe(false)

    // POSITIVE CONTROL — the other intents did NOT inherit it: POTA gets its own preset, Globe.
    intent('POTA/SOTA')
    expect(pick()).toEqual(['Globe'])

    intent('Chase DX')
    expect(pick()).toEqual(['Beam'])
    expect(heat().checked).toBe(false)
  })
})

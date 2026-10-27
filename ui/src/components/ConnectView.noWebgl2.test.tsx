// @vitest-environment jsdom
//
// THE CONDITIONS MAP ON A COMPUTER THAT MAKES NO WEBGL 2 CONTEXT, AND WHEN A 3-D GLOBE FAILS ON SCREEN.
//
// three.js draws with WebGL 2 only. The GPU gate (gpu.ts `gpuCapableForGlobe`) passes a WebGL 1 context with a
// hardware renderer, so on such a machine 3D was offered, and picking it (or opening on a stored 3D pick) mounted
// a globe that threw, with nothing between it and App's section boundary: all of Conditions was the error page,
// and a stored 3D pick brought it back every time Conditions opened. 3D now needs the WebGL 2 context three.js
// asks for (gpu.ts `webgl2ForGlobe`), the way it needs a capable GPU, and a globe that fails once it is on screen
// leaves the map cell to a note; the picker above it, and the rest of Conditions, carry on.
//
// Drives the REAL ConnectView + REAL MapView inside App's real section boundary. Globe3D is stubbed (jsdom has
// no WebGL) so a test can make it throw as three.js does, and the GPU probes are stubbed so each answer can be
// driven.
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
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
}))
/** The 3-D globe: drawn, or failing as three.js fails when it cannot make its context. */
const globe = vi.hoisted(() => ({ fails: false }))
vi.mock('./Globe3D', () => ({
  default: () => {
    if (globe.fails) throw new Error('THREE.WebGLRenderer: Error creating WebGL context.')
    return <div data-testid="globe3d-stub" />
  },
}))
const gpu = vi.hoisted(() => ({ ok: true, webgl2: true }))
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => gpu.ok, webgl2ForGlobe: () => gpu.webgl2 }))
import { ConnectView } from './ConnectView'
import { ErrorBoundary } from './ErrorBoundary'
import { t } from '../i18n'

// The house budget (ConnectView.mapPicker.test.tsx): a loaded full suite runs cases far slower than one core.
vi.setConfig({ testTimeout: 15_000 })

;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
}

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
const picker = () => screen.getByRole('group', { name: 'Map view' })
const option = (name: string) => within(picker()).getByRole('button', { name })
const choose = (name: string) => fireEvent.click(option(name))
const pressed = () =>
  within(picker())
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
/** The section's error page, if that is what is on screen. */
const crash = () => document.querySelector('.view-crash')?.textContent ?? null
/** What the map cell shows: the section's error page, the 3-D globe, its note, or the 2-D map. */
async function onScreen(): Promise<string> {
  // Globe3D is lazy: give the Suspense boundary a turn before deciding it is absent.
  await act(async () => {})
  if (crash() !== null) return 'crash'
  if (screen.queryByTestId('globe3d-stub')) return '3d'
  const note = document.querySelector('.connect-map .globe-note')
  if (note) return `note:${note.textContent}`
  const map = document.querySelector('.map-view')
  return map ? `2d:${map.getAttribute('data-projection')}` : 'nothing'
}

async function mount() {
  await act(async () => {
    render(
      <ErrorBoundary label={t('features.connect.label')}>
        <ConnectView {...props} />
      </ErrorBoundary>,
    )
  })
}

let errors: ReturnType<typeof vi.spyOn>
beforeEach(() => {
  localStorage.clear()
  gpu.ok = true
  gpu.webgl2 = true
  globe.fails = false
  errors = vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('a capable GPU that makes no WebGL 2 context', () => {
  beforeEach(() => {
    gpu.webgl2 = false
  })

  it('offers 3D disabled, with the explanation, and keeps the map on Globe', async () => {
    await mount()
    const three = option('3D')
    expect(three.getAttribute('aria-disabled')).toBe('true')
    expect(three.getAttribute('title')).toBe(t('globe.unsupported'))
    choose('3D')
    expect(await onScreen()).toBe('2d:globe')
    expect(pressed()).toEqual(['Globe'])
  })

  it('a stored 3D pick shows Globe and says so, instead of the error page every time Conditions opens', async () => {
    localStorage.setItem('nexus.connect.intent', 'dx')
    localStorage.setItem(STORE, JSON.stringify({ dx: { map: '3d' } }))
    await mount()
    expect(await onScreen()).toBe('2d:globe')
    expect(pressed()).toEqual(['Globe'])
    expect(screen.getByText(t('connect.globe3d.standIn'))).toBeTruthy()
  })

  it('POSITIVE CONTROL: with the WebGL 2 context there, the same 3D is offered and draws', async () => {
    gpu.webgl2 = true
    await mount()
    expect(option('3D').getAttribute('aria-disabled')).not.toBe('true')
    choose('3D')
    expect(await onScreen()).toBe('3d')
  })
})

describe('a 3-D globe that fails on screen', () => {
  it('leaves the map cell to a note, and the picker still changes the map', async () => {
    globe.fails = true
    await mount()
    choose('3D')
    expect(await onScreen()).toBe(`note:${t('globe.failed')}`)
    expect(pressed()).toEqual(['3D'])
    choose('Flat')
    expect(await onScreen()).toBe('2d:world')
  })

  it('says what failed in the log, never silently', async () => {
    globe.fails = true
    await mount()
    choose('3D')
    await onScreen()
    // The globe's own boundary says it, naming the globe: App's would name all of Conditions.
    const said = errors.mock.calls.map((c: unknown[]) => c.map(String).join(' ')).filter((l: string) => l.startsWith('[nexus] 3-D globe failed'))
    expect(said.join('\n')).toContain('Error creating WebGL context')
  })

  it('picking 3D again mounts a fresh globe, which draws once it can', async () => {
    globe.fails = true
    await mount()
    choose('3D')
    expect(await onScreen()).toBe(`note:${t('globe.failed')}`)
    globe.fails = false
    choose('Globe')
    choose('3D')
    expect(await onScreen()).toBe('3d')
  })
})

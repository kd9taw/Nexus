// @vitest-environment jsdom
//
// ONE MAP PICKER (operator decision, 2026-09-13 late).
//
// Connect had two controls for one question — "what map am I looking at": a 2D/3D toggle in the
// header, and Globe / Beam / World inside the 2-D map's toolbar, which unmounted with the map the
// moment 3D was on. They are now one row of four choices in the map cell, the same node whatever
// renders beneath it: Globe (the 2-D orthographic globe, the default) · 3D (the WebGL globe) ·
// Flat (the world projection) · Beam (azimuthal equidistant). Each intent remembers its pick.
//
// Drives the REAL ConnectView + REAL MapView. Globe3D is stubbed only to observe which renderer
// mounted (jsdom has no WebGL), and the GPU probe is stubbed so both of its answers can be driven.
// What the 2-D map actually projects is read off the real MapView (`data-projection`), not off the
// picker's own pressed state — a picker that lit up without reaching the map would pass the latter.
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
const gpu = { ok: true }
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => gpu.ok }))
import { ConnectView } from './ConnectView'
import { EN } from '../i18n'

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
const picker = () => screen.getByRole('group', { name: 'Map view' })
const option = (name: string) => within(picker()).getByRole('button', { name })
const choose = (name: string) => fireEvent.click(option(name))
const intent = (name: string) => fireEvent.click(screen.getByRole('button', { name }))
/** The pressed choice, read off the picker. */
const pressed = () =>
  within(picker())
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
/** What is actually on screen: the WebGL globe, or the 2-D map and the projection it draws. */
async function onScreen(): Promise<string> {
  // Globe3D is lazy: give the Suspense boundary a turn before deciding it is absent.
  await act(async () => {})
  if (screen.queryByTestId('globe3d-stub')) return '3d'
  const map = document.querySelector('.map-view')
  return map ? `2d:${map.getAttribute('data-projection')}` : 'nothing'
}

async function mount() {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<ConnectView {...props} />)
  })
  return r
}

beforeEach(() => {
  localStorage.clear()
  gpu.ok = true
})
afterEach(() => cleanup())

describe('the Connect map picker', () => {
  it('offers exactly Globe · 3D · Flat · Beam, and each selects its renderer and projection', async () => {
    await mount()
    expect(within(picker()).getAllByRole('button').map((b) => b.textContent)).toEqual(['Globe', '3D', 'Flat', 'Beam'])
    const want: Array<[string, string]> = [
      ['3D', '3d'],
      ['Flat', '2d:world'],
      ['Beam', '2d:aeqd'],
      ['Globe', '2d:globe'],
    ]
    for (const [name, renders] of want) {
      choose(name)
      expect(await onScreen(), name).toBe(renders)
      expect(pressed(), name).toEqual([name])
    }
  })

  it('there is no separate 2D/3D toggle any more', async () => {
    await mount()
    expect(screen.queryByRole('button', { name: /^🌐/ })).toBeNull()
    expect(document.querySelector('.connect-3d-toggle')).toBeNull()
    // …and the 2-D map carries no projection buttons of its own: the picker is the whole choice.
    expect(within(document.querySelector('.map-toolbar') as HTMLElement).queryByRole('button', { name: 'Beam' })).toBeNull()
  })

  it('3D → Flat → Globe → Beam: the picker is the same node, in the map cell, the whole way', async () => {
    await mount()
    const node = picker()
    const cell = document.querySelector('.connect-map')!
    for (const [name, renders] of [
      ['3D', '3d'],
      ['Flat', '2d:world'],
      ['Globe', '2d:globe'],
      ['Beam', '2d:aeqd'],
    ] as const) {
      choose(name)
      expect(await onScreen(), name).toBe(renders)
      expect(picker(), `${name}: the picker was remounted or moved`).toBe(node)
      expect(cell.contains(node), name).toBe(true)
      // Never inside a renderer, which is exactly what unmounts on a switch.
      expect(node.closest('.map-view, .globe3d-wrap'), name).toBeNull()
    }
  })

  it('a fresh intent starts on Globe — every intent', async () => {
    await mount()
    for (const name of ['Chase DX', 'POTA/SOTA', 'Ragchew', '6m/VHF']) {
      intent(name)
      expect(pressed(), name).toEqual(['Globe'])
      expect(await onScreen(), name).toBe('2d:globe')
    }
  })

  it('each intent remembers its pick across an intent switch and a reload', async () => {
    const first = await mount()
    choose('3D') // Chase DX
    intent('POTA/SOTA')
    expect(pressed(), 'CONTROL: POTA/SOTA did not inherit Chase DX’s 3D').toEqual(['Globe'])
    choose('Beam')
    intent('Chase DX')
    expect(await onScreen()).toBe('3d')
    expect(JSON.parse(localStorage.getItem(STORE)!).dx.map).toBe('3d')

    // Reload: a true unmount and a fresh mount from storage alone.
    await act(async () => first.unmount())
    await mount()
    expect(pressed()).toEqual(['3D'])
    expect(await onScreen()).toBe('3d')
    intent('POTA/SOTA')
    expect(pressed()).toEqual(['Beam'])
    expect(await onScreen()).toBe('2d:aeqd')
  })
})

describe('upgrading stored map settings into the picker', () => {
  it.each([
    ['a per-intent map3d:true → 3D (whatever kind it also held)', { [STORE]: JSON.stringify({ dx: { map3d: true, kind: 'aeqd' } }) }, '3D', '3d'],
    ['a per-intent map3d:false keeps its kind', { [STORE]: JSON.stringify({ dx: { map3d: false, kind: 'aeqd' } }) }, 'Beam', '2d:aeqd'],
    ['a per-intent kind alone', { [STORE]: JSON.stringify({ dx: { kind: 'world' } }) }, 'Flat', '2d:world'],
    ['an unknown kind → Globe', { [STORE]: JSON.stringify({ dx: { kind: 'mercator' } }) }, 'Globe', '2d:globe'],
    ['an unknown pick → Globe', { [STORE]: JSON.stringify({ dx: { map: 'hologram' } }) }, 'Globe', '2d:globe'],
    ['the legacy shared 3D flag → 3D', { 'nexus.connect.map3d': '1', 'nexus.connect.projection': 'world' }, '3D', '3d'],
    ['the legacy shared projection → its projection', { 'nexus.connect.map3d': '0', 'nexus.connect.projection': 'world' }, 'Flat', '2d:world'],
  ])('%s', async (_what, stored, pick, renders) => {
    localStorage.setItem('nexus.connect.intent', 'dx')
    for (const [k, v] of Object.entries(stored)) localStorage.setItem(k, v)
    await mount()
    expect(pressed()).toEqual([pick])
    expect(await onScreen()).toBe(renders)
  })
})

describe('a machine whose GPU cannot run the 3D globe', () => {
  beforeEach(() => {
    gpu.ok = false
  })

  it('offers 3D disabled, with the explanation, and still defaults to Globe', async () => {
    await mount()
    expect(pressed()).toEqual(['Globe'])
    const three = option('3D')
    expect(three.getAttribute('aria-disabled')).toBe('true')
    expect(three.getAttribute('title')).toBe(EN['globe.unsupported'])
    choose('3D')
    expect(await onScreen()).toBe('2d:globe')
    expect(pressed()).toEqual(['Globe'])
  })

  it('a 3D pick stored on this machine shows Globe instead of a globe it cannot draw', async () => {
    localStorage.setItem('nexus.connect.intent', 'dx')
    localStorage.setItem(STORE, JSON.stringify({ dx: { map: '3d' } }))
    await mount()
    expect(await onScreen()).toBe('2d:globe')
    expect(pressed()).toEqual(['Globe'])
  })

  it('POSITIVE CONTROL — on a capable GPU the same 3D option is enabled and works', async () => {
    gpu.ok = true
    await mount()
    expect(option('3D').getAttribute('aria-disabled')).not.toBe('true')
    choose('3D')
    expect(await onScreen()).toBe('3d')
  })
})

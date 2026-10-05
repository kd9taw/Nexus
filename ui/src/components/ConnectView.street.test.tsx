// @vitest-environment jsdom
//
// THE STREET CHOICE in Connect's map picker (operator ruling 2026-10-04, D5), driven through the REAL
// ConnectView and MapView. The street renderer is stood in for (jsdom has no WebGL), and the street-map
// commands answer at the API boundary. Pinned here: Street is hidden from everyone until its maps are
// hosted (the manifest constant is empty) and appears only with the bench flag; without a pack it
// carries a download badge and a press opens the download sheet; with one it draws; a download shows
// its percent on the choice; and a stored Street pick that cannot draw shows Flat and says why,
// keeping the pick.
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { StreetPack } from '../features/streetPack'
import type { StreetProgress } from '../features/streetMaps'

const bench = vi.hoisted(() => ({
  on: true,
  packs: [] as unknown[],
  webgl2: true,
  progress: null as null | ((p: StreetProgress) => void),
}))
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  isTauri: () => true,
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
  streetMapInfo: vi.fn(async () => ({ folder: '/home/op/.local/share/Nexus/maps', bench: bench.on })),
  streetMapPacks: vi.fn(async () => bench.packs),
  streetMapUnfinished: vi.fn(async () => []),
  streetMapSize: vi.fn(async (area: { km: number; detail: string }) => ({
    packId: 'p',
    bytes: 126_400_000,
    downloadBytes: 126_400_000,
    assetsBytes: 0,
    tiles: 1000,
    requests: 40,
    bbox: [0, 0, 1, 1],
    minZoom: 0,
    maxZoom: 14,
    detail: area.detail,
    buildId: '20261004',
    dataDate: '2026-10-04',
    freeBytes: 40_000_000_000,
    enoughSpace: true,
    resumeBytes: 0,
    installed: false,
  })),
  streetMapDownload: vi.fn(
    (_area: unknown, onProgress: (p: StreetProgress) => void) =>
      new Promise(() => {
        bench.progress = onProgress
      }),
  ),
}))
vi.mock('./Globe3D', () => ({ default: () => <div data-testid="globe3d-stub" /> }))
vi.mock('./StreetMap', () => ({ default: () => <div className="street-map" data-testid="street-stub" /> }))
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => true, webgl2Available: () => bench.webgl2 }))
import { ConnectView } from './ConnectView'
import { t } from '../i18n'
import { __resetStreetMapsForTests, STREET_MAP_MANIFEST_URL } from '../features/streetMaps'

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

const PACK: StreetPack = {
  id: '20261004-streets-200km-4250n08900w',
  name: 'EN52 200 km',
  bbox: [-90.2, 41.6, -87.8, 43.4],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 126_400_000,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
}

const STORE = 'nexus.connect.intents'
// `hidden`: while the download sheet is open, Radix hides the rest of the page from the accessibility
// tree, and the picker is still there to be read.
const picker = () => screen.getByRole('group', { name: t('map.projection.aria'), hidden: true })
const names = () => within(picker()).getAllByRole('button', { hidden: true }).map((b) => b.textContent)
const street = () => within(picker()).getByRole('button', { name: new RegExp(`^${t('map.projection.street.label')}`), hidden: true })
const pressed = () =>
  within(picker())
    .getAllByRole('button', { hidden: true })
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
async function onScreen(): Promise<string> {
  await act(async () => {})
  const map = document.querySelector('.map-view')
  return map ? `2d:${map.getAttribute('data-projection')}` : 'nothing'
}
async function settle() {
  for (let i = 0; i < 4; i++) await act(async () => {})
}
async function mount() {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<ConnectView {...props} />)
  })
  await settle()
  return r
}

beforeEach(() => {
  localStorage.clear()
  __resetStreetMapsForTests()
  bench.on = true
  bench.packs = []
  bench.webgl2 = true
  bench.progress = null
})
afterEach(() => cleanup())

describe('the Street choice — hidden until its maps are hosted', () => {
  it('is hidden from everyone while the manifest constant is empty and no bench asks for it', async () => {
    expect(STREET_MAP_MANIFEST_URL, 'a URL here shows Street to every operator').toBe('')
    bench.on = false
    await mount()
    expect(names()).toEqual(['Globe', '3D', 'Flat', 'Beam'])
  })

  it('appears last on a bench run (NEXUS_STREET_MAP=1), so the four keep their places', async () => {
    await mount()
    expect(names()[4]).toMatch(new RegExp(`^${t('map.projection.street.label')}`))
    expect(names().slice(0, 4)).toEqual(['Globe', '3D', 'Flat', 'Beam'])
  })
})

describe('the Street choice — without a pack', () => {
  it('carries a download badge, and a press opens the download sheet instead of a map that cannot draw', async () => {
    await mount()
    expect(street().querySelector('.map-picker-badge')).not.toBeNull()
    expect(street().getAttribute('title')).toBe(t('map.projection.street.download'))
    fireEvent.click(street())
    await settle()
    expect(screen.getByRole('dialog', { name: t('map.street.sheet.title') })).toBeTruthy()
    expect(pressed()).toEqual(['Globe'])
    expect(await onScreen()).toBe('2d:globe')
    expect(localStorage.getItem(STORE) ?? '').not.toContain('street')
  })

  it("shows a running download's percent on the choice", async () => {
    await mount()
    fireEvent.click(street())
    await act(async () => {
      await new Promise((r) => setTimeout(r, 300))
    })
    await settle()
    fireEvent.click(screen.getByRole('button', { name: t('map.street.sheet.download') }))
    await settle()
    act(() => bench.progress?.({ phase: 'tiles', done: 42, total: 100, bytesPerSec: 1_000_000, etaSecs: 60 }))
    expect(street().querySelector('.map-picker-chip')?.textContent).toBe(t('map.street.chip', { pct: 42 }))
    expect(street().querySelector('.map-picker-badge')).toBeNull()
  })
})

describe('the Street choice — with a pack', () => {
  it('draws the street map and remembers the pick for the intent', async () => {
    bench.packs = [PACK]
    await mount()
    expect(street().querySelector('.map-picker-badge')).toBeNull()
    fireEvent.click(street())
    expect(await onScreen()).toBe('2d:street')
    expect(screen.getByTestId('street-stub')).toBeTruthy()
    expect(JSON.parse(localStorage.getItem(STORE)!).dx.map).toBe('street')
  })
})

describe('a stored Street pick that cannot draw shows Flat, says why, and keeps the pick', () => {
  beforeEach(() => {
    localStorage.setItem('nexus.connect.intent', 'dx')
    localStorage.setItem(STORE, JSON.stringify({ dx: { map: 'street' } }))
  })
  const kept = () => JSON.parse(localStorage.getItem(STORE)!).dx.map

  it('no pack on this computer', async () => {
    await mount()
    expect(await onScreen()).toBe('2d:world')
    expect(screen.getByText(t('map.street.standIn.noPack'))).toBeTruthy()
    expect(kept()).toBe('street')
  })

  it('no WebGL2: Street is unavailable, with the reason', async () => {
    bench.packs = [PACK]
    bench.webgl2 = false
    await mount()
    expect(await onScreen()).toBe('2d:world')
    expect(screen.getByText(t('map.street.standIn.noWebgl2'))).toBeTruthy()
    expect(street().getAttribute('aria-disabled')).toBe('true')
    expect(street().getAttribute('title')).toBe(t('map.street.noWebgl2'))
    fireEvent.click(street())
    expect(await onScreen()).toBe('2d:world')
    expect(kept()).toBe('street')
  })

  it('no longer offered here (the bench flag is gone)', async () => {
    bench.on = false
    bench.packs = [PACK]
    await mount()
    expect(await onScreen()).toBe('2d:world')
    expect(screen.getByText(t('map.street.standIn.hidden'))).toBeTruthy()
    expect(kept()).toBe('street')
  })

  it('draws again the moment it can (a pack installed)', async () => {
    bench.packs = [PACK]
    await mount()
    expect(await onScreen()).toBe('2d:street')
    expect(screen.queryByText(t('map.street.standIn.noPack'))).toBeNull()
  })
})

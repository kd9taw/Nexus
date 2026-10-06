// @vitest-environment jsdom
//
// The street map's webview state (features/streetMaps.ts) against the street-map commands, which
// answer here at the API boundary: when the choice is offered at all, what one download looks like to
// every view of it (progress, cancel, a pause that Retry resumes, a refusal), Remove and Update, and
// which pack the map draws.
import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { StreetPack } from './streetPack'

const api = vi.hoisted(() => ({
  tauri: true,
  bench: false,
  packs: [] as unknown[],
  unfinished: [] as unknown[],
  download: null as null | {
    onProgress: (p: unknown) => void
    resolve: (p: unknown) => void
    reject: (e: unknown) => void
  },
  removed: [] as string[],
  probes: 0,
}))
vi.mock('../api', () => ({
  isTauri: () => api.tauri,
  streetMapInfo: vi.fn(async () => ({ folder: '/maps', bench: api.bench })),
  streetMapPacks: vi.fn(async () => api.packs),
  streetMapUnfinished: vi.fn(async () => api.unfinished),
  streetMapDownload: vi.fn(
    (_area: unknown, onProgress: (p: unknown) => void) =>
      new Promise((resolve, reject) => {
        api.download = { onProgress, resolve, reject }
      }),
  ),
  streetMapCancel: vi.fn(async () => {
    api.download?.reject({ kind: 'cancelled', message: 'cancelled' })
    return true
  }),
  streetMapRemove: vi.fn(async (id: string) => {
    api.removed.push(id)
    api.packs = api.packs.filter((p) => (p as StreetPack).id !== id)
    return 126_400_000
  }),
  streetMapUpdates: vi.fn(async () => []),
  streetMapInstallFile: vi.fn(async () => null),
}))
vi.mock('../gpu', () => ({
  webgl2Available: () => {
    api.probes++
    return true
  },
}))
import {
  __resetStreetMapsForTests,
  cancelStreetDownload,
  packFor,
  removeStreetMap,
  startStreetDownload,
  STREET_MAP_MANIFEST_URL,
  updateStreetMap,
  useStreetMaps,
  type StreetArea,
} from './streetMaps'
import { streetMapUpdates } from '../api'

const pack = (id: string, bbox: [number, number, number, number]): StreetPack => ({
  id,
  name: id,
  bbox,
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 1,
  dataDate: '2026-10-04',
  sha256: '00',
})
const HOME = pack('home', [-98.2, 38.4, -97, 39.3])
const AREA: StreetArea = { lat: 38.84, lon: -97.61, km: 200, detail: 'streets' }

async function loaded() {
  const hook = renderHook(() => useStreetMaps())
  for (let i = 0; i < 4; i++) await act(async () => {})
  return hook
}

beforeEach(() => {
  __resetStreetMapsForTests()
  Object.assign(api, { tauri: true, bench: false, packs: [], unfinished: [], download: null, removed: [], probes: 0 })
})
afterEach(() => vi.clearAllMocks())

describe('whether the Street choice is offered', () => {
  it('is offered to every desktop operator once the manifest constant names the host, no bench needed', async () => {
    expect(STREET_MAP_MANIFEST_URL, 'the address the Rust side fetches the index from').toBe(
      'https://maps.hamradiotools.io/streetmaps.json',
    )
    api.packs = [HOME]
    const { result } = await loaded()
    expect(result.current).toMatchObject({ offered: true, bench: false, folder: '/maps', webgl2: true, packs: [HOME] })
    expect(api.probes).toBe(1)
  })

  it('is offered on a bench run (NEXUS_STREET_MAP=1), with the packs and the GPU answer', async () => {
    api.bench = true
    api.packs = [HOME]
    const { result } = await loaded()
    expect(result.current).toMatchObject({ offered: true, bench: true, folder: '/maps', webgl2: true, packs: [HOME] })
    expect(api.probes).toBe(1)
  })

  it('is never offered outside the desktop shell (the Remote page), bench or not', async () => {
    api.tauri = false
    api.bench = true
    const { result } = await loaded()
    expect(result.current.offered).toBe(false)
  })
})

describe('one download, as every view sees it', () => {
  beforeEach(() => {
    api.bench = true
  })

  it('runs with a percent, then lists the new pack and goes idle', async () => {
    const { result } = await loaded()
    let done: Promise<StreetPack | null>
    act(() => {
      done = startStreetDownload(AREA)
    })
    expect(result.current.download).toMatchObject({ state: 'running', percent: 0 })
    act(() => api.download!.onProgress({ phase: 'assets', done: 1, total: 2 }))
    expect(result.current.download).toMatchObject({ state: 'running', percent: 0 })
    act(() => api.download!.onProgress({ phase: 'tiles', done: 63, total: 126, bytesPerSec: 1, etaSecs: 9 }))
    expect(result.current.download).toMatchObject({ state: 'running', percent: 50 })
    // A retry keeps the percent it had.
    act(() => api.download!.onProgress({ phase: 'retrying', attempt: 1, waitSecs: 2, reason: 'reset' }))
    expect(result.current.download).toMatchObject({ state: 'running', percent: 50 })
    act(() => api.download!.onProgress({ phase: 'verifying', done: 1, total: 9 }))
    expect(result.current.download).toMatchObject({ state: 'running', percent: 99 })
    api.packs = [HOME]
    await act(async () => {
      api.download!.resolve(HOME)
      expect(await done!).toEqual(HOME)
    })
    expect(result.current.download).toEqual({ state: 'idle' })
    expect(result.current.packs).toEqual([HOME])
  })

  it('starts nothing while one runs', async () => {
    await loaded()
    void startStreetDownload(AREA)
    expect(await startStreetDownload(AREA)).toBeNull()
  })

  it('goes back to idle on Cancel (Download resumes what it kept)', async () => {
    const { result } = await loaded()
    let done: Promise<StreetPack | null>
    act(() => {
      done = startStreetDownload(AREA)
    })
    await act(async () => {
      await cancelStreetDownload()
      expect(await done!).toBeNull()
    })
    expect(result.current.download).toEqual({ state: 'idle' })
  })

  it('stops with Retry after the network gives out, and without it for a full disk', async () => {
    const { result } = await loaded()
    for (const [kind, retry] of [
      ['paused', true],
      ['network', true],
      ['buildGone', true],
      ['diskSpace', false],
      ['invalidArchive', false],
    ] as const) {
      let done: Promise<StreetPack | null>
      act(() => {
        done = startStreetDownload(AREA)
      })
      await act(async () => {
        api.download!.reject({ kind, message: 'english diagnostics' })
        await done!
      })
      expect(result.current.download, kind).toEqual({
        state: 'stopped',
        area: AREA,
        error: { kind, message: 'english diagnostics' },
        retry,
      })
    }
  })
})

describe('Remove and Update', () => {
  beforeEach(() => {
    api.bench = true
  })

  it('removes a pack, says the bytes freed, and lists again', async () => {
    api.packs = [HOME]
    const { result } = await loaded()
    let freed = 0
    await act(async () => {
      freed = await removeStreetMap('home')
    })
    expect(freed).toBe(126_400_000)
    expect(result.current.packs).toEqual([])
  })

  it('updates by downloading the newer build of the same square, then removing the old pack', async () => {
    const next = pack('next', HOME.bbox)
    api.packs = [HOME]
    await loaded()
    let done: Promise<StreetPack | null>
    act(() => {
      done = updateStreetMap({ packId: 'home', area: AREA, dataDate: '2026-10-04', newBuildId: '20270104', newDataDate: '2027-01-04' })
    })
    expect(api.removed, 'the old pack stays until the new one is in').toEqual([])
    api.packs = [HOME, next]
    await act(async () => {
      api.download!.resolve(next)
      await done!
    })
    expect(api.removed).toEqual(['home'])
  })

  it('never checks for an update by itself', async () => {
    api.packs = [HOME]
    await loaded()
    expect(streetMapUpdates).not.toHaveBeenCalled()
  })
})

describe('which pack the map draws', () => {
  const AWAY = pack('away', [11.9, 41.5, 13.1, 42.3])
  it('the one holding the station, else the newest', () => {
    expect(packFor([HOME, AWAY], { lat: 38.84, lon: -97.61 })).toBe(HOME)
    expect(packFor([HOME, AWAY], { lat: 0, lon: 0 })).toBe(AWAY)
    expect(packFor([HOME, AWAY], null)).toBe(AWAY)
    expect(packFor([], { lat: 38.84, lon: -97.61 })).toBeNull()
    expect(packFor(null, null)).toBeNull()
  })
})

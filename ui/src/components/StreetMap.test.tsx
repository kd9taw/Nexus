// @vitest-environment jsdom
//
// The street map's own behaviour, with MapLibre replaced by a recorder (jsdom has no WebGL and
// never draws): what it builds, what stands in for it when it cannot draw, what it hands the
// overlays, and that it lets go of its GPU context. Drawing itself is checked in a real browser.
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { StyleSpecification } from 'maplibre-gl'
import { t } from '../i18n'
import { packUrl, type StreetPack } from '../features/streetPack'
import { createRef } from 'react'
import StreetMap, { type StreetCamera, type StreetMapHandle, type StreetPointer } from './StreetMap'

const h = vi.hoisted(() => {
  const state = { webgl2: true, throwOnCreate: null as Error | null }
  type Handler = (e?: unknown) => void
  class FakeMap {
    static all: FakeMap[] = []
    options: Record<string, unknown>
    handlers: Record<string, Handler[]> = {}
    controls: Array<{ control: unknown; position?: string }> = []
    removed = 0
    styles: StyleSpecification[] = []
    centre: [number, number]
    zoom: number
    moving = false
    pixelRatio = 1
    touchZoomRotate = { disableRotation: vi.fn() }
    keyboard = { disableRotation: vi.fn() }
    setPixelRatio = vi.fn((r: number) => {
      this.pixelRatio = r
    })
    container = document.createElement('div')
    zoomTo = vi.fn()
    getCanvasContainer() {
      return this.container
    }
    constructor(options: Record<string, unknown>) {
      if (state.throwOnCreate) throw state.throwOnCreate
      this.options = options
      this.centre = options.center as [number, number]
      this.zoom = options.zoom as number
      FakeMap.all.push(this)
    }
    on(type: string, fn: Handler) {
      ;(this.handlers[type] ??= []).push(fn)
      return this
    }
    fire(type: string, e?: unknown) {
      for (const fn of this.handlers[type] ?? []) fn(e)
    }
    addControl(control: unknown, position?: string) {
      this.controls.push({ control, position })
      return this
    }
    remove() {
      this.removed++
    }
    setStyle(style: StyleSpecification) {
      this.styles.push(style)
    }
    getCenter() {
      return { lng: this.centre[0], lat: this.centre[1] }
    }
    getZoom() {
      return this.zoom
    }
    getBearing() {
      return 0
    }
    isMoving() {
      return this.moving
    }
    getPixelRatio() {
      return this.pixelRatio
    }
    project([lon, lat]: [number, number]) {
      return { x: 500 + (lon - this.centre[0]) * 100, y: 300 + (this.centre[1] - lat) * 100 }
    }
  }
  class FakeAttribution {
    options: { compact?: boolean; customAttribution?: string }
    constructor(options: { compact?: boolean; customAttribution?: string }) {
      this.options = options
    }
  }
  class FakeGpuError extends Error {}
  return { state, FakeMap, FakeAttribution, FakeGpuError, close: vi.fn() }
})

vi.mock('maplibre-gl', () => ({
  MapLibreMap: h.FakeMap,
  AttributionControl: h.FakeAttribution,
  GPUInitializationError: h.FakeGpuError,
  setWorkerUrl: vi.fn(),
  addProtocol: vi.fn(),
}))
vi.mock('../gpu', () => ({ webgl2Available: () => h.state.webgl2 }))
vi.mock('../features/streetPack', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/streetPack')>()),
  openStreetPack: vi.fn(() => h.close),
}))

const PACK: StreetPack = {
  id: 'home-200km',
  name: 'Home',
  bbox: [-98.2, 38.4, -97.0, 39.3],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 5_070_601,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
}

const theMap = () => {
  expect(h.FakeMap.all).toHaveLength(1)
  return h.FakeMap.all[0]
}

beforeEach(() => {
  document.documentElement.setAttribute('data-theme', 'dark')
})
afterEach(() => {
  // Unmount first: the setup file's own cleanup runs after this hook, and an unmount it did would
  // close the pack after the counts below were reset.
  cleanup()
  h.FakeMap.all = []
  h.state.webgl2 = true
  h.state.throwOnCreate = null
  h.close.mockClear()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  document.documentElement.removeAttribute('data-theme')
})

describe('StreetMap — what stands in when it cannot draw', () => {
  it('says why without WebGL2, and never builds a map', () => {
    h.state.webgl2 = false
    render(<StreetMap pack={PACK} />)
    expect(screen.getByRole('status').textContent).toBe(t('map.street.noWebgl2'))
    expect(h.FakeMap.all).toHaveLength(0)
  })

  it('says the same when MapLibre cannot get its GPU context, rather than crashing', () => {
    h.state.throwOnCreate = new h.FakeGpuError('Failed to initialize WebGL')
    render(<StreetMap pack={PACK} />)
    expect(screen.getByRole('status').textContent).toBe(t('map.street.noWebgl2'))
    expect(h.close).toHaveBeenCalledTimes(1)
  })

  it('says the pack cannot be read when its header fails, and shrugs off a single bad tile', () => {
    render(<StreetMap pack={PACK} />)
    const map = theMap()
    act(() => map.fire('error', { error: new Error('bad tile'), sourceId: 'protomaps', tile: {} }))
    expect(screen.queryByRole('status')).toBeNull()
    act(() => map.fire('error', { error: new Error('no such pack'), sourceId: 'protomaps' }))
    expect(screen.getByRole('status').textContent).toBe(t('map.street.unreadable'))
    expect(map.removed).toBe(1)
  })
})

describe('StreetMap — the map it builds', () => {
  it('opens one map on the pack, rotation and pitch off and the world drawn once', () => {
    render(<StreetMap pack={PACK} />)
    const map = theMap()
    expect(map.options).toMatchObject({
      container: expect.any(HTMLElement),
      center: [expect.closeTo(-97.6, 9), expect.closeTo(38.85, 9)],
      zoom: 12,
      attributionControl: false,
      dragRotate: false,
      pitchWithRotate: false,
      touchPitch: false,
      maxPitch: 0,
      renderWorldCopies: false,
      locale: { 'Map.Title': t('map.street.aria') },
    })
    expect((map.options.style as StyleSpecification).sources).toEqual({ protomaps: { type: 'vector', url: packUrl(PACK) } })
    expect(map.touchZoomRotate.disableRotation).toHaveBeenCalled()
    expect(map.keyboard.disableRotation).toHaveBeenCalled()
  })

  it('opens where the caller says', () => {
    render(<StreetMap pack={PACK} center={[-97.61, 38.84]} zoom={15} />)
    expect(theMap().options).toMatchObject({ center: [-97.61, 38.84], zoom: 15 })
  })

  it('credits © OpenStreetMap, linked to its copyright page, in a control that never folds away', () => {
    render(<StreetMap pack={PACK} />)
    const [{ control }] = theMap().controls
    const { options } = control as InstanceType<typeof h.FakeAttribution>
    expect(options.compact).toBe(false)
    const a = new DOMParser().parseFromString(options.customAttribution ?? '', 'text/html').querySelector('a')
    expect(a?.textContent).toBe('© OpenStreetMap')
    expect(a?.getAttribute('href')).toBe('https://www.openstreetmap.org/copyright')
    // A `_blank` anchor is what externalLinks.ts routes to the system browser.
    expect(a?.getAttribute('target')).toBe('_blank')
  })

  it('removes the map, and so its GPU context, on unmount, and closes the pack', () => {
    const { unmount } = render(<StreetMap pack={PACK} />)
    const map = theMap()
    expect(map.removed).toBe(0)
    unmount()
    expect(map.removed).toBe(1)
    expect(h.close).toHaveBeenCalledTimes(1)
  })

  it('keeps the open map when the same pack is listed again, and swaps it for a different pack', () => {
    const { rerender } = render(<StreetMap pack={PACK} />)
    const map = theMap()
    // `street_map_packs()` answers with new objects every time it is asked.
    rerender(<StreetMap pack={{ ...PACK }} />)
    expect(h.FakeMap.all).toHaveLength(1)
    expect(map.removed).toBe(0)
    expect(map.styles).toEqual([])
    // An updated pack (a new sha256) is a different archive: the old map goes, a new one opens on it.
    const updated = { ...PACK, sha256: 'cd'.repeat(32) }
    rerender(<StreetMap pack={updated} />)
    expect(map.removed).toBe(1)
    expect(h.FakeMap.all).toHaveLength(2)
    expect((h.FakeMap.all[1].options.style as StyleSpecification).sources).toEqual({
      protomaps: { type: 'vector', url: packUrl(updated) },
    })
  })

  it('repaints the open map in place when the theme changes, never building a second one', async () => {
    render(<StreetMap pack={PACK} />)
    const map = theMap()
    expect((map.options.style as StyleSpecification).sprite).toBe('street-asset://sprites/dark')
    await act(async () => {
      document.documentElement.setAttribute('data-theme', 'light')
    })
    expect(map.styles.map((s) => s.sprite)).toEqual(['street-asset://sprites/light'])
    expect(h.FakeMap.all).toHaveLength(1)
  })

  it('sizes its bitmap by device pixels per layout pixel, so the app zoom never softens it', () => {
    let fire: (entries: unknown[]) => void = () => {}
    const observe = vi.fn()
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: (entries: unknown[]) => void) {
          fire = cb
        }
        observe = observe
        disconnect() {}
      },
    )
    render(<StreetMap pack={PACK} />)
    const map = theMap()
    expect(observe).toHaveBeenCalledWith(expect.any(HTMLElement), { box: 'device-pixel-content-box' })
    // 125% app zoom on a 2× screen: 1000 layout px paint 2500 device px.
    fire([{ devicePixelContentBoxSize: [{ inlineSize: 2500 }], contentBoxSize: [{ inlineSize: 1000 }] }])
    expect(map.setPixelRatio).toHaveBeenCalledWith(2.5)
  })
})

describe('StreetMap — the context-loss watchdog', () => {
  it('shows "paused" with Reload when a lost context stays lost for 10 s, and drops the dead map', () => {
    vi.useFakeTimers()
    render(<StreetMap pack={PACK} />)
    const map = theMap()
    act(() => map.fire('webglcontextlost'))
    act(() => vi.advanceTimersByTime(9_999))
    expect(screen.queryByRole('status')).toBeNull()
    act(() => vi.advanceTimersByTime(1))
    expect(screen.getByRole('status').textContent).toContain(t('map.street.paused'))
    expect(map.removed).toBe(1)

    fireEvent.click(screen.getByRole('button', { name: t('map.street.reload') }))
    expect(screen.queryByRole('status')).toBeNull()
    expect(h.FakeMap.all).toHaveLength(2)
    expect(h.FakeMap.all[1].removed).toBe(0)
  })

  it('shows nothing when the context comes back in time', () => {
    vi.useFakeTimers()
    render(<StreetMap pack={PACK} />)
    const map = theMap()
    act(() => map.fire('webglcontextlost'))
    act(() => vi.advanceTimersByTime(5_000))
    act(() => map.fire('webglcontextrestored'))
    act(() => vi.advanceTimersByTime(60_000))
    expect(screen.queryByRole('status')).toBeNull()
    expect(map.removed).toBe(0)
  })
})

describe('StreetMap — the camera it exposes', () => {
  it('hands over centre, zoom, bearing and the projection at once, then on every move and resize', () => {
    const cameras: StreetCamera[] = []
    render(<StreetMap pack={PACK} onCamera={(c) => c && cameras.push(c)} />)
    const map = theMap()
    expect(cameras).toHaveLength(1)
    expect(cameras[0]).toMatchObject({ center: [expect.closeTo(-97.6, 9), expect.closeTo(38.85, 9)], zoom: 12, bearing: 0 })
    // The projection is MapLibre's own, live.
    expect(cameras[0].project(map.centre)).toEqual([500, 300])

    map.centre = [-97.5, 38.9]
    map.zoom = 13.5
    act(() => map.fire('move'))
    expect(cameras).toHaveLength(2)
    expect(cameras[1]).toMatchObject({ center: [-97.5, 38.9], zoom: 13.5 })
    act(() => map.fire('resize'))
    expect(cameras).toHaveLength(3)
  })

  it('says while the map is moving, and is heard once more when it stops', () => {
    const cameras: StreetCamera[] = []
    render(<StreetMap pack={PACK} onCamera={(c) => c && cameras.push(c)} />)
    const map = theMap()
    expect(cameras[0].moving).toBe(false)
    map.moving = true
    map.centre = [-97.5, 38.9]
    act(() => map.fire('move'))
    act(() => map.fire('render'))
    expect(cameras.slice(1).map((c) => c.moving)).toEqual([true, true])
    // MapLibre is no longer moving when it fires `moveend`.
    map.moving = false
    act(() => map.fire('moveend'))
    expect(cameras).toHaveLength(4)
    expect(cameras[3]).toMatchObject({ center: [-97.5, 38.9], moving: false })
  })
})

describe('StreetMap — what it hands the overlays', () => {
  it('re-syncs the camera on every frame MapLibre renders, and says null when the map goes', () => {
    const cameras: Array<StreetCamera | null> = []
    const { unmount } = render(<StreetMap pack={PACK} onCamera={(c) => cameras.push(c)} />)
    const map = theMap()
    map.centre = [-97.4, 38.7]
    act(() => map.fire('render'))
    expect(cameras).toHaveLength(2)
    expect(cameras[1]).toMatchObject({ center: [-97.4, 38.7] })
    unmount()
    expect(cameras[cameras.length - 1]).toBeNull()
  })

  it("passes MapLibre's pointer events on in the map's px, and a double-click the caller takes does not zoom", () => {
    const seen: StreetPointer[] = []
    render(
      <StreetMap
        pack={PACK}
        onPointer={(e) => {
          seen.push(e)
          if (e.type === 'dblclick') e.preventDefault()
        }}
      />,
    )
    const map = theMap()
    const ev = (x: number, y: number) => ({ point: { x, y }, preventDefault: vi.fn() })
    const move = ev(10, 20)
    const dbl = ev(30, 40)
    act(() => {
      map.fire('mousemove', move)
      map.fire('click', ev(11, 21))
      map.fire('dblclick', dbl)
      map.fire('mouseout', ev(0, 0))
    })
    expect(seen.map((e) => [e.type, e.x, e.y])).toEqual([
      ['move', 10, 20],
      ['click', 11, 21],
      ['dblclick', 30, 40],
      ['out', 0, 0],
    ])
    expect(dbl.preventDefault).toHaveBeenCalledTimes(1)
    expect(move.preventDefault).not.toHaveBeenCalled()
  })

  it("shows the caller's cursor over the map, and gives MapLibre's own back", () => {
    const { rerender } = render(<StreetMap pack={PACK} cursor="pointer" />)
    const map = theMap()
    expect(map.container.style.cursor).toBe('pointer')
    rerender(<StreetMap pack={PACK} />)
    expect(map.container.style.cursor).toBe('')
  })

  it('zooms by whole steps when asked', () => {
    const ref = createRef<StreetMapHandle>()
    render(<StreetMap ref={ref} pack={PACK} />)
    act(() => ref.current?.zoomBy(-1))
    expect(theMap().zoomTo).toHaveBeenCalledWith(11)
  })
})

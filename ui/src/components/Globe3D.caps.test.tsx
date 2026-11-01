// @vitest-environment jsdom
//
// THE CAPS AND OVALS ON THE 3-D GLOBE: the aurora layer turns itself on in a storm and draws OVATION's
// own edge, and none at all when OVATION is stale; the polar cap's approximate edge is
// a dashed line group only at a measured Kp; the cap and aurora chips sit in the chip stack. Each with
// a control. The pure rules are pinned in capsNow.test.ts and the 2-D map's in MapView.caps.test.tsx.
//
// The react-globe.gl stub is the one Globe3D.sky.test.tsx uses (jsdom has no WebGL); Globe3D is the
// REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle } from 'react'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import type { PropagationSnapshot } from '../types'
import type * as THREE_NS from 'three'

const feeds = vi.hoisted(() => ({ aurora: null as unknown, pca: null as unknown, kpForecast: { points: [] } as unknown }))
vi.mock('../api', () => ({
  getAurora: vi.fn(async () => feeds.aurora),
  getPca: vi.fn(async () => feeds.pca),
  getSatellites: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  getKpForecast: vi.fn(async () => feeds.kpForecast),
  // The 2-D map's own feeds, for the test that unticks on the map and then opens the globe.
  getDeclination: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
  getArrivalOutlook: vi.fn(async () => ({ watch: null, model: null, l1: null, rationale: null })),
}))
vi.mock('three/examples/jsm/postprocessing/UnrealBloomPass.js', () => ({
  UnrealBloomPass: class {
    setSize() {}
    dispose() {}
  },
}))

vi.mock('react-globe.gl', async () => {
  const THREE = await import('three')
  const scene = new THREE.Scene()
  const fake = {
    lights: () => [],
    // Invertible: x = longitude, y = latitude, z = altitude — where a marker went, read back.
    getCoords: (lat: number, lng: number, alt = 0) => ({ x: lng, y: lat, z: alt }),
    scene: () => scene,
    postProcessingComposer: () => ({ addPass() {}, passes: [] }),
    controls: () => controls,
    // What globeWebgl.tsx hands back when the globe goes (globeWebgl.test.tsx tests that part).
    renderer: () => renderer,
    // globe.gl's getter: where the camera is (its setter form is the same call with arguments).
    pointOfView: () => ({ lat: 0, lng: 0, altitude: 2.2 }),
    paused: false,
    frames: 0,
    pauseAnimation: () => {
      fake.paused = true
    },
    resumeAnimation: () => {
      fake.paused = false
      fake.frames++
    },
  }
  const controls = Object.assign(new THREE.EventDispatcher<Record<string, object>>(), {
    autoRotate: false,
    autoRotateSpeed: 0,
    connect() {},
    disconnect() {},
  })
  const renderer = {
    domElement: document.createElement('canvas'),
    getContext: () => ({ isContextLost: () => false }),
    dispose() {},
    forceContextLoss() {},
  }
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    useImperativeHandle(ref, () => fake, [])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  })
  return { default: Globe, __fake: fake }
})

import Globe3D from './Globe3D'
import { MapView } from './MapView'
import * as ReactGlobe from 'react-globe.gl'
import * as THREE from 'three'
import { t } from '../i18n'
import { kpLabel } from '../capsNow'
import { loadGlobeLayers, saveGlobeLayers } from '../features/globeLayers'
import type { AuroraOval, PcaView } from '../types'

type FakeGlobe = { paused: boolean; frames: number; scene: () => THREE_NS.Scene }
const fake = (ReactGlobe as unknown as { __fake: FakeGlobe }).__fake

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  fake.paused = false
  fake.frames = 0
  feeds.aurora = null
  feeds.pca = null
  feeds.kpForecast = { points: [] }
  localStorage.clear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(
    () =>
      new Proxy({ getExtension: () => null } as Record<string | symbol, unknown>, {
        get: (t, k) => (k in t ? t[k] : () => ({ addColorStop() {} })),
      }) as unknown as RenderingContext,
  )
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
  const s = fake.scene()
  for (const o of [...s.children]) s.remove(o)
})

const NOW = Date.UTC(2026, 9, 31, 1, 30)
const NOW_S = NOW / 1000
const snap = (kp: number) =>
  ({
    source: 'live',
    asOf: NOW_S,
    spots: [],
    openings: [],
    dxpeditions: { workableNow: [] },
    spaceWx: { sfi: 150, kp, aIndex: 5, xrayClass: 'B2.0', flare: false, xrayLong: 2e-7, fetched: true, sfiAt: NOW_S - 3600, kpAt: NOW_S - 120, xrayAt: NOW_S - 60, aDay: NOW_S - 86400 },
  }) as unknown as PropagationSnapshot

async function mount(kp: number) {
  vi.useFakeTimers({ now: NOW })
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<Globe3D myGrid="EN52" prop={snap(kp)} selectedCall={null} onSelectCall={() => {}} stations={[]} xrayLong={2e-7} />)
  })
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10)
  })
  return r
}
const group = (name: string) => fake.scene().children.find((o) => o.name === name) as THREE_NS.Group | undefined
/** The aurora point cloud's drawn points: 0 when it is hidden or absent. */
const auroraDots = () => {
  const c = fake.scene().children.find((o) => o.name === 'cloud:aurora') as THREE_NS.Points | undefined
  return c && c.visible ? (c.geometry.getAttribute('position')?.count ?? 0) : 0
}
const dashed = (g: THREE_NS.Group) => (g.children[0] as THREE_NS.Line).material instanceof THREE.LineDashedMaterial

const ring = (lat: number): [number, number][] => [[-170, lat], [-90, lat], [0, lat], [90, lat], [170, lat]]
const LATS = Array.from({ length: 24 }, () => 60)
const OVAL: AuroraOval = {
  points: [{ lat: 66, lon: -100, prob: 50 }],
  observedAt: NOW_S - 20 * 60,
  forecastAt: NOW_S + 40 * 60,
  edge: { north: LATS, south: LATS.map((l) => -l) },
}
const PCA: PcaView = {
  j10: 150,
  a30Day: 1.5,
  a30Night: 0.8,
  cutoffDeg: 60,
  points: [{ lat: 72, lon: -100, db30: 1.4 }],
  protonsAt: NOW_S - 300,
  sScale: 2,
  sepRecoveryH: 24.235,
  edge: [ring(62), ring(-62)],
}

describe('the aurora oval on the 3-D globe', () => {
  it('a storm turns the layer on and draws OVATION’s own edge, solid, with the chip saying why', async () => {
    feeds.aurora = OVAL
    const r = await mount(6)
    const g = group('auroraEdge')
    expect(g, 'the edge line group').toBeDefined()
    expect(auroraDots(), 'CONTROL: the current OVATION’s dots are drawn').toBeGreaterThan(0)
    expect(g!.children.length).toBe(2)
    expect(dashed(g!)).toBe(false)
    expect(r.container.querySelector('.wx-chip-stack .aurora-chip')!.textContent).toContain(
      t('map.aurora.chip.storm', { why: kpLabel({ kp: 6, source: 'estimated', at: NOW_S - 120 }) }),
    )
  })

  it('a stale OVATION draws no oval line and no dots, even in a storm, and the chip says it is out of date', async () => {
    feeds.aurora = { ...OVAL, forecastAt: NOW_S - 3600 }
    const r = await mount(6)
    expect(group('auroraEdge')).toBeUndefined()
    expect(auroraDots(), 'no aurora point cloud drawn').toBe(0)
    expect(r.container.querySelector('.aurora-chip')!.textContent).toContain(t('map.aurora.chip.stale', { time: '00:30Z' }))
  })

  it('CONTROL — no storm and the layer off: no edge, no chip', async () => {
    feeds.aurora = OVAL
    const r = await mount(3)
    expect(group('auroraEdge')).toBeUndefined()
    expect(r.container.querySelector('.aurora-chip')).toBeNull()
  })
})

describe('an undated oval on the 3-D globe', () => {
  it('fails closed: no dots and no line, even in a storm, and the chip says its time is unknown', async () => {
    feeds.aurora = { points: OVAL.points, observedAt: null, forecastAt: null }
    const r = await mount(6)
    expect(auroraDots()).toBe(0)
    expect(group('auroraEdge')).toBeUndefined()
    expect(r.container.querySelector('.aurora-chip')!.textContent).toContain(t('map.aurora.chip.undated'))
  })
})

describe('the polar cap on the 3-D globe', () => {
  it('draws the approximate edge dashed at a measured Kp, with the cap chip', async () => {
    feeds.pca = PCA
    const r = await mount(3)
    const g = group('capEdge')
    expect(g).toBeDefined()
    expect(dashed(g!)).toBe(true)
    expect(r.container.querySelector('.wx-chip-stack .pca-chip')!.textContent).toContain(t('map.pca.head', { scale: 2, db: '1.4' }))
  })

  it('CONTROL — without a measured Kp there is no edge line', async () => {
    feeds.pca = { ...PCA, cutoffDeg: null, edge: null }
    const r = await mount(3)
    expect(group('capEdge')).toBeUndefined()
    expect(r.container.querySelector('.pca-chip')).not.toBeNull()
  })
})

describe('round 3: the storm rule and the untick on the 3-D globe', () => {
  it('NOAA’s measured Kp 6 (a period that ended 90 min ago) beside an estimate of 3 turns the layer on, said as measured', async () => {
    const start = NOW_S - 90 * 60 - 3 * 3600
    feeds.kpForecast = { points: [{ timeUnix: start, kp: 6, kind: 'observed', noaaScale: null }] }
    feeds.aurora = OVAL
    const r = await mount(3)
    expect(group('auroraEdge'), 'the layer is on').toBeDefined()
    expect(r.container.querySelector('.aurora-chip')!.textContent).toContain(
      t('map.aurora.chip.storm', { why: kpLabel({ kp: 6, source: 'measured', at: start, until: start + 3 * 3600 }) }),
    )
  })

  it('an operator who had the aurora on and unticks it during a storm gets it off in one click, and it stays off', async () => {
    saveGlobeLayers({ ...loadGlobeLayers(true), aurora: true })
    feeds.aurora = OVAL
    const r = await mount(6)
    const box = () =>
      [...r.container.querySelectorAll('.globe3d-layers label')]
        .find((l) => l.textContent?.includes(t('globe.layer.aurora')))!
        .querySelector('input') as HTMLInputElement
    expect(box().checked).toBe(true)
    await act(async () => {
      fireEvent.click(box())
    })
    expect(box().checked).toBe(false)
    await act(async () => {
      await vi.advanceTimersByTimeAsync(11 * 60_000)
    })
    expect(box().checked).toBe(false)
    expect(group('auroraEdge')).toBeUndefined()
  })
})

describe('round 4: one untick for the whole storm, on the 3-D globe too', () => {
  const box = (c: HTMLElement) =>
    [...c.querySelectorAll('.globe3d-layers label')]
      .find((l) => l.textContent?.includes(t('globe.layer.aurora')))!
      .querySelector('input') as HTMLInputElement
  const globe = (kp: number) => <Globe3D myGrid="EN52" prop={snap(kp)} selectedCall={null} onSelectCall={() => {}} stations={[]} xrayLong={2e-7} />

  it('G4: an untick of the storm’s layer survives a remount during the same storm', async () => {
    feeds.aurora = OVAL
    const r = await mount(6)
    expect(box(r.container).checked).toBe(true)
    await act(async () => {
      fireEvent.click(box(r.container))
    })
    cleanup()
    const r2 = await mount(6)
    expect(box(r2.container).checked).toBe(false)
    expect(group('auroraEdge')).toBeUndefined()
  })

  it('an untick on the 2-D map holds when the 3-D globe opens, in the same storm', async () => {
    feeds.aurora = OVAL
    vi.useFakeTimers({ now: NOW })
    let m!: ReturnType<typeof render>
    await act(async () => {
      m = render(
        <MapView myGrid="EN52" theme={'dark' as never} stations={[]} prop={snap(6)} selectedCall={null} onSelectCall={() => {}} needByCall={new Map()} intent="dx" projection="world" />,
      )
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10)
    })
    const mapBox = [...m.container.querySelectorAll('.map-layer')]
      .find((r) => r.textContent?.includes(t('map.layer.aurora.label')))!
      .querySelector('input') as HTMLInputElement
    expect(mapBox.checked, 'CONTROL: the storm had it on').toBe(true)
    await act(async () => {
      fireEvent.click(mapBox)
    })
    cleanup()
    const r = await mount(6)
    expect(box(r.container).checked).toBe(false)
  })

  it('an untick on a quiet day does not stop the next storm turning the layer on', async () => {
    saveGlobeLayers({ ...loadGlobeLayers(true), aurora: true })
    feeds.aurora = OVAL
    const r = await mount(3)
    await act(async () => {
      fireEvent.click(box(r.container))
    })
    expect(box(r.container).checked).toBe(false)
    await act(async () => {
      r.rerender(globe(6))
    })
    expect(box(r.container).checked, 'the storm turns it on').toBe(true)
  })

  it('measured Kp 4 beside an estimate of 6.3 turns it on, said as the estimate', async () => {
    feeds.kpForecast = { points: [{ timeUnix: NOW_S - 90 * 60 - 3 * 3600, kp: 4, kind: 'observed', noaaScale: null }] }
    feeds.aurora = OVAL
    const r = await mount(6.3)
    expect(group('auroraEdge')).toBeDefined()
    expect(r.container.querySelector('.aurora-chip')!.textContent).toContain(
      t('map.aurora.chip.storm', { why: kpLabel({ kp: 6.3, source: 'estimated', at: NOW_S - 120 }) }),
    )
  })
})

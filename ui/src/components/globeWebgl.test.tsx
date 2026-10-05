// @vitest-environment jsdom
//
// A 3-D GLOBE HANDS ITS WEBGL CONTEXT BACK WHEN IT GOES, AND SURVIVES A LOSS WHILE IT IS SHOWN (operator,
// 2026-10-04: the 3-D globe's missing context-loss handling). Both globes, Globe3D and QsoGlobe, run
// through the same checks (globeWebgl.tsx).
//
// jsdom has no WebGL, so `react-globe.gl` is a stub, but it keeps REAL the two things the leak was made
// of. One is three.js's own OrbitControls on the stub's canvas, built once the canvas is in the document
// as three-render-objects builds it: its keydown listener on the canvas's root node is what kept every
// closed globe alive in Chrome. The other is globe.gl's teardown order: react-kapsule runs its destructor
// in a passive cleanup, after React has detached the canvas, and the destructor disposes the controls
// there. The renderer is a fake that logs what is done to it, and its context can be lost and restored
// the way the browser does it, by events on the canvas.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, type ReactElement } from 'react'
import { act, cleanup, render, screen } from '@testing-library/react'
import type { PropagationSnapshot } from '../types'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getAurora: vi.fn(async () => []),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  askLog: vi.fn(async () => []),
}))
vi.mock('three/examples/jsm/postprocessing/UnrealBloomPass.js', () => ({
  UnrealBloomPass: class {
    setSize() {}
    dispose() {}
  },
}))

type Controls = typeof import('three/examples/jsm/controls/OrbitControls.js').OrbitControls
/** One globe the stub mounted: its scene's box and canvas, its controls, its loop, and its context. */
interface Instance {
  n: number
  container: HTMLDivElement
  canvas: HTMLCanvasElement
  controls: InstanceType<Controls> | null
  running: boolean
  /** The browser losing the context (its event is cancelable; preventDefault asks for it back). */
  lose(): Event
  restore(): void
  api: { camera(): import('three').PerspectiveCamera; renderer(): { dispose(): void }; pauseAnimation(): void } & Record<string, unknown>
}
/** Everything done to the globes, in order, across every instance. */
const log: string[] = []
/** The instances the stub has mounted, oldest first. */
const instances: Instance[] = []

function makeInstance(THREE: typeof import('three')): Instance {
  const n = instances.length
  const container = document.createElement('div')
  const canvas = document.createElement('canvas')
  container.appendChild(canvas)
  let lost = false
  const renderer = {
    domElement: canvas,
    capabilities: { maxTextureSize: 4096, getMaxAnisotropy: () => 1 },
    getContext: () => ({ isContextLost: () => lost }),
    dispose: () => log.push(`#${n} renderer.dispose`),
    forceContextLoss: () => {
      log.push(`#${n} renderer.forceContextLoss`)
      lost = true
    },
  }
  const camera = new THREE.PerspectiveCamera()
  const scene = new THREE.Scene()
  const self: Instance = {
    n,
    container,
    canvas,
    controls: null,
    running: true,
    lose() {
      lost = true
      const e = new Event('webglcontextlost', { cancelable: true })
      canvas.dispatchEvent(e)
      return e
    },
    restore() {
      lost = false
      canvas.dispatchEvent(new Event('webglcontextrestored'))
    },
    api: {
      renderer: () => renderer,
      controls: () => self.controls,
      scene: () => scene,
      camera: () => camera,
      lights: () => [],
      getCoords: () => ({ x: 0, y: 0, z: 0 }),
      postProcessingComposer: () => ({ addPass() {}, passes: [] }),
      pointOfView: () => ({ lat: 0, lng: 0, altitude: 2.2 }),
      pauseAnimation: () => {
        log.push(`#${n} pause`)
        self.running = false
      },
      resumeAnimation: () => {
        log.push(`#${n} resume`)
        self.running = true
      },
    },
  }
  instances.push(self)
  return self
}

vi.mock('react-globe.gl', async () => {
  const THREE = await import('three')
  const { OrbitControls } = await import('three/examples/jsm/controls/OrbitControls.js')
  const Globe = forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    const host = useRef<HTMLDivElement>(null)
    const inst = useMemo(() => makeInstance(THREE), [])
    useImperativeHandle(ref, () => inst.api, [inst])
    // react-kapsule mounts the kapsule in a layout effect, and three-render-objects builds the controls
    // once the canvas is in the document.
    useLayoutEffect(() => {
      host.current!.appendChild(inst.container)
      inst.controls = new OrbitControls(inst.api.camera(), inst.canvas)
    }, [inst])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    // globe.gl's destructor: a passive cleanup, after React has detached the canvas.
    useEffect(
      () => () => {
        log.push(`#${inst.n} destructor`)
        inst.api.pauseAnimation()
        inst.controls?.dispose()
        inst.api.renderer().dispose()
      },
      [inst],
    )
    return <div data-testid="globe" ref={host} />
  })
  return { default: Globe }
})

import Globe3D from './Globe3D'
import QsoGlobe from './QsoGlobe'
import { RESTORE_WAIT_MS } from './globeWebgl'
import { t } from '../i18n'

const QUIET = { source: 'live', asOf: 1, spots: [], openings: [], dxpeditions: { workableNow: [] } } as unknown as PropagationSnapshot
/** Each globe, and whether its loop sleeps while nothing changes (Globe3D's does; QsoGlobe's spins). */
const GLOBES: Array<[string, () => ReactElement, boolean]> = [
  ['Globe3D', () => <Globe3D myGrid="EN52" prop={QUIET} selectedCall={null} onSelectCall={() => {}} stations={[]} />, true],
  ['QsoGlobe', () => <QsoGlobe logTick={1} />, false],
]

/** The keydown listeners on the document right now (OrbitControls' is a capturing one). */
let keydownOnDocument = new Set<EventListenerOrEventListenerObject>()
/** What the 3-D probe did with its context: `webglOk()` asks for one before the globe mounts. */
const probeLose = vi.fn()

beforeEach(() => {
  log.length = 0
  instances.length = 0
  probeLose.mockClear()
  localStorage.clear()
  keydownOnDocument = new Set()
  const add = document.addEventListener.bind(document)
  const remove = document.removeEventListener.bind(document)
  vi.spyOn(document, 'addEventListener').mockImplementation((type, l, o) => {
    if (type === 'keydown' && l) keydownOnDocument.add(l)
    add(type, l, o)
  })
  vi.spyOn(document, 'removeEventListener').mockImplementation((type, l, o) => {
    if (type === 'keydown' && l) keydownOnDocument.delete(l)
    remove(type, l, o)
  })
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  ;(globalThis as unknown as { IntersectionObserver: unknown }).IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  // A 2-D context for the sprite canvases (any method a no-op), and the 3-D probe's WebGL one.
  const ctx2d = new Proxy({} as Record<string | symbol, unknown>, {
    get: (o, k) => (k in o ? o[k] : () => ({ addColorStop() {} })),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((type: string) =>
    /webgl/.test(type)
      ? { getExtension: (name: string) => (name === 'WEBGL_lose_context' ? { loseContext: probeLose } : null) }
      : ctx2d) as never)
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

async function show(make: () => ReactElement) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(make())
  })
  expect(instances.length, 'the globe mounted').toBeGreaterThan(0)
  return r
}
const shown = () => instances[instances.length - 1]
/** The paused globe's word and Reload (globeWebgl.tsx's GlobePaused), or null. */
const pausedNote = () => document.querySelector('.globe-paused')

describe.each(GLOBES)('%s and its WebGL context', (_name, make, sleeps) => {
  it("takes its controls' keydown listener off the document when it goes", async () => {
    const r = await show(make)
    expect(keydownOnDocument.size, 'while it is shown').toBe(1)
    await act(async () => r.unmount())
    expect(keydownOnDocument.size, 'after it went').toBe(0)
  })

  it('hands its context back when it goes: the loop stopped, the renderer disposed, then the context lost', async () => {
    const r = await show(make)
    const g = shown()
    await act(async () => r.unmount())
    const at = (what: string) => log.indexOf(`#${g.n} ${what}`)
    expect(log.filter((l) => l === `#${g.n} renderer.forceContextLoss`), log.join(' · ')).toHaveLength(1)
    expect(at('pause'), log.join(' · ')).toBeGreaterThan(-1)
    expect(at('pause')).toBeLessThan(at('renderer.forceContextLoss'))
    expect(at('renderer.dispose')).toBeLessThan(at('renderer.forceContextLoss'))
    // Nothing draws into it afterwards.
    expect(log.slice(at('renderer.forceContextLoss')).filter((l) => l === `#${g.n} resume`), log.join(' · ')).toEqual([])
    expect(g.running).toBe(false)
  })

  it("lets three.js's shared sprite geometry go of the renderers that drew it when it goes", async () => {
    // Every Sprite shares one geometry, and a renderer that drew a sprite stays registered on it (a
    // dispose listener holding the renderer, its canvas and the textures it drew) until it is disposed:
    // in Chrome each closed Connect globe kept its 32 MB basemap canvas that way.
    const { Sprite } = await import('three')
    const shared = new Sprite().geometry
    const registered = vi.fn()
    shared.addEventListener('dispose', registered) // what a renderer does when it draws a sprite
    const r = await show(make)
    expect(registered).not.toHaveBeenCalled()
    await act(async () => r.unmount())
    expect(registered).toHaveBeenCalledTimes(1)
    shared.removeEventListener('dispose', registered)
  })

  it('asks for a lost context back and hides the scene while it is away', async () => {
    await show(make)
    const g = shown()
    let e!: Event
    await act(async () => {
      e = g.lose()
    })
    expect(e.defaultPrevented, 'preventDefault() on webglcontextlost').toBe(true)
    expect(g.container.style.visibility).toBe('hidden')
    expect(pausedNote()).toBeNull()
  })

  it(`says the 3D view is paused, with a Reload, when the context is not back within ${RESTORE_WAIT_MS / 1000} s, and not before`, async () => {
    await show(make)
    const g = shown()
    vi.useFakeTimers()
    await act(async () => {
      g.lose()
    })
    await act(async () => {
      vi.advanceTimersByTime(RESTORE_WAIT_MS - 100)
    })
    expect(pausedNote()).toBeNull()
    await act(async () => {
      vi.advanceTimersByTime(100)
    })
    expect(pausedNote()?.getAttribute('role')).toBe('status')
    expect(pausedNote()?.textContent).toContain(t('globe.paused.text'))
    expect(screen.getByRole('button', { name: t('globe.paused.reload') })).toBeTruthy()
  })

  it('a context that comes back clears the pause, shows the scene and draws it again', async () => {
    vi.useFakeTimers()
    await show(make)
    const g = shown()
    await act(async () => {
      vi.advanceTimersByTime(2000)
    })
    // Globe3D's loop has gone to sleep by now (nothing changed), so only a redraw can wake it;
    // QsoGlobe's spins on, and draws a restored context on its own.
    expect(g.running, 'the loop before the loss').toBe(!sleeps)
    await act(async () => {
      g.lose()
      vi.advanceTimersByTime(RESTORE_WAIT_MS)
    })
    expect(pausedNote()).not.toBeNull()
    const before = log.length
    await act(async () => {
      g.restore()
    })
    expect(pausedNote()).toBeNull()
    expect(g.container.style.visibility).toBe('')
    expect(g.running, log.slice(before).join(' · ')).toBe(true)
  })

  it('Reload mounts the globe afresh, and the lost one is let go', async () => {
    await show(make)
    const g = shown()
    vi.useFakeTimers()
    await act(async () => {
      g.lose()
      vi.advanceTimersByTime(RESTORE_WAIT_MS)
    })
    await act(async () => {
      screen.getByRole('button', { name: t('globe.paused.reload') }).click()
    })
    expect(instances.length, 'a new globe').toBe(g.n + 2)
    expect(log, 'the old one destroyed').toContain(`#${g.n} destructor`)
    expect(keydownOnDocument.size, 'and only the new one listening').toBe(1)
    expect(pausedNote()).toBeNull()
    expect(shown().container.isConnected).toBe(true)
  })
})

describe('the 3-D probe', () => {
  it('hands the context it asked for straight back', async () => {
    await show(GLOBES[0][1])
    expect(probeLose).toHaveBeenCalledTimes(1)
  })
})

// @vitest-environment jsdom
//
// THE 2-D MAP REDRAWS WHEN THE PICTURE CHANGES, AND NEVER RE-PROJECTS THE WORLD FOR A DOT.
//
// App hands the map a NEW `stations` array on every 300 ms snapshot, whether or not anything was
// decoded. `placed`, `rxLines` and the draw effect were keyed on that identity, so the whole canvas
// was cleared and redrawn ~3×/s for identical content — and every redraw re-projected every country
// outline, state border and graticule line (15.6% main thread in headless Chrome with every layer
// OFF). Two properties, each with the control that proves its counter is live:
//   1. identical content → no redraw at all;
//   2. a redraw that is not a view change reuses the cached base map (land, coast, borders, grid).
//
// jsdom has no 2-D canvas, so each canvas gets a recording context (no pixels); MapView is the REAL
// component. The base geography's two painters (basemap.ts `paintProjected`, `paintEquirect`) are
// wrapped with a counter: one call is one re-projection of the world.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import type { MapSpot, PropagationSnapshot, Station } from '../types'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))
const basemapCalls = { n: 0 }
vi.mock('../basemap', async (importOriginal) => {
  const real = await importOriginal<typeof import('../basemap')>()
  return {
    ...real,
    paintProjected: (...a: Parameters<typeof real.paintProjected>) => {
      basemapCalls.n++
      return real.paintProjected(...a)
    },
    paintEquirect: (...a: Parameters<typeof real.paintEquirect>) => {
      basemapCalls.n++
      return real.paintEquirect(...a)
    },
  }
})

import { MapView } from './MapView'
import { PALETTE_EVENT } from '../usePaletteRoles'
import { MAP_TOKENS, SKINS, STANDARD_MAP } from '../features/skins'

// THE BUDGET (2026-10-09). The slowest case here, "a new but identical stations array (the 300 ms…", takes 0.27 s and
// 0.28 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one core,
// past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Per-canvas call counts, by context method name. */
const calls = new WeakMap<HTMLCanvasElement, Map<string, number>>()
/** Every fill and stroke colour any canvas was handed, in order: what the map painted with. */
const inks: string[] = []
function recordingContext(canvas: HTMLCanvasElement) {
  const counts = new Map<string, number>()
  calls.set(canvas, counts)
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (..._args: unknown[]) => {
        counts.set(String(k), (counts.get(String(k)) ?? 0) + 1)
        if (k === 'measureText') return { width: 0 }
        // A gradient's stops are colours the map paints with too (the globe's sea).
        return { addColorStop: (_at: number, colour: unknown) => void (typeof colour === 'string' && inks.push(colour)) }
      }
    },
    set: (t, k, v) => {
      t[k] = v
      if ((k === 'fillStyle' || k === 'strokeStyle') && typeof v === 'string') inks.push(v)
      return true
    },
  })
}

beforeEach(() => {
  localStorage.clear()
  basemapCalls.n = 0
  inks.length = 0
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  // jsdom reports every document as hidden, and both maps skip their 1 s ticks for a hidden tab.
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(400)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

const station = (call: string, grid: string): Station =>
  ({ call, grid, snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false }) as unknown as Station
const ROSTER = [station('K1AB', 'FN42'), station('W6XY', 'CM87'), station('JA1ZZ', 'PM95')]
const clone = (s: Station[]) => s.map((x) => ({ ...x }))
const NEEDS = new Map()

function props(stations: Station[], prop: PropagationSnapshot | null = null) {
  return {
    myGrid: 'EN52',
    theme: 'dark' as never,
    stations,
    prop,
    selectedCall: null,
    onSelectCall: () => {},
    needByCall: NEEDS,
    intent: 'dx' as const,
  }
}

/** Full redraws of the visible map canvas so far (each draw clears it exactly once). */
function redraws(container: HTMLElement): number {
  const canvas = container.querySelector('.map-canvas-wrap > canvas') as HTMLCanvasElement
  return calls.get(canvas)?.get('clearRect') ?? 0
}

describe('MapView redraws on content, not on snapshot identity', () => {
  it('a new but identical stations array (the 300 ms snapshot) does not redraw', async () => {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER)} />)
    })
    const first = redraws(r.container)
    expect(first, 'CONTROL: the map really drew').toBeGreaterThan(0)
    for (let i = 0; i < 3; i++) {
      await act(async () => {
        r.rerender(<MapView {...props(clone(ROSTER))} />)
      })
    }
    expect(redraws(r.container)).toBe(first)
  })

  it('POSITIVE CONTROL — a decode that moves DOES redraw', async () => {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER)} />)
    })
    const first = redraws(r.container)
    await act(async () => {
      r.rerender(<MapView {...props([...clone(ROSTER).slice(0, 2), station('JA1ZZ', 'QM05')])} />)
    })
    expect(redraws(r.container)).toBeGreaterThan(first)
  })
})

describe('MapView caches the base map across redraws', () => {
  it('a content redraw does not re-project the world', async () => {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER)} />)
    })
    const projected = basemapCalls.n
    expect(projected, 'CONTROL: the base map was projected at least once').toBeGreaterThan(0)
    const first = redraws(r.container)
    await act(async () => {
      r.rerender(<MapView {...props([...clone(ROSTER).slice(0, 2), station('JA1ZZ', 'QM05')])} />)
    })
    expect(redraws(r.container), 'CONTROL: that really was a redraw').toBeGreaterThan(first)
    expect(basemapCalls.n).toBe(projected)
  })

  it('POSITIVE CONTROL — a projection change DOES re-project the base map', async () => {
    await act(async () => {
      render(<MapView {...props(ROSTER)} />)
    })
    const projected = basemapCalls.n
    const other = screen.getByRole('button', { name: 'Beam' })
    await act(async () => fireEvent.click(other))
    expect(basemapCalls.n).toBeGreaterThan(projected)
  })

  it('the 1 s heat pulse still repaints, off the cached base', async () => {
    vi.useFakeTimers()
    const spot = { call: 'DL1AA', lat: 50, lon: 8, band: '20m', heardMe: false, ageSecs: 60, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'X' } as unknown as MapSpot
    const prop = {
      source: 'live',
      asOf: 1,
      spots: [spot],
      openings: [{ band: '20m', mode: 'F2', octant: 'E', bearingDeg: 60, maxKm: 7000, probability: 0.7 }],
      dxpeditions: { workableNow: [] },
      spaceWx: { sfi: 150, kp: 2, aIndex: 5, xrayClass: 'B2', flare: false, xrayLong: 1e-7 },
      insights: [],
    } as unknown as PropagationSnapshot
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER, prop)} />)
    })
    const first = redraws(r.container)
    const projected = basemapCalls.n
    // One act per tick: a single 3 s act would batch three ticks into one commit.
    for (let i = 0; i < 3; i++) await act(async () => void vi.advanceTimersByTime(1_000))
    expect(redraws(r.container), 'the pulse must keep repainting').toBeGreaterThanOrEqual(first + 3)
    expect(basemapCalls.n, 'a pulse frame must not re-project the world').toBe(projected)
  })
})

describe('MapView redraws when a colour role changes (Settings ▸ Appearance ▸ Colours)', () => {
  // The map paints the accent and the SNR greens/ambers from a per-draw token memo, and a CSS
  // custom property changing under a canvas says nothing to React. Keyed on the theme alone, a
  // new accent or OK green would sit unpainted until something unrelated redrew the map. The
  // attribute is set the way usePaletteRoles sets it, and the event is the one it fires.
  afterEach(() => document.documentElement.removeAttribute('data-accent'))

  it('a new accent redraws the map, so every token is read again', async () => {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER)} />)
    })
    const first = redraws(r.container)
    expect(first, 'CONTROL: the map really drew').toBeGreaterThan(0)
    await act(async () => {
      document.documentElement.setAttribute('data-accent', 'violet')
      window.dispatchEvent(new Event(PALETTE_EVENT))
    })
    expect(redraws(r.container)).toBe(first + 1)
  })

  it('Night coming on redraws the map, so its labels and marks take the night inks', async () => {
    // Night (Settings ▸ Appearance ▸ Workspace) retunes --text, --bg and the accent the map paints
    // with, and changes neither the theme nor a colour role. Set the way useNight sets it.
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER)} />)
    })
    const first = redraws(r.container)
    try {
      await act(async () => {
        document.documentElement.setAttribute('data-night', '1')
        window.dispatchEvent(new Event(PALETTE_EVENT))
      })
      expect(redraws(r.container)).toBe(first + 1)
    } finally {
      document.documentElement.removeAttribute('data-night')
    }
  })

  it('CONTROL — the same event with no colour change does not redraw', async () => {
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<MapView {...props(ROSTER)} />)
    })
    const first = redraws(r.container)
    await act(async () => {
      window.dispatchEvent(new Event(PALETTE_EVENT))
    })
    expect(redraws(r.container)).toBe(first)
  })
})

describe('the base map is the theme’s (Settings ▸ Appearance ▸ Theme)', () => {
  // The basemap's colours are tokens (styles.css MAP BASEMAP); a dark built-in theme brings its
  // own (features/skins.ts). jsdom loads no sheet, so a token is set the way the sheet would
  // resolve it, on <html>, and the theme attribute and event are useSkin's.
  const root = document.documentElement
  afterEach(() => {
    for (const t of MAP_TOKENS) root.style.removeProperty(t)
    root.removeAttribute('data-skin')
  })

  it('with no sheet it paints the standard basemap', async () => {
    await act(async () => {
      render(<MapView {...props(ROSTER)} />)
    })
    // The map opens on the globe: its sea is a gradient of three of them, its land the globe's.
    for (const t of ['--map-ocean', '--map-ocean-lit', '--map-ocean-deep', '--map-land-globe', '--map-rim'] as const) {
      expect(inks, t).toContain(STANDARD_MAP[t])
    }
  })

  it('a built-in theme re-bakes the cached base map in its own land and sea', async () => {
    await act(async () => {
      render(<MapView {...props(ROSTER)} />)
    })
    const projected = basemapCalls.n
    const lagoon = SKINS.find((x) => x.id === 'lagoon')!.map!
    expect(inks, 'CONTROL: the standard sea is not already Lagoon’s').not.toContain(lagoon['--map-ocean'])
    await act(async () => {
      for (const [t, v] of Object.entries(lagoon)) root.style.setProperty(t, v)
      root.setAttribute('data-skin', 'lagoon')
      window.dispatchEvent(new Event(PALETTE_EVENT))
    })
    expect(basemapCalls.n, 'the base map was not re-baked').toBeGreaterThan(projected)
    for (const t of ['--map-ocean', '--map-ocean-lit', '--map-ocean-deep', '--map-land-globe', '--map-rim'] as const) {
      expect(inks, t).toContain(lagoon[t])
    }
  })
})

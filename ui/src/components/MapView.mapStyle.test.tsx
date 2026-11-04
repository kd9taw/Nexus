// @vitest-environment jsdom
//
// THE MAP STYLE ON THE REAL MAP (Map layers ▸ Map style; features/mapStyle, #444).
//
// The 2-D map is rendered whole, its canvases given a context that records every colour it paints
// with (jsdom has no 2-D canvas; the globe's sea is a gradient, so its stops are recorded too). jsdom
// loads no sheet, so a theme's tokens are set on <html> the way the sheet resolves them, and the
// theme attribute and event are useSkin's. What the map holds:
//   1. the row is in the Layers panel of Connect's map and of the APRS map, Atlas the default;
//   2. Dark re-bakes the base map in DARK_MAP, and the pick is this window's record, shared by both
//      maps and read again on the next open;
//   3. a stored value that is not a style opens on the theme's map;
//   4. under a dark built-in theme no pick paints the theme's own map, and a pick overrides it;
//   5. a Dark map paints its markers in the dark palette (a display well's inks), an Atlas map in
//      the page's.
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Station } from '../types'

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
}))

import { MapView } from './MapView'
import { PALETTE_EVENT } from '../usePaletteRoles'
import { MAP_TOKENS, SKINS, STANDARD_MAP } from '../features/skins'
import { DARK_MAP } from '../features/mapStyle'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { MODES, chainOf, parseRules, winnerAt } from '../cssCascade'

vi.setConfig({ testTimeout: 15_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** Every fill and stroke colour any canvas was handed, in order. */
let inks: string[] = []
function recordingContext(canvas: HTMLCanvasElement) {
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return () => {
        if (k === 'measureText') return { width: 0 }
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

const root = document.documentElement
let sheet: HTMLStyleElement | null = null
beforeEach(() => {
  localStorage.clear()
  inks = []
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  ;(globalThis as unknown as { Path2D: unknown }).Path2D = class {
    moveTo() {}
    lineTo() {}
    closePath() {}
    arc() {}
    rect() {}
  }
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1000)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(800)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  for (const t of MAP_TOKENS) root.style.removeProperty(t)
  root.style.removeProperty('--snr-strong')
  root.removeAttribute('data-skin')
  sheet?.remove()
  sheet = null
})

const station = (call: string, grid: string): Station =>
  ({ call, grid, snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false }) as unknown as Station
const ROSTER = [station('K1AB', 'FN42'), station('W6XY', 'CM87')]
const connect = { myGrid: 'EN52', theme: 'light' as never, stations: ROSTER, prop: null, selectedCall: null, onSelectCall: () => {}, needByCall: new Map(), intent: 'dx' as const }

const pick = () => screen.getByRole('combobox', { name: 'Map style' }) as HTMLSelectElement
const options = () => [...pick().options].map((o) => o.value)
/** The globe's sea and land, as the base map paints them in `p`. */
const SEA_AND_LAND = ['--map-ocean', '--map-ocean-lit', '--map-ocean-deep', '--map-land-globe', '--map-rim'] as const
const paintsIn = (p: Readonly<Record<string, string>>) => SEA_AND_LAND.every((t) => inks.includes(p[t]))

async function open(props: Record<string, unknown> = {}) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<MapView {...connect} {...props} />)
  })
  return r
}
async function choose(value: string) {
  inks = []
  await act(async () => {
    fireEvent.change(pick(), { target: { value } })
  })
}

describe('the Map style row', () => {
  it('is in Connect’s Layers panel, on Atlas, with Atlas and Dark', async () => {
    await open()
    const panel = screen.getByRole('combobox', { name: 'Map style' }).closest('.map-layers') as HTMLElement
    expect(panel).not.toBeNull()
    expect(within(panel).getByRole('combobox', { name: 'Map style' })).toBe(pick())
    expect(options()).toEqual(['atlas', 'dark'])
    expect(pick().value).toBe('atlas')
    expect(paintsIn(STANDARD_MAP)).toBe(true)
  })

  it('Dark re-bakes the base map in the Dark palette, and is remembered for this window', async () => {
    const r = await open()
    expect(paintsIn(DARK_MAP), 'CONTROL: Atlas is not already Dark').toBe(false)
    await choose('dark')
    expect(paintsIn(DARK_MAP)).toBe(true)
    expect(inks).not.toContain(STANDARD_MAP['--map-ocean'])
    expect(localStorage.getItem('nexus.map.style')).toBe('dark')
    r.unmount()
    inks = []
    await open()
    expect(pick().value).toBe('dark')
    expect(paintsIn(DARK_MAP)).toBe(true)
  })

  it('Atlas again is no pick, and paints the atlas', async () => {
    localStorage.setItem('nexus.map.style', 'dark')
    await open()
    await choose('atlas')
    expect(localStorage.getItem('nexus.map.style')).toBe('')
    expect(paintsIn(STANDARD_MAP)).toBe(true)
  })

  it('the APRS map has the row too, and follows a pick made on Connect’s map in the same window', async () => {
    await open()
    await act(async () => {
      render(<MapView {...connect} embedded={{ aprs: true }} aprs={[]} />)
    })
    const both = screen.getAllByRole('combobox', { name: 'Map style' }) as HTMLSelectElement[]
    expect(both).toHaveLength(2)
    await act(async () => {
      fireEvent.change(both[0], { target: { value: 'dark' } })
    })
    expect(both[1].value).toBe('dark')
  })

  it('its words stay on one line beside the select, in every mode (they wrapped on Amber LCD)', async () => {
    await open()
    const words = pick().closest('label')!.querySelector('.map-style-label') as HTMLElement
    expect(words?.textContent).toBe('Map style')
    const strip = (n: string) =>
      readFileSync(resolve(process.cwd(), 'src', n), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
    const rules = parseRules(strip('styles.css') + '\n' + strip('cockpit-panes.css'))
    const chain = chainOf(words)
    const faults = MODES.filter((m) => winnerAt(rules, m, chain, 'white-space')?.value !== 'nowrap')
    expect(faults).toEqual([])
  })

  it.each([['photo'], ['{"dark":true}'], ['Dark']])('a stored %j opens on the theme’s map', async (raw) => {
    localStorage.setItem('nexus.map.style', raw)
    await open()
    expect(pick().value).toBe('atlas')
    expect(paintsIn(STANDARD_MAP)).toBe(true)
    expect(paintsIn(DARK_MAP)).toBe(false)
  })
})

describe('a dark built-in theme keeps its own map until the operator picks one', () => {
  const lagoon = SKINS.find((s) => s.id === 'lagoon')!.map!
  async function wearLagoon() {
    for (const [t, v] of Object.entries(lagoon)) root.style.setProperty(t, v)
    root.setAttribute('data-skin', 'lagoon')
    window.dispatchEvent(new Event(PALETTE_EVENT))
  }

  it('no pick paints the theme’s map, and the row offers it', async () => {
    await act(wearLagoon)
    await open()
    expect(options()).toEqual(['theme', 'atlas', 'dark'])
    expect(pick().value).toBe('theme')
    expect(paintsIn(lagoon)).toBe(true)
  })

  it('a pick overrides it, either way, and the theme’s own map comes back', async () => {
    await act(wearLagoon)
    await open()
    await choose('atlas')
    expect(localStorage.getItem('nexus.map.style')).toBe('atlas')
    expect(paintsIn(STANDARD_MAP)).toBe(true)
    expect(inks).not.toContain(lagoon['--map-ocean'])
    await choose('dark')
    expect(paintsIn(DARK_MAP)).toBe(true)
    await choose('theme')
    expect(localStorage.getItem('nexus.map.style')).toBe('')
    expect(paintsIn(lagoon)).toBe(true)
  })
})

describe('a Dark map paints its markers in the dark palette', () => {
  // The page's ink (a light theme's dark green) on <html>; the well's (the dark palette's) on `.well`.
  const PAGE = '#1a6b2e'
  const WELL = '#4ade80'
  beforeEach(() => {
    root.style.setProperty('--snr-strong', PAGE)
    sheet = document.createElement('style')
    sheet.textContent = `.well { --snr-strong: ${WELL}; }`
    document.head.appendChild(sheet)
  })

  it('Atlas paints a strong station in the page’s ink', async () => {
    await open()
    expect(inks).toContain(PAGE)
    expect(inks).not.toContain(WELL)
  })

  it('Dark paints it in the well’s', async () => {
    localStorage.setItem('nexus.map.style', 'dark')
    await open()
    expect(inks).toContain(WELL)
    expect(inks).not.toContain(PAGE)
  })
})

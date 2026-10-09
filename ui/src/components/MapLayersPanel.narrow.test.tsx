// @vitest-environment jsdom
//
// AT THE 1024×768 FLOOR THE LAYERS PANEL NO LONGER COVERS THE BAND NAMES.
//
// The map is about 445 px wide there. The Layers panel (top left, 200 px) and the Conditions rail
// (right, at least 248 px) cannot both be open side by side, and Layers stacks above the rail on
// purpose (styles.css `.map-insights`), so it covered the left edge of the rail's Band conditions
// list. Hit-testing in Chrome at 1024×768: four band names under the Layers panel and none of the
// eleven the topmost element. With no fold of the operator's own on record, the Layers panel now
// starts folded on a map that narrow, and opens by itself again when the map is wide enough. The
// operator's own fold or unfold is remembered and always wins.
//
// Drives the REAL MapView (the 3-D globe shares the panel and the rule: Globe3D.render.test.tsx).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { MapView } from './MapView'
import { OVERLAYS_SIDE_BY_SIDE_PX } from './MapLayersPanel'
import { parseRules, winnerAt, type El } from '../cssCascade'
import type { PropagationSnapshot } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "starts folded, so the band names beneath it are not…", takes
// 0.28 s and 0.29 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const KEY = 'nexus.connect.layersPanel.collapsed'
const PROP = {
  advisory: { headline: '', banners: [], bands: [{ band: '20m', tier: 'Active', modeled: 'Open', score: 0.7, nHearMe: 3, nIHear: 9, bestRegion: null, confidence: 'Likely', reason: 'r' }] },
  openings: [],
  dxpeditions: { workableNow: [], active: [], upcoming: [] },
  spots: [],
  source: 'live',
  asOf: 1,
} as unknown as PropagationSnapshot

async function mount(width: number, prop: PropagationSnapshot | null = PROP) {
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(width)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(300)
  // A map with a size draws: jsdom has no 2-D context, so hand it one whose every method is a no-op.
  const ctx = new Proxy({} as Record<string | symbol, unknown>, {
    get: (o, k) => (k in o ? o[k] : () => ({ addColorStop() {} })),
    set: (o, k, v) => ((o[k] = v), true),
  })
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => ctx as unknown as RenderingContext)
  await act(async () => {
    render(
      <MapView
        myGrid="EN52"
        theme={'dark' as never}
        stations={[]}
        prop={prop}
        selectedCall={null}
        onSelectCall={() => {}}
        needByCall={new Map()}
        intent="dx"
      />,
    )
  })
}
const open = () => screen.queryByRole('complementary', { name: 'Layers' }) != null
const folded = () => screen.queryByRole('button', { name: 'Layers' }) != null

beforeEach(() => {
  localStorage.clear()
  window.history.replaceState({}, '', '/')
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('the Layers panel on a map too narrow for it and the Conditions rail side by side', () => {
  it('starts folded, so the band names beneath it are not covered', async () => {
    await mount(445)
    expect(folded(), 'the Layers panel is open over the Conditions rail at 445 px').toBe(true)
    expect(open()).toBe(false)
  })

  it('CONTROLS: a wide map, or a map with no Conditions rail, keeps it open', async () => {
    await mount(1200)
    expect(open(), 'open by default on a wide map').toBe(true)
    cleanup()
    vi.restoreAllMocks()
    await mount(445, null)
    expect(open(), 'no rail to cover, so nothing to fold for').toBe(true)
  })

  it('the operator’s own fold or unfold is remembered and wins', async () => {
    localStorage.setItem(KEY, '0')
    await mount(445)
    expect(open(), 'unfolded on record stays open on a narrow map').toBe(true)
    cleanup()
    vi.restoreAllMocks()
    localStorage.setItem(KEY, '1')
    await mount(1200)
    expect(folded(), 'folded on record stays folded on a wide map').toBe(true)
  })

  it('the width it folds below is the one the sheet draws: Layers’ place and width, a gap, the rail’s least width and margin', () => {
    const rules = parseRules(readFileSync(resolve(__dirname, '../styles.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, ''))
    const el = (classes: string[], tag = 'div'): El[] => [{ tag, classes, attrs: {} }]
    const px = (v: string | undefined, re: RegExp) => Number(re.exec(v ?? '')?.[1])
    const layersLeft = px(winnerAt(rules, 'dark', el(['map-layers'], 'aside'), 'left')?.value, /^(\d+)px$/)
    const layersWidth = px(winnerAt(rules, 'dark', el(['map-layers'], 'aside'), 'width')?.value, /^min\((\d+)px,/)
    const railWidth = px(winnerAt(rules, 'dark', el(['map-insights'], 'aside'), 'width')?.value, /^clamp\((\d+)px,/)
    const railRight = winnerAt(rules, 'dark', el(['map-insights'], 'aside'), 'right')?.value
    expect([layersLeft, layersWidth, railWidth, railRight]).toEqual([8, 200, 248, 'var(--space-3)'])
    // 8 + 200, an 8 px gap, 248 + the rail's 12 px margin (--space-3 at the standard density).
    expect(OVERLAYS_SIDE_BY_SIDE_PX).toBe(layersLeft + layersWidth + 8 + railWidth + 12)
  })
})

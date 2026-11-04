// @vitest-environment jsdom
//
// THE MAP STYLE (Map layers ▸ Map style): Atlas or Dark for the flat map and the 2-D globe, whatever
// the UI theme (#444). What this file holds:
//   1. the pick is a validated enum, CLAMPED ON LOAD: anything a store from another build could hold
//      that is not a style reads as no pick;
//   2. the precedence: an operator's pick paints its own palette; no pick paints the theme's map
//      (Atlas, or a dark built-in theme's own);
//   3. Dark IS dark, and everything drawn over it reads: the grid, range rings and coasts, the
//      greyline, the TX/RX arcs, and every marker ink the map paints with, in every UI theme, mode
//      and colour-role set. The markers are read from the dark palette (a display well) on a Dark
//      map, so the light theme's dark inks never land on dark land; the negative control proves the
//      light theme's own inks would not have read there.
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  MODES,
  SKIN_MODES,
  baseTheme,
  contrast,
  expandWith,
  luminance,
  parseHex,
  parseRules,
  rootTokensFrom,
  toRgb,
  tokensAt,
  type Mode,
  type Rgb,
} from '../cssCascade'
import { MAP_TOKENS, SKINS, STANDARD_MAP, type MapToken } from './skins'
import {
  DARK_MAP,
  MAP_STYLES,
  loadMapStyle,
  mapPaletteFor,
  mapStyleOf,
  mapStyleOptions,
  mapStylePick,
  mapStyleShown,
  saveMapStyle,
  useMapStyle,
} from './mapStyle'
import { BAND_COLOR, GETTING_OUT, GREYLINE_INK, NIGHT_INK, PATH_RX, needColor } from '../components/MapView'
import type { NeedTag } from '../types'

beforeEach(() => localStorage.clear())
afterEach(() => {
  vi.restoreAllMocks()
  window.history.replaceState({}, '', '/')
})

describe('the pick is a validated enum, clamped on load', () => {
  it('names exactly Atlas and Dark', () => {
    expect([...MAP_STYLES]).toEqual(['atlas', 'dark'])
  })

  it('reads back what was saved, per window', () => {
    saveMapStyle('dark')
    expect(loadMapStyle()).toBe('dark')
    saveMapStyle('atlas')
    expect(loadMapStyle()).toBe('atlas')
    saveMapStyle(null)
    expect(loadMapStyle()).toBeNull()
  })

  it.each([['photo'], ['DARK'], [''], ['null'], ['{"style":"dark"}'], [' dark']])('a stored %j is no pick', (raw) => {
    localStorage.setItem('nexus.map.style', raw)
    expect(loadMapStyle()).toBeNull()
  })

  it('anything that is not a style is no pick', () => {
    for (const v of [undefined, null, 1, true, {}, ['dark'], 'photo']) expect(mapStyleOf(v)).toBeNull()
    expect(mapStyleOf('dark')).toBe('dark')
    expect(mapStyleOf('atlas')).toBe('atlas')
  })

  it('a pop-out starts on the main window’s pick and keeps its own after', () => {
    saveMapStyle('dark')
    window.history.replaceState({}, '', '/?panel=connect&instance=w2')
    expect(loadMapStyle()).toBe('dark')
    saveMapStyle('atlas')
    expect(loadMapStyle()).toBe('atlas')
    window.history.replaceState({}, '', '/')
    expect(loadMapStyle()).toBe('dark')
  })

  it('a pop-out that follows the main window’s pick follows a change made there, live', () => {
    window.history.replaceState({}, '', '/?panel=connect&instance=w2')
    const popout = renderHook(() => useMapStyle())
    expect(popout.result.current[0]).toBeNull()
    // The main window writes its record; this window hears it through the storage event.
    act(() => {
      localStorage.setItem('nexus.map.style', 'dark')
      window.dispatchEvent(new StorageEvent('storage', { key: 'nexus.map.style', newValue: 'dark' }))
    })
    expect(popout.result.current[0]).toBe('dark')
  })

  it('with storage refusing writes, a pick still applies for the session', () => {
    const set = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('QuotaExceededError')
    })
    const { result } = renderHook(() => useMapStyle())
    act(() => result.current[1]('dark'))
    expect(result.current[0]).toBe('dark')
    expect(loadMapStyle()).toBe('dark')
    act(() => result.current[1](null))
    expect(result.current[0]).toBeNull()
    expect(set).toHaveBeenCalled()
  })

  it('the hook follows a pick made by another map in the same window', () => {
    const a = renderHook(() => useMapStyle())
    const b = renderHook(() => useMapStyle())
    expect(a.result.current[0]).toBeNull()
    act(() => a.result.current[1]('dark'))
    expect(b.result.current[0]).toBe('dark')
    expect(loadMapStyle()).toBe('dark')
  })
})

describe('precedence: a pick wins, no pick is the theme’s map', () => {
  const midnight = SKINS.find((s) => s.id === 'midnight')!
  const paper = SKINS.find((s) => s.id === 'paper')!

  it('a pick paints its own palette, on any theme', () => {
    expect(mapPaletteFor('dark')).toEqual(DARK_MAP)
    expect(mapPaletteFor('atlas')).toEqual(STANDARD_MAP)
  })

  it('no pick reads the theme’s tokens (Atlas, or a dark theme’s own map)', () => {
    expect(mapPaletteFor(null)).toBeNull()
  })

  it('the control offers the theme’s own map only where the theme brings one', () => {
    expect(mapStyleOptions(null)).toEqual(['atlas', 'dark'])
    expect(mapStyleOptions(paper.id)).toEqual(['atlas', 'dark'])
    expect(midnight.map).toBeDefined()
    expect(mapStyleOptions(midnight.id)).toEqual(['theme', 'atlas', 'dark'])
  })

  it('the control shows what the map paints', () => {
    expect(mapStyleShown(null, null)).toBe('atlas')
    expect(mapStyleShown('dark', null)).toBe('dark')
    expect(mapStyleShown(null, midnight.id)).toBe('theme')
    expect(mapStyleShown('atlas', midnight.id)).toBe('atlas')
    expect(mapStyleShown('dark', midnight.id)).toBe('dark')
  })

  it('choosing what the theme already paints stores no pick, so a later dark theme still brings its map', () => {
    expect(mapStylePick('atlas', null)).toBeNull()
    expect(mapStylePick('dark', null)).toBe('dark')
    expect(mapStylePick('theme', midnight.id)).toBeNull()
    expect(mapStylePick('atlas', midnight.id)).toBe('atlas')
    expect(mapStylePick('dark', midnight.id)).toBe('dark')
  })
})

// ── Dark reads, and so does everything drawn over it ─────────────────────────────────────────
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
const ALL_MODES: Mode[] = [...new Set<Mode>([...MODES, ...SKIN_MODES])]
/** The theme tokens MapView fills or rings a marker, arc or label with (its `cssVar` reads that are
 *  not a font or a surface). */
const MARKER_TOKENS = [
  '--snr-strong',
  '--snr-marginal',
  '--snr-weak',
  '--text-faint',
  '--text',
  '--accent',
  '--band-open',
  '--band-marginal',
  '--band-closed',
  '--state-good',
  '--text-dim',
  '--need-pota',
  '--need-sota',
  '--need-dxped',
  // The APRS map's station categories, and the path's stretches (a flare or a proton event on it).
  '--aprs-cat-mobile',
  '--aprs-cat-fixed',
  '--aprs-cat-infra',
  '--aprs-cat-wx',
  '--aprs-cat-marine',
  '--aprs-cat-air',
  '--aprs-cat-object',
  '--aprs-cat-other',
  '--route-polar',
  '--route-grey',
  '--route-flare-weak',
  '--route-flare-out',
  '--route-pca',
] as const
/** A display well, the dark palette's scope (styles.css DISPLAY WELLS): what a Dark map reads its
 *  marker inks from. */
const WELL = [{ tag: 'span', classes: ['well'], attrs: {} }]
const hex = (c: string): Rgb => parseHex(c)!
const over = (bg: Rgb, ink: Rgb, a: number): Rgb => bg.map((v, i) => Math.round(v * (1 - a) + ink[i] * a)) as unknown as Rgb
const rgbOf = (c: string): Rgb => {
  const m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/.exec(c)
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : hex(c)
}
/** What a marker can land on: the flat map's land, the globe's land, the sea and the globe's lit sea. */
const GROUNDS = (p: Readonly<Record<MapToken, string>>) =>
  (['--map-land', '--map-land-globe', '--map-ocean', '--map-ocean-lit'] as const).map((t) => [t, hex(p[t])] as const)
/** The night side: the greyline's near-black at its core alpha (0.2 × three caps ≈ 0.6). */
const night = (c: Rgb) => over(c, rgbOf(NIGHT_INK), 0.6)

function inkOf(tokens: Map<string, string>, t: string): Rgb | null {
  const raw = tokens.get(t)
  return raw === undefined ? null : toRgb(expandWith(tokens, raw), [0, 0, 0])
}
/** Every marker ink under 3:1 (WCAG's floor for a graphic) on any ground, as a message. */
function markerFaults(palette: Readonly<Record<MapToken, string>>, mode: Mode, tokens: Map<string, string>): string[] {
  const out: string[] = []
  for (const t of MARKER_TOKENS) {
    const ink = inkOf(tokens, t)
    if (!ink) {
      out.push(`${mode}: ${t} is not declared`)
      continue
    }
    for (const [g, bg] of GROUNDS(palette)) {
      const r = contrast(ink, bg)
      if (r < 3) out.push(`${mode}: ${t} on ${g} = ${r.toFixed(2)}:1`)
    }
  }
  return out
}

describe('Dark is a dark map', () => {
  it('declares every basemap token, as plain hexes', () => {
    expect(Object.keys(DARK_MAP).sort()).toEqual([...MAP_TOKENS].sort())
    for (const t of MAP_TOKENS) expect(DARK_MAP[t], t).toMatch(/^#[0-9a-f]{6}$/)
  })

  it('its land and sea are dark, apart, and the globe’s shading runs lit → sea → limb', () => {
    for (const t of ['--map-land', '--map-land-globe', '--map-ocean', '--map-ocean-lit', '--map-ocean-deep'] as const) {
      expect(luminance(hex(DARK_MAP[t])), t).toBeLessThan(0.05)
    }
    expect(contrast(hex(DARK_MAP['--map-land']), hex(DARK_MAP['--map-ocean']))).toBeGreaterThanOrEqual(1.3)
    expect(contrast(hex(DARK_MAP['--map-land-globe']), hex(DARK_MAP['--map-ocean']))).toBeGreaterThanOrEqual(1.2)
    const L = (t: MapToken) => luminance(hex(DARK_MAP[t]))
    expect(L('--map-ocean-lit')).toBeGreaterThan(L('--map-ocean'))
    expect(L('--map-ocean')).toBeGreaterThan(L('--map-ocean-deep'))
  })

  it('is not any theme’s map, nor the atlas', () => {
    for (const p of [STANDARD_MAP, ...SKINS.flatMap((s) => (s.map ? [s.map] : []))]) {
      expect(DARK_MAP['--map-ocean']).not.toBe(p['--map-ocean'])
    }
  })
})

describe('everything drawn over Dark reads', () => {
  it('coasts and borders, the grid and the range rings (the coast ink) stand off land and sea', () => {
    for (const [g, bg] of GROUNDS(DARK_MAP)) {
      expect(contrast(hex(DARK_MAP['--map-coast']), bg), `coast on ${g}`).toBeGreaterThanOrEqual(4.5)
      // The grid is stroked at 0.6 of its opacity: it reads at least as well as on Atlas.
      const grid = contrast(over(bg, hex(DARK_MAP['--map-coast']), 0.6), bg)
      const atlas = Math.min(...GROUNDS(STANDARD_MAP).map(([, a]) => contrast(over(a, hex(STANDARD_MAP['--map-coast']), 0.6), a)))
      expect(grid, `grid on ${g}`).toBeGreaterThanOrEqual(atlas)
    }
    expect(contrast(hex(DARK_MAP['--map-state']), hex(DARK_MAP['--map-land'])), 'state lines on land').toBeGreaterThanOrEqual(2)
    expect(contrast(hex(DARK_MAP['--map-rim']), hex(DARK_MAP['--map-land'])), 'rivers on land').toBeGreaterThanOrEqual(1.3)
  })

  it('the greyline’s line reads on the day side and the night side', () => {
    for (const [g, bg] of GROUNDS(DARK_MAP)) {
      expect(contrast(rgbOf(GREYLINE_INK), bg), `day ${g}`).toBeGreaterThanOrEqual(4.5)
      expect(contrast(rgbOf(GREYLINE_INK), night(bg)), `night ${g}`).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('the night shading still darkens the land a visible step, as on the dark themes’ maps', () => {
    // A dark day side leaves the shading less to darken than on Atlas (5:1); the gold line above is what
    // marks the greyline. The step is held at least to the dark built-in themes' (the darkest, Midnight,
    // gives 1.14:1), and to 1.25:1 whatever they do.
    const step = (p: Readonly<Record<MapToken, string>>) => contrast(hex(p['--map-land']), night(hex(p['--map-land'])))
    const themes = SKINS.flatMap((s) => (s.map ? [step(s.map)] : []))
    expect(themes.length).toBeGreaterThan(0)
    expect(step(DARK_MAP)).toBeGreaterThanOrEqual(Math.max(1.25, Math.min(...themes)))
  })

  it('the TX and RX arcs, the band colours and the need colours read on every ground', () => {
    const needs: NeedTag[] = ['NewEntity', 'NewIota', 'NewZone', 'NewBand', 'NewMode', 'Confirm']
    const fixed: Record<string, string> = { GETTING_OUT, PATH_RX, ...BAND_COLOR }
    for (const n of needs) fixed[n] = needColor(n)!
    const faults: string[] = []
    for (const [name, c] of Object.entries(fixed)) {
      for (const [g, bg] of GROUNDS(DARK_MAP)) {
        const r = contrast(hex(c), bg)
        if (r < 3) faults.push(`${name} ${c} on ${g} = ${r.toFixed(2)}:1`)
      }
    }
    expect(faults).toEqual([])
  })

  it('every marker ink, read from the dark palette (a well), reads in every UI theme and mode', () => {
    expect(ALL_MODES.some((m) => baseTheme(m) === 'light')).toBe(true)
    expect(ALL_MODES.flatMap((m) => markerFaults(DARK_MAP, m, tokensAt(RULES, m, WELL)))).toEqual([])
  })

  it('NEGATIVE CONTROL — the light theme’s own inks would not read on Dark (why the map reads the well)', () => {
    expect(markerFaults(DARK_MAP, 'light', rootTokensFrom(RULES, 'light')).length).toBeGreaterThan(0)
  })
})

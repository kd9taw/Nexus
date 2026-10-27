// @vitest-environment jsdom
//
// A MAP LABEL READS IN EVERY THEME: ITS HALO FOLLOWS ITS INK.
//
// The 2-D maps letter each label over a halo (MapView's `haloText`): the APRS stations, Connect's
// stations, spots, satellites and DXpeditions and, at street scale, the grid squares of stations, spots,
// parks and the QTH. The halo was always MARKER_HALO, a near-black. Under the dark theme's light inks it
// is what lifts a label off the map; the light theme letters its labels in DARK inks (--text #14202e,
// --accent #0174ab), so there every label was a dark blot, on the APRS Globe as on Connect's maps.
//
// Each label is read where it is drawn: the REAL MapView, its canvases given a context that records the
// ink and the halo of every label, and the theme's inks resolved from the sheet itself (cssCascade.ts) and
// set on the root, where the map reads them. Every theme, colour-role preset and built-in theme is swept
// (each base mode bare and under every preset, and every theme of its base: 192 modes), one map per
// scene, re-drawn in each. What every label holds:
//   (1) its ink is the colour its path letters in: a theme token the sheet declares in both themes, or
//       one of the map's own fixed colours (a band's, a need's), the same in every theme;
//   (2) its ink is at least 4.5:1 from its halo (WCAG's floor for small text), on the halo's own colour:
//       its 0.9 alpha is left out, since what shows through it is whatever map lies under the label;
//   (3) in a dark theme its halo is MARKER_HALO exactly, what every dark theme has always drawn, so the
//       dark themes do not move by a pixel.
// (2) is what the light theme failed, at 1.2:1 for --text. A halo that followed the THEME instead of the
// ink fails it from the other side: the band and need colours are light inks in every theme (15 m's
// #ffcc44 is 1.5:1 on a white halo), so they keep the dark halo in the light theme too.
// RepeaterMap letters its own labels, in fixed light inks over the same dark halo: the control, in both
// themes.
import { act, cleanup, render, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ComponentProps } from 'react'
import type { AprsStation } from '../api'
import type { MapSpot, NeedTag, OtaMapSpot, PropagationSnapshot, SatView, Station, WorkableCard } from '../types'
import type { StreetPack } from '../features/streetPack'
import type { StreetCamera, StreetMapProps } from './StreetMap'
import {
  MODES,
  SKIN_MODES,
  baseTheme,
  contrast,
  expandWith,
  parseHex,
  parseRules,
  rgbHex,
  rootTokensFrom,
  toRgb,
  type Mode,
  type Rgb,
} from '../cssCascade'
import { gridToLatLon, type LatLon } from '../grid'

const feeds = vi.hoisted(() => ({ sats: null as unknown }))
vi.mock('../api', () => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => feeds.sats),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => ({ calls: [], parks: [] })),
}))
// The street renderer's stand-in (jsdom has no WebGL): it keeps the props MapView gives it, so a test
// can hand MapView a camera.
const fake = vi.hoisted(() => ({ props: null as null | Record<string, unknown> }))
vi.mock('./StreetMap', async () => {
  const { forwardRef } = await import('react')
  return {
    default: forwardRef<unknown, Record<string, unknown>>(function FakeStreet(props, _ref) {
      fake.props = props
      return <div className="street-map" data-testid="fake-street" />
    }),
  }
})

import { MapView, MARKER_HALO } from './MapView'
import { RepeaterMap } from './RepeaterMap'

// 192 re-drawn maps per scene: the slowest case is the sweep, and a loaded suite runs it many times slower
// than an idle core. 15 s is the house budget for a case; a sweep gets four of them.
vi.setConfig({ testTimeout: 60_000 })

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

/** One label as the map lettered it: its text, the ink it was filled in and the halo stroked under it. */
type Label = { text: string; ink: string; halo: string }
let labels: Label[] = []
/** A 2-D context that keeps what is set on it and answers every call, recording each label: a text
 *  stroked (its halo) and then filled (its ink). A text filled without a halo is not a label here. */
function recordingContext(canvas: HTMLCanvasElement) {
  const state: Record<string | symbol, unknown> = {}
  let stroked: { text: string; halo: string } | null = null
  return new Proxy(state, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        if (k === 'strokeText') stroked = { text: String(a[0]), halo: String(t.strokeStyle) }
        if (k === 'fillText') {
          if (stroked && stroked.text === String(a[0])) labels.push({ ...stroked, ink: String(t.fillStyle) })
          stroked = null
        }
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}

const W = 1000
const H = 800
beforeEach(() => {
  localStorage.clear()
  labels = []
  feeds.sats = null
  fake.props = null
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => W)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => H)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  document.documentElement.removeAttribute('style')
  document.documentElement.removeAttribute('data-theme')
})

// ── the themes, from the sheet ──
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The theme tokens the map letters a label in. */
const INK_TOKENS = [
  '--text',
  '--accent',
  '--text-faint',
  '--snr-strong',
  '--snr-marginal',
  '--snr-weak',
  '--status-new-entity',
  '--status-new-band',
  '--status-new-mode',
  '--status-confirmed',
  '--status-dupe',
] as const
type InkToken = (typeof INK_TOKENS)[number]
/** Every base mode, bare and under every colour-role preset, and every built-in theme of its base. */
const ALL_MODES: Mode[] = [...new Set<Mode>([...MODES, ...SKIN_MODES])]
const DARK_MODES = ALL_MODES.filter((m) => baseTheme(m) === 'dark')
const LIGHT_MODES = ALL_MODES.filter((m) => baseTheme(m) === 'light')

/** Each ink token's colour in `mode`, as a canvas hands a colour back (#rrggbb); '' where the sheet does
 *  not declare it. */
function inksOf(mode: Mode): Map<InkToken, string> {
  const tokens = rootTokensFrom(RULES, mode)
  const bg = toRgb(expandWith(tokens, tokens.get('--bg') ?? '#000000'), [0, 0, 0]) ?? ([0, 0, 0] as Rgb)
  return new Map(
    INK_TOKENS.map((t) => {
      const raw = tokens.get(t)
      const c = raw === undefined ? null : toRgb(expandWith(tokens, raw), bg)
      return [t, c ? rgbHex(c) : '']
    }),
  )
}
/** Put `mode`'s inks on the root, where the map reads its colours. */
function wear(mode: Mode): Map<InkToken, string> {
  const inks = inksOf(mode)
  const root = document.documentElement
  root.setAttribute('data-theme', baseTheme(mode))
  for (const [t, v] of inks) root.style.setProperty(t, v)
  return inks
}

/** A colour's own RGB, its alpha left out: #rgb / #rrggbb, or rgb() / rgba(). */
function own(c: string): Rgb | null {
  const hex = parseHex(c)
  if (hex) return hex
  const m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/.exec(c.trim())
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null
}

/** What a label is lettered in: a theme token, or one of the map's own fixed colours. */
type Ink = InkToken | 'fixed'
/** Every way the labels lettered in `mode` break (1)–(3); empty when they hold. `fixed` carries each
 *  fixed-colour label's ink from the first mode, so a fixed colour that moves with the theme is caught. */
function faults(mode: Mode, inks: Map<InkToken, string>, expected: Record<string, Ink>, fixed: Map<string, string>): string[] {
  const out: string[] = []
  for (const l of labels) if (!(l.text in expected)) out.push(`${mode}: "${l.text}" is a label this test does not know`)
  for (const [text, ink] of Object.entries(expected)) {
    const mine = labels.filter((l) => l.text === text)
    if (mine.length === 0) out.push(`${mode}: "${text}" was not lettered`)
    for (const l of mine) {
      if (ink === 'fixed') {
        if (!fixed.has(text)) fixed.set(text, l.ink)
        if (l.ink !== fixed.get(text)) out.push(`${mode}: "${text}" in ${l.ink}, its fixed colour ${fixed.get(text)} in another theme`)
      } else if (l.ink !== inks.get(ink)) out.push(`${mode}: "${text}" in ${l.ink}, not its ${ink} ${inks.get(ink)}`)
      const fg = own(l.ink)
      const halo = own(l.halo)
      if (!fg || !halo) {
        out.push(`${mode}: "${text}" in ${l.ink} on ${l.halo}: not a colour`)
        continue
      }
      const ratio = contrast(fg, halo)
      if (ratio < 4.5) out.push(`${mode}: "${text}" in ${l.ink} on its halo ${l.halo}: ${ratio.toFixed(2)}:1`)
      if (baseTheme(mode) === 'dark' && l.halo !== MARKER_HALO) out.push(`${mode}: "${text}" on ${l.halo}, not the dark themes' ${MARKER_HALO}`)
    }
  }
  return out
}

// ── the scenes ──
const MY_GRID = 'EN52'
const ME = gridToLatLon(MY_GRID)!
const near = (dLat: number, dLon: number): LatLon => ({ lat: ME.lat + dLat, lon: ME.lon + dLon })
const NOW = 1_790_000_000

/** Only these layers on: nothing else letters a label or covers one. */
function layers(on: string[]): void {
  const off = ['daynight', 'sunMoon', 'relief', 'muf', 'aurora', 'flare', 'pca', 'coast', 'states', 'grid', 'gridLabels', 'cqzones', 'coverage', 'rings', 'heat', 'openings', 'paths', 'txPaths', 'rxPaths']
  const table: Record<string, { visible: boolean }> = {}
  for (const k of off) table[k] = { visible: false }
  for (const k of ['sats', 'aprs', 'liveSpots', 'stations', 'dxped', 'ota']) table[k] = { visible: on.includes(k) }
  localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { layers: table } }))
}

const station = (call: string, grid: string, over: Partial<Station> = {}): Station =>
  ({ call, grid, snr: -10, lastHeardSlot: 1, heardCount: 2, presence: 'active', worked: false, ...over }) as unknown as Station
const spot = (call: string, ll: LatLon, over: Partial<MapSpot> = {}): MapSpot =>
  ({ call, lat: ll.lat, lon: ll.lon, band: '20m', heardMe: false, ageSecs: 60, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'X', ...over }) as unknown as MapSpot
const card = (need: WorkableCard['need'], brg: number): WorkableCard =>
  ({ call: `DX${brg}`, entity: 'X', need, band: '20m', bearingDeg: brg, distanceKm: 800, octant: 'N', status: 'WorkNow', likelihood: 'Good', likelihoodScore: 0.7, liveConfirmed: false, howToCall: '', windowHint: '', priority: 1 }) as unknown as WorkableCard
const snap = (over: Partial<PropagationSnapshot>): PropagationSnapshot =>
  ({ source: 'live', asOf: 1, spots: [], openings: [], dxpeditions: { workableNow: [] }, spaceWx: { sfi: 150, kp: 2, aIndex: 5, xrayClass: 'B2', flare: false, xrayLong: 1e-7 }, insights: [], ...over }) as unknown as PropagationSnapshot
const park = (activator: string, ll: LatLon, newRef: boolean): OtaMapSpot =>
  ({ program: 'POTA', reference: 'K-0001', name: 'Park', activator, freqMhz: 14.074, mode: 'SSB', lat: ll.lat, lon: ll.lon, approx: true, ageSecs: 60, newRef }) as OtaMapSpot
const moving = (call: string, ll: LatLon): AprsStation =>
  ({ call, lat: ll.lat, lon: ll.lon, symbolTable: '/', symbolCode: '>', kind: 'position', text: '', speedKnots: 20, courseDeg: 90, path: [], raw: '', lastHeardUnix: NOW, lastRfUnix: NOW, lastInetUnix: null, sourceKind: 'rf', packets: 1, firstHeardUnix: NOW, wx: null }) as unknown as AprsStation

/** THE GLOBE, as the APRS map and Connect draw it: every label kind a world-scale map letters. */
const GLOBE_LABELS: Record<string, Ink> = {
  APRS1: '--text', // a moving APRS station
  'BIRD-1': '--text', // a satellite
  SEL: '--accent', // the selected station, and the selected spot
  ATNO1: 'fixed', // needed stations, in their need's colour
  ZONE1: 'fixed',
  BAND1: 'fixed',
  MODE1: 'fixed',
  CONF1: 'fixed',
  '★': '--status-new-entity', // DXpeditions, in their need's status colour
  '◑': '--status-new-band',
  '◧': '--status-new-mode',
  '✓': '--status-confirmed',
  '·': '--status-dupe',
}
async function mountGlobe(mode: Mode) {
  layers(['sats', 'aprs', 'liveSpots', 'stations', 'dxped'])
  const bird = { name: 'BIRD-1', norad: null, lat: ME.lat + 3, lon: ME.lon - 4, altKm: 420, footprintKm: 2200, track: [], amateur: true, status: 'alive', classes: ['fm'] }
  feeds.sats = { tleAgeDays: 1, usableCount: 1, agingCount: 0, heldBackCount: 0, tleFetchedAt: 0, tleSource: 'bundled', birds: [bird] } as unknown as SatView
  const needs = new Map<string, NeedTag>([
    ['ATNO1', 'NewEntity'],
    ['ZONE1', 'NewZone'],
    ['BAND1', 'NewBand'],
    ['MODE1', 'NewMode'],
    ['CONF1', 'Confirm'],
  ])
  const props: ComponentProps<typeof MapView> = {
    myGrid: MY_GRID,
    theme: mode as never,
    stations: [station('ATNO1', 'EN61'), station('ZONE1', 'EN43'), station('BAND1', 'EN71'), station('MODE1', 'EM59'), station('CONF1', 'EN35'), station('SEL', 'EN62')],
    prop: snap({
      spots: [spot('SEL', near(-2, 3)), spot('OTHER', near(2, -3))],
      dxpeditions: { workableNow: [card('Atno', 20), card('NewBand', 80), card('NewMode', 150), card('Confirm', 220), card('Satisfied', 300)] } as never,
    }),
    selectedCall: 'SEL',
    onSelectCall: () => {},
    needByCall: needs,
    intent: 'dx',
    projection: 'globe',
    aprs: [moving('APRS1', near(0.5, 0.5))],
    aprsNowSec: NOW,
  }
  return mountAndRedraw(props, mode, async () => {})
}

/** STREET SCALE: each station, spot and park is its grid square, lettered with its call. */
const STREET_LABELS: Record<string, Ink> = {
  WKD1: '--text-faint', // a station already worked and not needed
  STR1: '--snr-strong', // stations neither worked nor needed, in their signal's colour
  MAR1: '--snr-marginal',
  WEAK1: '--snr-weak',
  ATNO2: 'fixed', // a needed station
  SEL: '--accent', // the selected station, and the selected spot
  HRD1: 'fixed', // a spot that heard me
  B160: 'fixed', // spots in their band's colour: 160 m the darkest, 15 m the lightest
  B15: 'fixed',
  NEWP: '--accent', // a park never logged
  OLDP: '--text-faint', // a park already logged
  [MY_GRID]: '--accent', // the QTH's own square
}
const PACK: StreetPack = {
  id: '20261004-streets-200km-4250n08900w',
  name: 'EN52 200 km',
  bbox: [-90.2, 41.6, -87.8, 43.4],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 1_000_000,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
}
// MapLibre's projection for an unrotated camera centred on the canvas (its Web Mercator formula).
const mx = (lng: number) => (180 + lng) / 360
const my = (lat: number) => (180 - (180 / Math.PI) * Math.log(Math.tan(Math.PI / 4 + (lat * Math.PI) / 360))) / 360
const camera = (lon: number, lat: number, zoom: number): StreetCamera => ({
  center: [lon, lat],
  zoom,
  bearing: 0,
  project: ([x, y]) => [(mx(x) - mx(lon)) * 512 * 2 ** zoom + W / 2, (my(y) - my(lat)) * 512 * 2 ** zoom + H / 2],
  moving: false,
})
async function mountStreet(mode: Mode) {
  layers(['liveSpots', 'stations', 'ota'])
  const props: ComponentProps<typeof MapView> = {
    myGrid: MY_GRID,
    theme: mode as never,
    stations: [
      station('WKD1', 'EN52', { worked: true }),
      station('STR1', 'EN52', { snr: -5 }),
      station('MAR1', 'EN52', { snr: -15 }),
      station('WEAK1', 'EN52', { snr: -25 }),
      station('ATNO2', 'EN52'),
      station('SEL', 'EN52'),
    ],
    prop: snap({ spots: [spot('SEL', ME), spot('HRD1', ME, { heardMe: true }), spot('B160', ME, { band: '160m' }), spot('B15', ME, { band: '15m' })] }),
    selectedCall: 'SEL',
    onSelectCall: () => {},
    needByCall: new Map<string, NeedTag>([['ATNO2', 'NewEntity']]),
    intent: 'dx',
    projection: 'street',
    streetPack: PACK,
    ota: [park('NEWP', ME, true), park('OLDP', ME, false)],
  }
  return mountAndRedraw(props, mode, async () => {
    await screen.findByTestId('fake-street')
    act(() => (fake.props as unknown as StreetMapProps).onCamera?.(camera(ME.lon, ME.lat, 12)))
  })
}

/** Mount the map wearing `first`'s inks; the returned `redraw` re-draws it in a mode's (the theme is a
 *  draw dependency of the map, and the first thing a draw does is drop its cached colours). */
async function mountAndRedraw(props: ComponentProps<typeof MapView>, first: Mode, settle: () => Promise<void>) {
  wear(first)
  let r!: ReturnType<typeof render>
  // Mounted under a theme name no mode has, so the first `redraw` is a change too.
  await act(async () => {
    r = render(<MapView {...props} theme={'mount' as never} />)
  })
  // The feeds a layer fetches for itself (the satellites) land a tick later.
  await act(async () => {})
  await act(async () => {})
  await settle()
  return async (mode: Mode): Promise<Map<InkToken, string>> => {
    const inks = wear(mode)
    labels = []
    await act(async () => {
      r.rerender(<MapView {...props} theme={mode as never} />)
    })
    return inks
  }
}

/** Sweep `modes` over a scene; every fault, all at once. */
async function sweep(mount: (m: Mode) => Promise<(m: Mode) => Promise<Map<InkToken, string>>>, modes: Mode[], expected: Record<string, Ink>) {
  const redraw = await mount(modes[0])
  // The fixed colours as the OTHER theme letters them: a fixed colour is the same in every theme.
  await redraw(baseTheme(modes[0]) === 'dark' ? 'light' : 'dark')
  const fixed = new Map(labels.filter((l) => expected[l.text] === 'fixed').map((l) => [l.text, l.ink]))
  const out: string[] = []
  let lettered = 0
  for (const mode of modes) {
    const inks = await redraw(mode)
    lettered += labels.length
    out.push(...faults(mode, inks, expected, fixed))
  }
  return { faults: out, lettered }
}

describe('the theme inks a map letters its labels in', () => {
  it('are declared in both themes, and differ between them (the light theme letters in dark inks)', () => {
    const dark = inksOf('dark')
    const light = inksOf('light')
    for (const t of INK_TOKENS) {
      expect(dark.get(t), `${t} in the dark theme`).toMatch(/^#[0-9a-f]{6}$/)
      expect(light.get(t), `${t} in the light theme`).toMatch(/^#[0-9a-f]{6}$/)
    }
    // CONTROL: the sweep below has something to find. The light theme's --text is the dark ink its
    // labels were blotted in.
    expect(contrast(own(light.get('--text')!)!, own(MARKER_HALO)!), 'the light --text on MARKER_HALO').toBeLessThan(1.5)
  })
})

describe('every label on the Globe (the APRS map, and Connect’s): 4.5:1 from its halo', () => {
  it('in every dark theme, on the halo the dark themes always drew', async () => {
    const r = await sweep(mountGlobe, DARK_MODES, GLOBE_LABELS)
    expect(r.faults).toEqual([])
    expect(r.lettered, 'labels lettered').toBeGreaterThanOrEqual(DARK_MODES.length * Object.keys(GLOBE_LABELS).length)
  })
  it('in every light theme', async () => {
    const r = await sweep(mountGlobe, LIGHT_MODES, GLOBE_LABELS)
    expect(r.faults).toEqual([])
    expect(r.lettered, 'labels lettered').toBeGreaterThanOrEqual(LIGHT_MODES.length * Object.keys(GLOBE_LABELS).length)
  })
})

describe('every label at street scale (the grid squares of stations, spots, parks and the QTH): 4.5:1 from its halo', () => {
  it('in every dark theme, on the halo the dark themes always drew', async () => {
    const r = await sweep(mountStreet, DARK_MODES, STREET_LABELS)
    expect(r.faults).toEqual([])
    expect(r.lettered, 'labels lettered').toBeGreaterThanOrEqual(DARK_MODES.length * Object.keys(STREET_LABELS).length)
  })
  it('in every light theme', async () => {
    const r = await sweep(mountStreet, LIGHT_MODES, STREET_LABELS)
    expect(r.faults).toEqual([])
    expect(r.lettered, 'labels lettered').toBeGreaterThanOrEqual(LIGHT_MODES.length * Object.keys(STREET_LABELS).length)
  })
})

describe('the control: RepeaterMap letters its labels in fixed light inks over the dark halo', () => {
  it('and they read in both themes', async () => {
    const seen = new Map<string, Label>()
    for (const mode of ['dark', 'light'] as Mode[]) {
      wear(mode)
      labels = []
      const machine = { id: 'm1', lat: ME.lat + 0.1, lon: ME.lon + 0.1, call: 'RPT1', mhz: 146.94, city: 'Town', added: false, pickable: true, savable: true, saved: false }
      const r = render(<RepeaterMap markers={[machine]} from={ME} fromLabel="HOME" reachKm={40} onSelect={() => {}} onPick={() => {}} onSave={() => {}} />)
      await act(async () => {})
      expect(labels.map((l) => l.text), `${mode}: the machine's call and the place are lettered`).toEqual(expect.arrayContaining(['RPT1', 'HOME']))
      for (const l of labels) {
        expect(l.halo, `${mode}: "${l.text}"`).toBe(MARKER_HALO)
        expect(contrast(own(l.ink)!, own(l.halo)!), `${mode}: "${l.text}" in ${l.ink}`).toBeGreaterThanOrEqual(4.5)
        const before = seen.get(l.text)
        if (before) expect(l.ink, `"${l.text}": the same ink in both themes`).toBe(before.ink)
        seen.set(l.text, l)
      }
      r.unmount()
    }
  })
})

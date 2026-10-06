// @vitest-environment jsdom
//
// A CONNECT BOX'S OWN TEXT SIZE (⋯ ▸ A− / A+, 80–160 %) REACHES EVERY WORD IN THE BOX — computed
// against the real sheets, for every box in the registry, rendered with data.
//
// The operator's pick is "per-box text size", and the whole difficulty is the one #215 met: the
// sheet sizes text four ways. `calc(<n>px * var(--text-scale))` follows a --text-scale set on the
// box body by itself. A `var(--fs-*)` does NOT — a custom property is computed where it is
// DECLARED, so the root's --fs-body is already a length by the time a box inherits it. A `rem` is
// measured against <html> and never sees the box at all. And `em` / `%` follow whatever the parent
// does. A box whose A+ grew half its words would be worse than no control, so the property held
// here is every word:
//
//   inside a box at factor f, EVERY element that carries text has f × the font size it has at
//   100 %, and at 100 % it has exactly the size it had before the box text size existed.
//
// The second half is the drift guard: the box re-declares the five --fs-* tokens, and a
// re-declaration that stopped matching the root's would change every box at 100 % — caught here.
//
// IT COMPUTES, with the app's own resolver (cssCascade.ts): selectors are matched against the
// rendered chain, custom properties follow CSS's own rule (an inherited token arrives computed, a
// declared one resolves on its own element — `tokensAt`), inline styles count, and every length is
// evaluated to pixels. jsdom lays nothing out, so this measures what the cascade SIZES, not where
// it lands; the report's Chrome census measured the same boxes on screen.
//
// SVG text is out, as in the #215 audit: a drawing's labels are sized in its own viewBox units and
// grow with the drawing, not with a font size.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import {
  cmpSpec,
  elOf,
  expandWith,
  parseRules,
  reachesChain,
  rootTokensFrom,
  type El,
  type Mode,
  type Rule,
} from '../../cssCascade'

const fx = vi.hoisted(() => {
  const NOW = Math.floor(Date.now() / 1000)
  return {
    NOW,
    episodes: [
      { band: '6m', mode: 'Sporadic-E', startedUtc: NOW - 7200, endedUtc: NOW - 5400, durationSecs: 1800, onsetKnown: true, peakZ: 4.2, maxKm: 1850, peakStations: 9, bearingDeg: 140, octant: 'SE' },
      { band: '10m', mode: 'F2', startedUtc: NOW - 20000, endedUtc: NOW - 15000, durationSecs: 5000, onsetKnown: false, peakZ: 3.1, maxKm: 7400, peakStations: 14, bearingDeg: 60, octant: 'NE' },
    ],
    kp: {
      points: Array.from({ length: 24 }, (_, i) => ({
        timeUnix: NOW - 12 * 10800 + i * 10800,
        kp: 2 + (i % 5),
        kind: i < 12 ? 'observed' : i < 14 ? 'estimated' : 'predicted',
        noaaScale: i % 5 === 4 ? 'G1' : null,
      })),
    },
    sats: {
      tleAgeDays: 1,
      usableCount: 97,
      agingCount: 0,
      heldBackCount: 0,
      tleFetchedAt: NOW,
      tleSource: 'mirror',
      birds: [],
      passes: [
        { name: 'RS-44', norad: 44909, aosUnix: NOW + 600, losUnix: NOW + 1200, maxElDeg: 45, aosAzDeg: 20, losAzDeg: 200 },
        { name: 'AO-7', norad: 7530, aosUnix: NOW + 3600, losUnix: NOW + 4300, maxElDeg: 12, aosAzDeg: 310, losAzDeg: 90 },
      ],
      excluded: [],
    },
    solar: {
      days: Array.from({ length: 30 }, (_, i) => ({ dayUnix: (Math.floor(NOW / 86400) - 30 + i) * 86400, sfi: 120 + (i % 7), ssn: 80 + (i % 11) })),
    },
    contests: [
      { name: 'CQ World Wide DX Contest, SSB', startUnix: NOW + 86400, endUnix: NOW + 3 * 86400, url: 'https://cqww.com' },
      { name: 'ARRL 10-Meter Contest', startUnix: NOW - 3600, endUnix: NOW + 40000, url: null },
    ],
    spectrum: { row: Array.from({ length: 256 }, (_, i) => -110 + (i % 32)), startHz: 0, binHz: 11.7, source: 'audio' },
  }
})

vi.mock('../../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../api')>()),
  getOpeningsLog: vi.fn(async () => fx.episodes),
  getKpForecast: vi.fn(async () => fx.kp),
  getSatellites: vi.fn(async () => fx.sats),
  getSolarIndices: vi.fn(async () => fx.solar),
  getContests: vi.fn(async () => fx.contests),
  getSpectrumRow: vi.fn(async () => fx.spectrum),
  getSettings: vi.fn(async () => ({ rotatorModel: 202, rotatorHost: '' })),
  readRotatorState: vi.fn(async () => ({ azDeg: 123, reading: 'position' })),
  readRotator: vi.fn(async () => 123),
  getSatTrackStatus: vi.fn(async () => null),
  getDeclination: vi.fn(async () => 3.2),
}))

import { PaneFrame } from './PaneFrame'
import { PANES } from './panes'
import type { PaneContext } from './paneContext'
import type { PaneId } from '../../features/connectConfig'
import type { BandReport, PropagationSnapshot } from '../../types'

// ── The sheets and the resolver ────────────────────────────────────────────────────────────

const read = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const ALL_RULES: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css'))
/** The rules that ARE the box text size. Dropping them gives the size every word had before it. */
const isBoxTextRule = (r: Rule) => r.selector.includes('.pane-frame[data-slot]')
const MODE: Mode = 'dark'
/** The effective viewport width the app stamps on <html> (useViewport); any width does, the
 *  check is a ratio. */
const VW_EFF = 1200

/** The class a selector's SUBJECT compound requires — a NECESSARY condition for it to match,
 *  which is all an index may use (cssCascade.testkit's reasoning). */
function subjectClass(sel: string): string | null {
  const stripped = sel.replace(/:(?:where|is|not|has)\([^)]*\)/g, '')
  const last = stripped.replace(/\s*[>+~]\s*/g, ' ').split(' ').filter(Boolean).pop() ?? ''
  return /\.([\w-]+)/.exec(last)?.[1] ?? null
}

class Sheet {
  readonly root: Map<string, string>
  private readonly byClass = new Map<string, Rule[]>()
  private readonly unfiled: Rule[] = []
  constructor(readonly rules: Rule[]) {
    this.root = rootTokensFrom(rules, MODE)
    for (const r of rules) {
      if (!r.decls.some((d) => d.prop === 'font-size' || d.prop === 'font' || d.prop.startsWith('--'))) continue
      const c = subjectClass(r.selector)
      if (!c) this.unfiled.push(r)
      else this.byClass.set(c, [...(this.byClass.get(c) ?? []), r])
    }
  }
  /** Every rule that could reach `node`; the verdict is still reachesChain's. */
  candidates(node: Element): Rule[] {
    const out = [...this.unfiled]
    for (const c of Array.from(node.classList)) out.push(...(this.byClass.get(c) ?? []))
    return out
  }
}
const WITH = new Sheet(ALL_RULES)
const WITHOUT = new Sheet(ALL_RULES.filter((r) => !isBoxTextRule(r)))

const important = (v: string) => /!\s*important\s*$/.test(v)
const bare = (v: string) => v.replace(/!\s*important\s*$/, '').trim()

/** Evaluate a resolved length to px: calc/min/max/clamp over px, unitless, rem, em, % and vw. */
function evalPx(src: string, ctx: { em: number; rem: number }): number {
  let i = 0
  const s = src.trim()
  const ws = () => {
    while (i < s.length && /\s/.test(s[i])) i++
  }
  const expr = (): number => {
    let v = term()
    for (ws(); i < s.length && (s[i] === '+' || s[i] === '-'); ws()) {
      const op = s[i++]
      const r = term()
      v = op === '+' ? v + r : v - r
    }
    return v
  }
  const term = (): number => {
    let v = factor()
    for (ws(); i < s.length && (s[i] === '*' || s[i] === '/'); ws()) {
      const op = s[i++]
      const r = factor()
      v = op === '*' ? v * r : v / r
    }
    return v
  }
  const args = (): number[] => {
    const out: number[] = []
    ws()
    if (s[i] !== '(') throw new Error(`expected ( in ${s}`)
    i++
    for (;;) {
      out.push(expr())
      ws()
      if (s[i] === ',') {
        i++
        continue
      }
      if (s[i] === ')') {
        i++
        return out
      }
      throw new Error(`bad argument list in ${s}`)
    }
  }
  const factor = (): number => {
    ws()
    const fn = /^(calc|min|max|clamp)\b/.exec(s.slice(i))
    if (fn) {
      i += fn[1].length
      const a = args()
      if (fn[1] === 'calc') return a[0]
      if (fn[1] === 'min') return Math.min(...a)
      if (fn[1] === 'max') return Math.max(...a)
      return Math.min(Math.max(a[0], a[1]), a[2])
    }
    if (s[i] === '(') {
      i++
      const v = expr()
      ws()
      i++
      return v
    }
    const num = /^(-?\d*\.?\d+)(px|rem|em|%|vw)?/.exec(s.slice(i))
    if (!num) throw new Error(`cannot read "${s.slice(i)}" in ${s}`)
    i += num[0].length
    const n = Number(num[1])
    switch (num[2]) {
      case 'rem':
        return n * ctx.rem
      case 'em':
        return n * ctx.em
      case '%':
        return (n * ctx.em) / 100
      case 'vw':
        return (n * VW_EFF) / 100
      default:
        return n
    }
  }
  const v = expr()
  ws()
  if (i !== s.length) throw new Error(`trailing "${s.slice(i)}" in ${s}`)
  return v
}

interface Computed {
  tokens: Map<string, string>
  px: number
}

/** Font size in px of every element from <body> down, with CSS's inheritance: a token an element
 *  inherits arrives computed, a token it declares (sheet winner, or its own inline style) resolves
 *  against its own tokens; font-size is the winning declaration evaluated against the parent's
 *  size, and inherits when nothing declares it or what it declares is invalid. */
function fontSizes(sheet: Sheet, appScale: number): Map<Element, Computed> {
  const rootDecl = new Map(sheet.root)
  rootDecl.set('--text-scale', String(appScale))
  rootDecl.set('--vw-eff', `${VW_EFF}px`)
  const rootTokens = new Map([...rootDecl].map(([k, v]) => [k, expandWith(rootDecl, v)]))
  // html { font-size: calc(100% * var(--text-scale)) } over the UA's 16 px.
  const remPx = 16 * appScale
  const out = new Map<Element, Computed>()
  const visit = (node: Element, chain: El[], parent: Computed) => {
    const at = [...chain, elOf(node)]
    const cands = sheet.candidates(node).filter((r) => reachesChain(r.selector, at, MODE))
    // custom properties declared ON this element
    const declared = new Map<string, { rule: Rule | null; value: string }>()
    for (const rule of cands) {
      for (const d of rule.decls) {
        if (!d.prop.startsWith('--')) continue
        const prev = declared.get(d.prop)
        if (!prev || (prev.rule && (cmpSpec(rule.spec, prev.rule.spec) > 0 || (cmpSpec(rule.spec, prev.rule.spec) === 0 && rule.order > prev.rule.order))))
          declared.set(d.prop, { rule, value: d.value })
      }
    }
    const style = (node as HTMLElement).style
    if (style)
      for (let i = 0; i < style.length; i++) {
        const name = style[i]
        if (name.startsWith('--')) declared.set(name, { rule: null, value: style.getPropertyValue(name) })
      }
    let tokens = parent.tokens
    if (declared.size) {
      const own = new Map(parent.tokens)
      for (const [k, { value }] of declared) own.set(k, value)
      tokens = new Map(parent.tokens)
      for (const k of declared.keys()) tokens.set(k, expandWith(own, own.get(k)!))
    }
    // font-size: the winner of font-size / the `font` shorthand, then an inline font-size
    let win: { value: string; rule: Rule } | null = null
    for (const rule of cands) {
      const d = rule.decls.filter((x) => x.prop === 'font-size' || x.prop === 'font').pop()
      if (!d) continue
      const beats =
        !win ||
        (important(d.value) !== important(win.value)
          ? important(d.value)
          : cmpSpec(rule.spec, win.rule.spec) > 0 || (cmpSpec(rule.spec, win.rule.spec) === 0 && rule.order > win.rule.order))
      if (beats) win = { value: d.value, rule }
    }
    let value = win ? bare(win.value) : null
    if (style?.fontSize && !(win && important(win.value))) value = style.fontSize
    let px = parent.px
    if (value && value !== 'inherit' && value !== 'unset') {
      if (value === 'initial') px = 16
      else if (value === 'smaller') px = parent.px / 1.2
      else if (value === 'larger') px = parent.px * 1.2
      else {
        try {
          const v = expandWith(tokens, value)
          const n = evalPx(v, { em: parent.px, rem: remPx })
          if (Number.isFinite(n) && n > 0) px = n
        } catch {
          // invalid at computed-value time: font-size behaves as `unset`, i.e. inherits
        }
      }
    }
    const me = { tokens, px }
    out.set(node, me)
    for (const child of Array.from(node.children)) visit(child, at, me)
  }
  visit(document.body, [], { tokens: rootTokens, px: remPx })
  return out
}

// ── The boxes, with data ───────────────────────────────────────────────────────────────────

const band = (b: string, tier: BandReport['tier'], modeled: 'Open' | 'Marginal' | 'Closed', score: number): BandReport => ({
  band: b,
  tier,
  score,
  nHearMe: 4,
  nIHear: 7,
  bestRegion: { region: 'Europe', octant: 'NE', bearingDeg: 45, stations: 6, bidirectional: true },
  confidence: 'Likely',
  reason: 'Stations near you hear Europe',
  modeled,
  modeledReason: 'open per model',
})
const hourly = (peak: number) => Array.from({ length: 24 }, (_, h) => Math.max(0, peak - Math.abs(h - 14) / 20))
const outlookBand = (b: string, workability: string, score: number) => ({
  band: b,
  workability,
  score,
  window: '1400–1800Z',
  grayline: b === '40m',
  hourly: hourly(score),
  reliability: 70,
  modeNow: [
    { mode: 'FT8', score: 0.8 },
    { mode: 'CW', score: 0.5 },
    { mode: 'SSB', score: 0.2 },
  ],
})

const PROP = {
  advisory: {
    headline: '20m and 15m are open to Europe right now',
    bands: [band('20m', 'Active', 'Open', 0.8), band('15m', 'Moderate', 'Open', 0.6), band('40m', 'Quiet', 'Marginal', 0.3), band('10m', 'Closed', 'Closed', 0.05)],
    banners: ['Kp 5 — the polar paths are disturbed'],
  },
  worldwide: { headline: '', bands: [band('20m', 'Active', 'Open', 0.9)], banners: [] },
  openings: [
    { band: '6m', mode: 'Sporadic-E', octant: 'SE', bearingDeg: 140, maxKm: 1850, probability: 0.7, stations: 9, confidence: 'Likely', confidenceScore: 0.7, reciprocalPairs: 3, anomalyZ: 4.2, onsetSecs: 600, isNew: false, note: 'Es patch over the Gulf' },
    { band: '10m', mode: 'F2', octant: 'NE', bearingDeg: 60, maxKm: 7400, probability: 0.5, stations: 14, confidence: 'Strong', confidenceScore: 0.9, reciprocalPairs: 5, anomalyZ: 3.1, onsetSecs: 1800, isNew: true, note: '' },
  ],
  dxpeditions: {
    workableNow: [
      { call: 'VP8PJ', entity: 'South Orkney Is.', need: 'Atno', band: '20m', bearingDeg: 160, octant: 'S', distanceKm: 13000, status: 'WorkNow', likelihood: 'Good', likelihoodScore: 0.7, liveConfirmed: true, howToCall: 'Fox/Hound on 14.080', ft8Mode: 'FoxHound', windowHint: '1400–1600Z', priority: 1, modes: ['FT8', 'CW'] },
    ],
    active: ['VP8PJ'],
    upcoming: [],
  },
  spaceWx: { sfi: 142, kp: 5, aIndex: 18, xrayClass: 'C2.4', flare: false, xrayLong: 2.4e-6, solarWind: { bzNt: -6.1, btNt: 9.4, speedKms: 612, density: 7.2 } },
  source: 'live',
  asOf: fx.NOW - 60,
  spots: [
    { call: 'DL1ABC', lat: 50, lon: 8, band: '20m', heardMe: true, ageSecs: 90, approx: false, freqMhz: 14.074, mode: 'FT8', entity: 'Germany', cqZone: 14 },
    { call: 'JA1XYZ', lat: 35, lon: 139, band: '15m', heardMe: false, ageSecs: 400, approx: true, freqMhz: null, mode: 'CW', entity: 'Japan', cqZone: 25 },
  ],
  wxTrend: {
    sfi: { now: 142, deltaPerHr: 0.5, dir: 'rising' },
    kp: { now: 5, deltaPerHr: 0.4, dir: 'rising' },
    muf: { now: 24, deltaPerHr: 1.2, dir: 'rising' },
    xray: { now: 2.4e-6, deltaPerHr: 0, dir: 'steady' },
    windowSecs: 3600,
    samples: 12,
  },
  insights: [
    { kind: 'mufTrend', level: 'good', plain: 'MUF building — 10m may open within the hour', technical: 'MUF 24 MHz, +1.2 MHz/h', band: '10m' },
    { kind: 'geomagnetic', level: 'caution', plain: 'Kp 5: polar paths are noisy', technical: 'Kp 5, A 18' },
  ],
  bestToRegion: [
    { region: 'Europe', octant: 'NE', bearingDeg: 45, band: '20m', tier: 'Active', modeled: 'Open', stations: 6, bidirectional: true, score: 0.8 },
    { region: 'Japan', octant: 'NW', bearingDeg: 320, band: '15m', tier: 'Quiet', modeled: 'Open', stations: 2, bidirectional: false, score: 0.4 },
  ],
  regionBand: [
    { region: 'Europe', band: '20m', stations: 6, hearMe: 3, iHear: 4 },
    { region: 'Europe', band: '15m', stations: 2, hearMe: 1, iHear: 1 },
    { region: 'Japan', band: '15m', stations: 2, hearMe: 0, iHear: 2 },
  ],
} as unknown as PropagationSnapshot

const BAND_OUTLOOK = {
  engine: 'p533',
  bands: [outlookBand('20m', 'Good', 0.7), outlookBand('15m', 'Fair', 0.45), outlookBand('40m', 'Excellent', 0.9)],
  mufNow: 24.1,
  mufHourly: hourly(24),
}

const CTX: PaneContext = {
  myGrid: 'EN52',
  entityCentroids: null,
  units: 'metric',
  theme: 'dark',
  intent: 'dx',
  prop: PROP,
  prov: { label: 'LIVE', cls: 'live' },
  needByCall: new Map([['VP8PJ', 'NewEntity']]),
  needAlerts: [
    { call: 'VP8PJ', entity: 'South Orkney Is.', band: '20m', zone: 13, tags: ['NewEntity'], priority: 1, headline: 'ATNO on 20m', mode: 'Digital', freqMhz: 14.08, admittedAt: fx.NOW - 120, evidence: 'heard by K9LC (EN52, 26 km)' },
  ],
  selectedCall: 'DL1ABC',
  selStation: null,
  selSpot: (PROP.spots ?? [])[0],
  selDxped: null,
  selDxpedWindow: null,
  dxpedWindows: new Map([['VP8PJ', { call: 'VP8PJ', engine: 'p533', best: '20m Good 1400–1600Z', outlook: [outlookBand('20m', 'Good', 0.7)] }]]),
  selGrid: 'JO40',
  pathPred: BAND_OUTLOOK,
  bandOutlook: BAND_OUTLOOK,
  pathOpen: BAND_OUTLOOK.bands,
  outlookOpen: BAND_OUTLOOK.bands,
  getout: {
    count: 3,
    maxKm: 7400,
    reports: [
      { call: 'G4XYZ', grid: 'IO91', band: '20m', snr: -12, bearingDeg: 50, km: 6300, octant: 'NE', ageSecs: 120 },
      { call: 'EA7ABC', grid: 'IM76', band: '20m', snr: -3, bearingDeg: 70, km: 7400, octant: 'E', ageSecs: 300 },
      { call: 'W6XX', grid: 'CM87', band: '15m', snr: null, bearingDeg: 270, km: 2800, octant: 'W', ageSecs: 600 },
    ],
  },
  focusBand: null,
  amp: {
    family: 'spe',
    model: '13K',
    linked: true,
    reason: '',
    operate: true,
    transmitting: false,
    outputWatts: 850,
    bandLabel: '20m',
    swr: 1.4,
    swrAtu: 1.2,
    volts: 48.2,
    amps: 22.5,
    temp: 41,
    tempCelsius: false,
    alarm: 'none',
    alarmActive: false,
    warning: '',
  } as unknown as PaneContext['amp'],
  rigBand: '20m',
  scales: { r: 1, s: 0, g: 2, gTomorrow: 1, asOf: fx.NOW - 300 },
  alerts: [{ productId: 'K05W', issued: fx.NOW - 3600, kind: 'warning', message: 'Geomagnetic K-index of 5 expected' }],
  muf: [
    { lat: 40, lon: -105, mufMhz: 21.4, fof2Mhz: 6.1, ageSecs: 600, confidence: 0.8 },
    { lat: 51, lon: 0, mufMhz: 18.2, fof2Mhz: 5.2, ageSecs: 900, confidence: 0.6 },
  ],
  onSelectCall: () => {},
  onWorkSpot: () => {},
  onPoint: () => {},
  toggleFocusBand: () => {},
}

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  // MiniSpectrum draws on a canvas; jsdom has none (the pane's text is what is measured).
  HTMLCanvasElement.prototype.getContext = (() => null) as unknown as HTMLCanvasElement['getContext']
})
afterEach(cleanup)

/** One box, rendered exactly where ConnectView renders it: the Connect grid's left rail. */
async function renderBox(paneId: PaneId, textScale: number, ctx: PaneContext = CTX) {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(
      <div className="app">
        <main className="layout single">
          <div className="connect-shell">
            <div className="connect" data-rails="both">
              <div className="connect-rail" data-side="left">
                <PaneFrame
                  slotId="left1"
                  paneId={paneId}
                  ctx={ctx}
                  onAssign={() => {}}
                  share={1}
                  onHide={() => {}}
                  textScale={textScale}
                  onTextScale={() => {}}
                />
              </div>
            </div>
          </div>
        </main>
      </div>,
    )
  })
  // Self-fetching boxes answer on a later turn; let every mocked fetch land.
  for (let i = 0; i < 4; i++) await act(async () => {})
  return r
}

/** The elements in the box body that carry text of their own (not inside a drawing). */
function wordsIn(container: HTMLElement): Element[] {
  const body = container.querySelector('.pane-body')!
  return [...body.querySelectorAll('*')].filter(
    (el) => !el.closest('svg') && [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && /\S/.test(n.textContent ?? '')),
  )
}
const describeEl = (el: Element) => `${el.tagName.toLowerCase()}${[...el.classList].map((c) => '.' + c).join('')} "${(el.textContent ?? '').trim().slice(0, 24)}"`

interface Reading {
  what: string
  stock: number
  at: Record<string, number>
}

async function readBox(paneId: PaneId): Promise<{ expert: boolean; readings: Reading[] }> {
  const at: Record<string, Map<number, number>> = {}
  let whats: string[] = []
  let stock: number[] = []
  let expert = false
  for (const [label, factor, app] of [
    ['80', 0.8, 1],
    ['100', 1, 1],
    ['160', 1.6, 1],
    ['160 on Larger', 1.6, 1.25],
  ] as const) {
    const { container } = await renderBox(paneId, factor)
    const words = wordsIn(container)
    const sizes = fontSizes(WITH, app)
    at[label] = new Map(words.map((w, i) => [i, sizes.get(w)!.px]))
    if (label === '100') {
      const before = fontSizes(WITHOUT, 1)
      stock = words.map((w) => before.get(w)!.px)
      whats = words.map(describeEl)
      expert = !container.querySelector('.pane-body > .pane-basic')
    } else if (words.length !== whats.length && whats.length) {
      throw new Error(`${paneId}: ${words.length} words at ${label} vs ${whats.length} at 100 % — the box rendered differently`)
    }
    cleanup()
  }
  return {
    expert,
    readings: whats.map((what, i) => ({ what, stock: stock[i], at: Object.fromEntries(Object.entries(at).map(([k, m]) => [k, m.get(i)!])) })),
  }
}

const near = (a: number, b: number) => Math.abs(a - b) < 1e-6

describe('a box’s text size reaches every word in it', () => {
  it('the resolver reads the sheet it claims to (control: <body> and a box’s one-line hint at 100 %)', async () => {
    // Positive controls for the resolver itself, on sizes the sheet states outright: `body` is
    // calc(14px * var(--text-scale)), so the app's own Text size must move it with no box involved;
    // a box's one-line hint (`.pane-basic`) is var(--fs-label), 12 px at Normal.
    expect(fontSizes(WITH, 1).get(document.body)!.px).toBeCloseTo(14, 6)
    expect(fontSizes(WITH, 1.25).get(document.body)!.px).toBeCloseTo(17.5, 6)
    // Conditions with no snapshot yet is always its hint line (its panel returns null).
    const { container } = await renderBox('advisory', 1, { ...CTX, prop: null })
    const hint = container.querySelector('.pane-body > .pane-basic')
    expect(hint, 'control: the hint line rendered').not.toBeNull()
    expect(fontSizes(WITH, 1).get(hint!)!.px).toBeCloseTo(12, 6)
  })

  it('every box, rendered with data: 80 %, 100 % and 160 % are 0.8×, 1× and 1.6× the size its words had before', async () => {
    const wrong: string[] = []
    let words = 0
    const full: string[] = []
    for (const p of PANES) {
      const { expert, readings } = await readBox(p.id)
      if (expert) full.push(p.id)
      words += readings.length
      for (const r of readings) {
        const want: Record<string, number> = { '80': 0.8 * r.stock, '100': r.stock, '160': 1.6 * r.stock, '160 on Larger': 1.6 * 1.25 * r.stock }
        const bad = Object.entries(want).filter(([k, v]) => !near(r.at[k], v))
        if (bad.length)
          wrong.push(`${p.id} ${r.what}: before ${r.stock}px → ${bad.map(([k, v]) => `${k}: ${r.at[k].toFixed(2)}px (want ${v.toFixed(2)})`).join(', ')}`)
      }
    }
    // A fixture that silently rendered every box empty would pass the assertion below with nothing
    // checked: the census must have seen real panels and real words.
    expect(full.length, `boxes that rendered their full panel: ${full.join(', ')}`).toBeGreaterThanOrEqual(20)
    expect(words, 'words measured').toBeGreaterThan(150)
    expect(wrong, 'these words do not follow the box’s A− / A+').toEqual([])
  }, 60_000)
})

// @vitest-environment jsdom
//
// DARK DISPLAYS IN BOTH THEMES — the operator's pick of 2026-09-26: "Light frame, dark displays"
// (Zeus's "day shack": a light chassis with dark display wells). The instruments — the S-meters,
// the transmit meters, the audio level meter, CW's zero-beat bar, the VFO readout, the
// waterfall's stage and axis strip, the Fast Graph, the mini spectrum — stay dark in the light
// theme, so a signal reads the same wherever the operator is.
//
// THE TRAP THIS GUARDS. A dark surface in the light theme is not enough on its own: the light
// theme's inks are tuned for WHITE (light `--snr-weak` is #a20003), and on a near-black well they
// vanish. So a `.well` is the dark palette itself — it joins the dark theme's token blocks as a
// selector — and everything painted inside one resolves to the dark theme's values in every mode.
// Joining the blocks, rather than copying their hexes, means a retuned dark ink retunes the wells
// too; the fidelity suite below is what proves the join holds.
//
// Every assertion is a computed relationship on the cascade winner, in dark, light, dark-high and
// light-high, measured on the elements the components actually render (the chains are read off
// the DOM, never written by hand).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { createElement, type ReactElement } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { SMeter } from './components/SMeter'
import { TxMeters } from './components/TxMeters'
import { LevelMeter } from './components/LevelMeter'
import { CockpitHeader } from './components/CockpitHeader'
import { Waterfall } from './components/Waterfall'
import { FastGraph } from './components/FastGraph'
import { MiniSpectrum } from './components/MiniSpectrum'
import { PhoneScope } from './components/PhoneScope'
import { ZeroBeat } from './components/ZeroBeat'
import {
  MODES,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
} from './cssCascade'
import type { AppSnapshot, RadioStatus } from './types'

vi.mock('./api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.0.0')),
  haltTx: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
  getSpectrumRow: vi.fn(() => new Promise(() => {})),
  getScopeRow: vi.fn(() => new Promise(() => {})),
  getFastPower: vi.fn(() => new Promise(() => {})),
  getMeters: vi.fn(() => new Promise(() => {})),
}))

// jsdom implements neither; the scopes read both on mount.
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver
window.matchMedia = ((q: string) =>
  ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia

afterEach(cleanup)

// The desktop's two sheets, in main.tsx's order, comments blanked in place. Read from cwd as
// the other jsdom suites do (under jsdom Vite rewrites a literal `new URL(…, import.meta.url)`).
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
const ROOT = Object.fromEntries(MODES.map((m) => [m, rootTokensFrom(RULES, m)])) as Record<Mode, Map<string, string>>
const rootValue = (mode: Mode, token: string) => expandWith(ROOT[mode], `var(${token})`).trim()

/** Which palette a well must show in `mode`: always the DARK theme, at the mode's contrast and
 *  with the mode's colour-role presets (a well shows a preset's dark value in either theme). */
const darkTwin = (mode: Mode): Mode => mode.replace(/^light/, 'dark') as Mode

/** The status inks the scope must re-declare. `--state-*` are listed alongside `--snr-*`
 *  because the aliases are computed on <html>: a descendant that re-declares only the target
 *  still inherits the light answer. */
const STATUS = [
  '--snr-strong',
  '--snr-marginal',
  '--snr-weak',
  '--state-good',
  '--state-ok',
  '--state-weak',
  '--state-pending',
  '--alert-critical',
  '--alert-warning',
  '--alert-info',
  '--tx',
  '--rx',
] as const
/** The inks the labels and values inside a well are drawn in. */
const TEXT = ['--text', '--text-dim', '--text-faint'] as const
/** Everything a well must carry over from the dark theme: the inks above, and the surfaces,
 *  accent and focus ring the contents paint with (segment faces, the readout's input, a digit's
 *  focus ring, the waterfall's legend chip). */
const ISLAND = [
  ...STATUS,
  ...TEXT,
  '--bg',
  '--bg-elev',
  '--bg-elev-2',
  '--panel',
  '--border',
  '--border-soft',
  '--accent',
  '--accent-ink',
  '--focus-ring',
  '--readout',
] as const

// ── The well tokens ─────────────────────────────────────────────────────────────────────────

describe('the well tokens', () => {
  const WELL = ['--well-bg', '--well-ink', '--well-grid'] as const

  it.each(WELL)('%s is declared, with one value in every mode', (tok) => {
    const values = MODES.map((m) => rootValue(m, tok))
    expect(values[0], `${tok} is not declared`).not.toBe('')
    for (const [i, m] of MODES.entries()) {
      expect(ROOT[m].has(tok), `${tok} resolves in no ${m} block`).toBe(true)
      expect(values[i], `${tok} differs in ${m}`).toBe(values[0])
    }
  })

  it('declares them in BOTH theme blocks, not only on :root (the contract: both themes)', () => {
    for (const theme of ['dark', 'light'] as const) {
      const blocks = RULES.filter((r) => r.selector === `[data-theme='${theme}']`)
      for (const tok of WELL) {
        expect(
          blocks.some((r) => r.decls.some((d) => d.prop === tok)),
          `${tok} is not declared under [data-theme='${theme}']`,
        ).toBe(true)
      }
    }
  })

  it.each(MODES)('--well-ink reads 7:1 on --well-bg in %s', (mode) => {
    const bg = toRgb(rootValue(mode, '--well-bg'), [0, 0, 0])
    const ink = toRgb(rootValue(mode, '--well-ink'), bg ?? [0, 0, 0])
    expect(bg, '--well-bg did not compute').not.toBeNull()
    expect(ink, '--well-ink did not compute').not.toBeNull()
    const ratio = contrast(ink!, bg!)
    expect(ratio, `--well-ink ${hex(ink!)} on --well-bg ${hex(bg!)} = ${ratio.toFixed(2)}:1 in ${mode}`).toBeGreaterThanOrEqual(7)
  })

  it.each(MODES)('every status ink reads 3:1 on --well-bg in %s (measured inside a well)', (mode) => {
    const inside = tokensAt(RULES, mode, [{ tag: 'div', classes: ['well'], attrs: {} }])
    // Undeclared, --well-bg would expand to nothing and `toRgb` would hand back the backdrop —
    // a pass measured against a colour nobody chose.
    expect(expandWith(inside, 'var(--well-bg)').trim(), '--well-bg is not declared').not.toBe('')
    const bg = toRgb(expandWith(inside, 'var(--well-bg)'), [0, 0, 0])
    expect(bg, '--well-bg did not compute').not.toBeNull()
    for (const tok of STATUS) {
      const c = toRgb(expandWith(inside, `var(${tok})`), bg!)
      expect(c, `${tok} did not compute inside a well`).not.toBeNull()
      const ratio = contrast(c!, bg!)
      expect(ratio, `${tok} ${hex(c!)} on --well-bg ${hex(bg!)} = ${ratio.toFixed(2)}:1 in ${mode}`).toBeGreaterThanOrEqual(3)
    }
  })
})

// ── The instruments, rendered ──────────────────────────────────────────────────────────────

const RADIO = {
  dialMhz: 14.2,
  band: '20m',
  sideband: 'USB',
  catOk: true,
  txEnabled: true,
  txAllowed: true,
  tuning: false,
  smeterDb: -12,
  txSwr: 1.3,
  swrScaleVerified: true,
  txAlc: 0.5,
  txPoW: 90,
  txCompDb: 10,
  ratedWatts: 100,
}
/** REQUIRED, never defaulted: keyed is what decides which meters render. */
const radio = (keyed: boolean) => ({ ...RADIO, transmitting: keyed }) as unknown as RadioStatus

interface Instrument {
  name: string
  mount: () => ReactElement
  /** The display element — it must BE a well. */
  display: string
  /** Something painted INSIDE the well whose colour is a status ink, measured on the well. */
  fills?: string
}

const INSTRUMENTS: Instrument[] = [
  { name: 'the S-meter', mount: () => createElement(SMeter, { radio: radio(false) }), display: '.ph-smeter-track', fills: '.ph-smeter-seg.lit' },
  {
    name: 'the TX meters (Phone/CW dock)',
    mount: () => createElement(TxMeters, { radio: radio(true), pinned: true }),
    display: '.ph-txmeter-track',
    fills: '.ph-txmeter-fill',
  },
  {
    name: "the TX meters (Operate's strip cell)",
    mount: () => createElement(TxMeters, { radio: radio(true), inline: true }),
    display: '.ph-txmeter-track',
    fills: '.ph-txmeter-fill',
  },
  { name: 'the RX audio level meter', mount: () => createElement(LevelMeter, { value: 0.5 }), display: '.level-meter', fills: '.level-fill' },
  {
    name: 'the VFO readout',
    mount: () =>
      createElement(CockpitHeader, {
        snap: { radio: radio(false) } as unknown as AppSnapshot,
        modeIndicator: 'USB',
        bandControl: '—',
      }),
    display: '.ch-readout',
  },
  {
    name: "the waterfall's stage (axis strip, markers, legend)",
    mount: () => createElement(Waterfall, { transmitting: false, rxOffsetHz: 1500, txOffsetHz: 1000, theme: 'light' }),
    display: '.wf-stage',
  },
  { name: 'the Fast Graph', mount: () => createElement(FastGraph, { periodS: 15, theme: 'light' }), display: '.fastgraph-wrap' },
  { name: 'the mini spectrum', mount: () => createElement(MiniSpectrum, {}), display: '.mini-spectrum-canvas' },
  {
    name: "the scope's S-meter strip",
    mount: () => createElement(PhoneScope, { transmitting: false, theme: 'light', smeterDb: -12 }),
    display: '.ph-scope-smeter-track',
    fills: '.ph-scope-smeter-fill',
  },
  { name: "CW's zero-beat bar", mount: () => createElement(ZeroBeat, { targetHz: 600 }), display: '.zb-bar' },
]

interface Rendered {
  display: El[]
  /** Each painted fill inside the well: its chain and its inline `background`, if any. */
  fills: { chain: El[]; inline: string }[]
}

function rendered(i: Instrument): Rendered {
  const { container } = render(i.mount())
  const el = container.querySelector(i.display)
  expect(el, `${i.name}: ${i.display} did not render`).not.toBeNull()
  const fills = i.fills
    ? [...container.querySelectorAll(i.fills)].map((f) => ({
        chain: chainOf(f),
        inline: (f as HTMLElement).style.getPropertyValue('background') || (f as HTMLElement).style.background || '',
      }))
    : []
  const out = { display: chainOf(el!), fills }
  cleanup()
  return out
}

/** What the well element paints behind its contents, over the page. */
function surfaceOf(chain: El[], mode: Mode, tokens: Map<string, string>): Rgb {
  const page = toRgb(rootValue(mode, '--bg'), [255, 255, 255])!
  const panel = toRgb(rootValue(mode, '--panel'), page)!
  const w = winnerAt(RULES, mode, chain, 'background', 'background-color')
  const v = w ? expandWith(tokens, w.value) : 'transparent'
  // Nothing painted: the element shows whatever panel it sits on — the frame, not a well.
  const c = toRgb(v, panel)
  expect(c, `the display's background did not compute: ${v}`).not.toBeNull()
  return c!
}

const cases = INSTRUMENTS.flatMap((i) => MODES.map((m) => [i.name, m, i] as const))

describe('every instrument IS a well', () => {
  it.each(INSTRUMENTS.map((i) => [i.name, i] as const))('%s carries the well class', (_n, i) => {
    const { display } = rendered(i)
    expect(display[display.length - 1].classes, `${i.name} (${i.display}) is not a .well`).toContain('well')
  })
})

describe('what an instrument paints is dark, and what it paints on it reads', () => {
  it.each(cases)('%s in %s: its face is dark (the well ink reads 7:1 on it)', (_n, mode, i) => {
    const { display } = rendered(i)
    const tokens = tokensAt(RULES, mode, display)
    const face = surfaceOf(display, mode, tokens)
    const ink = toRgb(expandWith(tokens, 'var(--well-ink)'), face)
    expect(ink, `--well-ink did not compute inside ${i.name}`).not.toBeNull()
    const ratio = contrast(ink!, face)
    expect(ratio, `${i.name} in ${mode}: face ${hex(face)}, --well-ink ${hex(ink!)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(7)
  })

  it.each(cases)('%s in %s: every status ink reads 3:1 on its face', (_n, mode, i) => {
    const { display } = rendered(i)
    const tokens = tokensAt(RULES, mode, display)
    const face = surfaceOf(display, mode, tokens)
    for (const tok of STATUS) {
      const c = toRgb(expandWith(tokens, `var(${tok})`), face)
      expect(c, `${tok} did not compute inside ${i.name}`).not.toBeNull()
      const ratio = contrast(c!, face)
      expect(ratio, `${i.name} in ${mode}: ${tok} ${hex(c!)} on ${hex(face)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(3)
    }
  })

  it.each(cases)('%s in %s: its labels read 4.5:1 on its face', (_n, mode, i) => {
    const { display } = rendered(i)
    const tokens = tokensAt(RULES, mode, display)
    const face = surfaceOf(display, mode, tokens)
    for (const tok of TEXT) {
      const c = toRgb(expandWith(tokens, `var(${tok})`), face)
      expect(c, `${tok} did not compute inside ${i.name}`).not.toBeNull()
      const ratio = contrast(c!, face)
      expect(ratio, `${i.name} in ${mode}: ${tok} ${hex(c!)} on ${hex(face)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5)
    }
  })

  const withFills = INSTRUMENTS.filter((i) => i.fills).flatMap((i) => MODES.map((m) => [i.name, m, i] as const))
  it.each(withFills)('%s in %s: the level it lights is a dark-theme ink, 3:1 on the face', (_n, mode, i) => {
    const { display, fills } = rendered(i)
    expect(fills.length, `${i.name} lit nothing to measure`).toBeGreaterThan(0)
    const face = surfaceOf(display, mode, tokensAt(RULES, mode, display))
    for (const f of fills) {
      const tokens = tokensAt(RULES, mode, f.chain)
      const w = winnerAt(RULES, mode, f.chain, 'background', 'background-color')
      // An inline style outranks the sheet (TxMeters and the scope strip set the zone colour so).
      const v = expandWith(tokens, f.inline || (w ? w.value : 'transparent'))
      const c = toRgb(v, face)
      expect(c, `${i.name}: the fill did not compute (${v})`).not.toBeNull()
      const ratio = contrast(c!, face)
      expect(ratio, `${i.name} in ${mode}: fill ${hex(c!)} on ${hex(face)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(3)
    }
  })
})

describe('a well is the dark theme, in every mode (the join holds)', () => {
  // The well joins the dark palette blocks rather than copying their values. What proves the
  // join is that every token it must carry resolves INSIDE a well to exactly the value the
  // dark theme resolves at <html> — at the mode's own contrast, so field mode's lifted inks
  // reach the wells too.
  it.each(cases)('%s in %s', (_n, mode, i) => {
    const { display } = rendered(i)
    const tokens = tokensAt(RULES, mode, display)
    const twin = darkTwin(mode)
    const off = ISLAND.filter((tok) => expandWith(tokens, `var(${tok})`).trim() !== rootValue(twin, tok)).map(
      (tok) => `${tok}: ${expandWith(tokens, `var(${tok})`).trim()} (the ${twin} theme says ${rootValue(twin, tok)})`,
    )
    expect(off, `${i.name} in ${mode} is not the ${twin} palette:\n${off.join('\n')}`).toEqual([])
  })
})

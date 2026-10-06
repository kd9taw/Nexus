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
// light-high and their Night twins (where a well is the dark + Night palette, dimmed with the
// room), measured on the elements the components actually render (the chains are read off the
// DOM, never written by hand).
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
  isNight,
  parseRules,
  rgbHex as hex,
  rolesOf,
  rootTokensFrom,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
} from './cssCascade'
import { SKINS } from './features/skins'
import { NEED_CHIP } from './features/needVisuals'
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

/** The built-in theme (features/skins.ts) a mode carries, if any. */
const skinOf = (mode: Mode) => SKINS.find((s) => s.id === rolesOf(mode).skin)
/** Which palette a well must show in `mode`: always the DARK theme, at the mode's contrast and
 *  with the mode's colour-role presets (a well shows a preset's dark value in either theme). A
 *  dark built-in theme IS a dark theme, so its wells are itself; a light one's are the standard
 *  dark palette, less the accent and readout it gives them (`ownInWell`). */
const darkTwin = (mode: Mode): Mode => {
  const dark = mode.replace(/^light/, 'dark')
  return (skinOf(mode)?.base === 'light' ? dark.replace(/ skin=[\w-]+/, '') : dark) as Mode
}
/** What a light theme paints in its wells itself, held to its table by styles-skins.test.ts. */
const ownInWell = (mode: Mode): string[] => Object.keys(skinOf(mode)?.well ?? {})

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
/** Every colour a spot tag's mark can take on the Phone and CW scopes (`spectrum/scopeSpots.ts`
 *  `spotInk`): each need's, and the dim POTA colour. Read from the need table, so a need added there
 *  is measured here the day it lands. */
const NEED_MARKS = [...new Set(Object.values(NEED_CHIP).map((c) => `--need-${c.cls}`)), '--pota-dim']
/** Everything a well must carry over from the dark theme: the inks above, and the surfaces,
 *  accent and focus ring the contents paint with (segment faces, the readout's input, a digit's
 *  focus ring, the waterfall's legend chip), and the need set the scope's spot tags are marked in. */
const ISLAND = [
  ...STATUS,
  ...TEXT,
  ...NEED_MARKS,
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

  it.each(WELL)('%s is declared, with one value in every mode of each Night setting', (tok) => {
    // One value in both themes, at every contrast and under every colour role: a well looks the
    // same wherever the operator is. Night dims the wells with the room (styles.css NIGHT), so
    // there are exactly two values, the day one and the night one. A dark built-in theme's wells
    // are the theme, so it has its own two (styles-skins.test.ts holds them to its table); a light
    // one's are the standard ones.
    const groupOf = (m: Mode) => (skinOf(m)?.base === 'dark' ? skinOf(m)!.id : '')
    for (const group of new Set(MODES.map(groupOf))) {
      for (const night of [false, true]) {
        const modes = MODES.filter((m) => isNight(m) === night && groupOf(m) === group)
        const values = modes.map((m) => rootValue(m, tok))
        expect(values[0], `${tok} is not declared`).not.toBe('')
        for (const [i, m] of modes.entries()) {
          expect(ROOT[m].has(tok), `${tok} resolves in no ${m} block`).toBe(true)
          expect(values[i], `${tok} differs in ${m}`).toBe(values[0])
        }
      }
    }
  })

  it('declares them in BOTH theme blocks, by day and at night, not only on :root (the contract: both themes)', () => {
    for (const theme of ['dark', 'light'] as const) {
      for (const sel of [`[data-theme='${theme}']`, `[data-theme='${theme}'][data-night='1']`]) {
        const blocks = RULES.filter((r) => r.selector === sel)
        for (const tok of WELL) {
          expect(
            blocks.some((r) => r.decls.some((d) => d.prop === tok)),
            `${tok} is not declared under ${sel}`,
          ).toBe(true)
        }
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

/** `tokensAt` once per mode and chain: several suites below resolve the same element in the
 *  same mode, and with Night the modes doubled. Pure — the rules never change in this file. */
const RESOLVED = new Map<string, Map<string, string>>()
function tokensOf(mode: Mode, chain: El[]): Map<string, string> {
  const key = `${mode}|${JSON.stringify(chain)}`
  let out = RESOLVED.get(key)
  if (!out) RESOLVED.set(key, (out = tokensAt(RULES, mode, chain)))
  return out
}

/** Each instrument is rendered ONCE and its chains kept: they are read off the real DOM, and they
 *  do not depend on the mode — only the cascade resolved over them does. Rendered per case, the
 *  32 modes Night brought (8 base × the colour-role sets) cost over a thousand renders. */
const RENDERED = new Map<Instrument, Rendered>()

function rendered(i: Instrument): Rendered {
  const hit = RENDERED.get(i)
  if (hit) return hit
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
  RENDERED.set(i, out)
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
    const tokens = tokensOf(mode, display)
    const face = surfaceOf(display, mode, tokens)
    const ink = toRgb(expandWith(tokens, 'var(--well-ink)'), face)
    expect(ink, `--well-ink did not compute inside ${i.name}`).not.toBeNull()
    const ratio = contrast(ink!, face)
    expect(ratio, `${i.name} in ${mode}: face ${hex(face)}, --well-ink ${hex(ink!)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(7)
  })

  it.each(cases)('%s in %s: every status ink reads 3:1 on its face', (_n, mode, i) => {
    const { display } = rendered(i)
    const tokens = tokensOf(mode, display)
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
    const tokens = tokensOf(mode, display)
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
    const face = surfaceOf(display, mode, tokensOf(mode, display))
    for (const f of fills) {
      const tokens = tokensOf(mode, f.chain)
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

describe("the scope's spot tags: every need mark reads on the scope's own floor", () => {
  // The Phone and CW scopes draw their spot tags on the overlays canvas, a well that takes the dark
  // palette's inks and none of its background (the picture under it is the floor). A tag's mark is its
  // need's colour, read off that canvas (`readOverlayInks`), on the tag's ground, `--well-bg`. In the
  // light theme the need set once reached it from <html> — the light inks, made for white — and the
  // zone, SOTA and watch marks read under 3:1 on the dark floor.
  let chain: El[] | null = null
  const overlays = (): El[] => {
    if (chain) return chain
    const { container } = render(createElement(PhoneScope, { transmitting: false, theme: 'light' }))
    const el = container.querySelector('.ph-scope-overlays')
    expect(el, 'the overlays canvas did not render').not.toBeNull()
    chain = chainOf(el!)
    cleanup()
    return chain
  }

  it('the marks are the need table plus the dim POTA colour (the guard measures something)', () => {
    expect(NEED_MARKS).toEqual(expect.arrayContaining(['--need-entity', '--need-zone', '--need-pota', '--need-sota', '--need-watch', '--pota-dim']))
    expect(overlays()[overlays().length - 1].classes).toEqual(expect.arrayContaining(['ph-scope-overlays', 'well']))
  })

  it.each(MODES)('in %s, every need mark reads 3:1 on the floor', (mode) => {
    const tokens = tokensOf(mode, overlays())
    expect(expandWith(tokens, 'var(--well-bg)').trim(), '--well-bg is not declared').not.toBe('')
    const floor = toRgb(expandWith(tokens, 'var(--well-bg)'), [0, 0, 0])
    expect(floor, '--well-bg did not compute').not.toBeNull()
    const low = NEED_MARKS.flatMap((tok) => {
      const c = toRgb(expandWith(tokens, `var(${tok})`), floor!)
      if (!c) return [`${tok} did not compute`]
      const ratio = contrast(c, floor!)
      return ratio < 3 ? [`${tok} ${hex(c)} on ${hex(floor!)} = ${ratio.toFixed(2)}:1`] : []
    })
    expect(low, `in ${mode}`).toEqual([])
  })
})

describe('a well is the dark theme, in every mode (the join holds)', () => {
  // The well joins the dark palette blocks rather than copying their values. What proves the
  // join is that every token it must carry resolves INSIDE a well to exactly the value the
  // dark theme resolves at <html> — at the mode's own contrast, so field mode's lifted inks
  // reach the wells too.
  it.each(cases)('%s in %s', (_n, mode, i) => {
    const { display } = rendered(i)
    const tokens = tokensOf(mode, display)
    const twin = darkTwin(mode)
    const own = ownInWell(mode)
    const off = ISLAND.filter((tok) => !own.includes(tok) && expandWith(tokens, `var(${tok})`).trim() !== rootValue(twin, tok)).map(
      (tok) => `${tok}: ${expandWith(tokens, `var(${tok})`).trim()} (the ${twin} theme says ${rootValue(twin, tok)})`,
    )
    expect(off, `${i.name} in ${mode} is not the ${twin} palette:\n${off.join('\n')}`).toEqual([])
  })
})

// @vitest-environment jsdom
//
// ON AIR YOU CANNOT MISS — the operator's pick of 2026-09-26: "Solid red, white text". A steady,
// studio-style sign, the loudest thing on the screen in every theme, with no motion.
//
// Before this, the cockpit header said ON AIR in `--tx`-coloured TEXT and nothing else: no fill,
// no border, a word that looked like every other word in the header. The amber theme showed what
// that costs (ui/DESIGN.md, the amber lesson): when hue is the only thing carrying the state, a
// weak hue loses it. So the FORM carries it now — a filled pill with a rim — and the hue only
// has to agree.
//
// THREE annunciators say "the transmitter is keyed", one per kind of screen, and this measures
// all three from the elements the components actually render (the chain is read off the DOM,
// never written out by hand — a selector is only as good as its reach):
//   · the cockpit header's pill (Phone, CW, RTTY, PSK, SSTV, JS8 and the Tempo view);
//   · Operate's strip caption — Operate's header draws no pill (`txState={false}`), so the
//     strip's "▲ TRANSMITTING" is that cockpit's annunciator;
//   · the TopBar's TX/RX plate, on every screen that is not a cockpit.
//
// Every assertion is a computed relationship on the cascade WINNER in all four modes (dark,
// light, dark-high, light-high). Nothing pins a hex: re-tuning a colour is free, breaking the
// sign is not. This is presentation only — nothing here reads or changes what keys the rig.
//
// THE FILL (coordinator ruling, 2026-09-26: option (a)). White on `--tx` itself measures 4.00:1
// in the dark theme (#e64343), under the 4.5:1 an ink needs, and `--tx` is locked. So the fill is
// `--tx` taken 10% toward black — DERIVED from the token, which the counterfactual suite below
// pins: swap `--tx` for a colour no theme uses and every sign's fill must follow it.
// Measured on this sheet when written, by this file's own resolver (the assertions are the spec,
// these numbers are the margins it had):
//
//   mode        --tx     fill     ink/fill  fill/--panel  fill/--bg  rim/--panel  rim/--bg
//   dark        #e64343  #cf3c3c    4.81        3.73         3.99       15.26       16.30
//   light       #a50000  #950000    9.22        8.98         7.62       16.03       13.61
//   dark-high   #e64343  #cf3c3c    4.81        3.91         4.36       18.81       21.00
//   light-high  #a50000  #950000    9.22        9.22         7.27       21.00       16.56
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { createElement } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { CockpitTxStrip } from './components/CockpitTxStrip'
import { OperateQsoStrip } from './components/OperateQsoStrip'
import { TopBar } from './components/TopBar'
import {
  MODES,
  chainOf,
  compoundMatches,
  contrast,
  expandWith,
  parseRules,
  reachesChain,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'
import type { QsoStatus, RadioStatus } from './types'

// THE BUDGET (2026-10-09). The slowest case here, "Operate's strip caption stays quiet when not keyed: no…", takes
// 0.35 s and 0.32 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('./api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.0.0')),
  haltTx: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
}))

afterEach(cleanup)

// The desktop's two sheets in the order main.tsx imports them. Comments blanked in place:
// prose must never read as a declaration. Read from `process.cwd()` as the other jsdom suites do:
// under jsdom, Vite rewrites a literal `new URL('./x.css', import.meta.url)` into an asset URL.
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

// ── The three annunciators, rendered ──────────────────────────────────────────────────────

const RADIO = {
  dialMhz: 14.074,
  band: '20m',
  sideband: 'USB',
  slot: 0,
  source: 'native',
  sourceLabel: 'Native',
  nextSlotMs: 5000,
  rxOffsetHz: 1500,
  txOffsetHz: 1500,
  txLevel: 0.5,
  rxLevel: 0.3,
  txEven: true,
  txCycleAuto: true,
  txEnabled: true,
  txAllowed: true,
  tuning: false,
  holdTxFreq: false,
  qsoRecording: false,
  catOk: true,
  dtSec: 0,
  clockOffsetMs: 0,
}
/** The radio, keyed or not. REQUIRED, never defaulted: the keyed state is what is under test. */
const radioKeyed = (keyed: boolean) => ({ ...RADIO, transmitting: keyed }) as unknown as RadioStatus

/** The TX strip's state caption — the ON AIR sign of every screen but FT's since operator
 *  batch 60 (it was the cockpit header's TX pill, which moved into the strip with the latch). */
function txStripCaption(keyed: boolean): Element {
  const { container } = render(createElement(CockpitTxStrip, { radio: radioKeyed(keyed), onStopTx: () => {} }))
  return container.querySelector('.cq-statecap')!
}

function operateCaption(keyed: boolean): Element {
  const noop = () => {}
  const qso = { running: false, state: 'Idle', dxcall: null, txNow: null } as unknown as QsoStatus
  const { container } = render(
    createElement(OperateQsoStrip, {
      qso,
      radio: radioKeyed(keyed),
      onSetMode: noop,
      onCallCq: noop,
      onResend: noop,
      onFreetext: () => true,
      onLog: noop,
      onSetTxEnabled: noop,
      onSetTune: noop,
      onHaltTx: noop,
      onSetHoldTxFreq: noop,
    } as unknown as Parameters<typeof OperateQsoStrip>[0]),
  )
  return container.querySelector('.cq-statecap')!
}

function topBarPlate(keyed: boolean): Element {
  const noop = () => {}
  const { container } = render(
    createElement(TopBar, {
      mycall: 'KD9TAW',
      mygrid: 'EN61',
      radio: radioKeyed(keyed),
      link: { tier: 'FT8' },
      bandPlan: [],
      onSetFrequency: noop,
      onSetTxEnabled: noop,
      onSetTune: noop,
      onHaltTx: noop,
      onSetTxEven: noop,
      onSetTxCycleAuto: noop,
      onSetHoldTxFreq: noop,
      tier: 'FT8',
      onTierChange: noop,
      onOpenGuide: noop,
    } as unknown as Parameters<typeof TopBar>[0]),
  )
  return container.querySelector('.txrx-indicator')!
}

interface Sign {
  name: string
  /** The annunciator's element, keyed or not — rendered, then read back as a chain. */
  chain: (keyed: boolean) => El[]
  /** Its unkeyed state stays QUIET: words, not a pill. The TopBar's plate is exempt — its
   *  receive state has always been a tinted RX plate, and that is not what changes here. */
  quietWhenIdle: boolean
}

/** Each state rendered ONCE and its chain kept: the chain is read off the real DOM and does not
 *  depend on the mode, only the cascade resolved over it does — and with Night there are 32. */
const capture = (make: (keyed: boolean) => Element) => {
  const seen = new Map<boolean, El[]>()
  return (keyed: boolean) => {
    const hit = seen.get(keyed)
    if (hit) return hit
    const el = make(keyed)
    expect(el, 'the annunciator did not render').not.toBeNull()
    const chain = chainOf(el)
    cleanup()
    seen.set(keyed, chain)
    return chain
  }
}

const SIGNS: Sign[] = [
  { name: "the TX strip's caption", chain: capture(txStripCaption), quietWhenIdle: true },
  { name: "Operate's strip caption", chain: capture(operateCaption), quietWhenIdle: true },
  { name: "the TopBar's TX plate", chain: capture(topBarPlate), quietWhenIdle: false },
]

// ── What the cascade paints on one of them ────────────────────────────────────────────────

/** The surfaces the spec measures the sign against. */
const SURROUNDINGS = ['--panel', '--bg'] as const

function surface(tokens: Map<string, string>, token: string): Rgb {
  const page = toRgb(expandWith(tokens, 'var(--bg)'), [255, 255, 255])!
  const c = toRgb(expandWith(tokens, `var(${token})`), page)
  expect(c, `${token} did not resolve to a colour`).not.toBeNull()
  return c!
}

/** The colour inside a `border` shorthand (whatever is not a width or a style). */
const borderColourOf = (v: string) =>
  v
    .split(/\s+/)
    .filter((t) => !/^-?[\d.]+(px|r?em)?$/.test(t) && !/^(none|solid|dashed|dotted|double|hidden|inset|outset|groove|ridge)$/.test(t))
    .join(' ')
const borderWidthOf = (v: string) => v.split(/\s+/).find((t) => /^[\d.]+(px|r?em)?$/.test(t)) ?? '0'

interface Paint {
  fill: Rgb | null
  fillValue: string
  ink: Rgb
  rim: Rgb | null
  rimWidth: string
  tokens: Map<string, string>
}

/** `tokensAt` once per rule set, mode and chain: every suite below resolves the same sign in the
 *  same mode several times over. Keyed on the rule set too — the counterfactual swaps `--tx`. */
const RESOLVED = new WeakMap<Rule[], Map<string, Map<string, string>>>()
function tokensOf(rules: Rule[], mode: Mode, chain: El[]): Map<string, string> {
  let byKey = RESOLVED.get(rules)
  if (!byKey) RESOLVED.set(rules, (byKey = new Map()))
  const key = `${mode}|${JSON.stringify(chain)}`
  let out = byKey.get(key)
  if (!out) byKey.set(key, (out = tokensAt(rules, mode, chain)))
  return out
}

function paintOf(chain: El[], mode: Mode, backdrop: Rgb, rules: Rule[] = RULES): Paint {
  const tokens = tokensOf(rules, mode, chain)
  const x = (v: string) => expandWith(tokens, v)
  const fillW = winnerAt(rules, mode, chain, 'background', 'background-color')
  const fillValue = fillW ? x(fillW.value) : 'transparent'
  const fill = /^(transparent|none)?$/.test(fillValue.trim()) ? null : toRgb(fillValue, backdrop)
  // `color` inherits, so the ink is the nearest ancestor's winner when the element has none.
  let inkValue = 'var(--text)'
  for (let i = chain.length; i > 0; i--) {
    const w = winnerAt(rules, mode, chain.slice(0, i), 'color')
    if (w && w.value !== 'inherit') {
      inkValue = w.value
      break
    }
  }
  const ink = toRgb(x(inkValue), fill ?? backdrop)
  expect(ink, `ink did not compute: ${x(inkValue)}`).not.toBeNull()
  const rimW = winnerAt(rules, mode, chain, 'border', 'border-color')
  const widthW = winnerAt(rules, mode, chain, 'border', 'border-width')
  const rimValue = rimW ? (rimW.prop === 'border' ? borderColourOf(x(rimW.value)) || x(inkValue) : x(rimW.value)) : ''
  const rim = rimValue && !/^transparent$/.test(rimValue.trim()) ? toRgb(rimValue, backdrop) : null
  const rimWidth = widthW ? (widthW.prop === 'border' ? borderWidthOf(x(widthW.value)) : x(widthW.value)) : '0'
  return { fill, fillValue, ink: ink!, rim, rimWidth, tokens }
}

/** Hue in degrees (HSL), for "is this still the TX red". */
function hue([r, g, b]: Rgb): number {
  const [R, G, B] = [r / 255, g / 255, b / 255]
  const max = Math.max(R, G, B)
  const min = Math.min(R, G, B)
  if (max === min) return 0
  const d = max - min
  const h = max === R ? ((G - B) / d) % 6 : max === G ? (B - R) / d + 2 : (R - G) / d + 4
  return (h * 60 + 360) % 360
}
const hueGap = (a: number, b: number) => Math.min(Math.abs(a - b), 360 - Math.abs(a - b))

const TEXT_MIN = 4.5
const EDGE_MIN = 3

const cases = SIGNS.flatMap((s) => MODES.map((m) => [s.name, m, s] as const))

describe('ON AIR is a filled, rimmed sign in every mode', () => {
  it.each(cases)('%s in %s: the keyed state paints a fill', (_n, mode, s) => {
    const chain = s.chain(true)
    const p = paintOf(chain, mode, surface(tokensOf(RULES, mode, chain), '--panel'))
    expect(p.fill, `${s.name} keyed in ${mode} paints no fill (background: ${p.fillValue}) — text alone`).not.toBeNull()
  })

  it.each(cases)('%s in %s: the ink reads on the fill at 4.5:1', (_n, mode, s) => {
    const chain = s.chain(true)
    const p = paintOf(chain, mode, surface(tokensOf(RULES, mode, chain), '--panel'))
    expect(p.fill, `${s.name} paints no fill in ${mode}`).not.toBeNull()
    const ratio = contrast(p.ink, p.fill!)
    expect(ratio, `${s.name} in ${mode}: ink ${hex(p.ink)} on fill ${hex(p.fill!)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(TEXT_MIN)
  })

  it.each(cases)('%s in %s: the fill stands 3:1 off the panel and the page', (_n, mode, s) => {
    const chain = s.chain(true)
    const tokens = tokensOf(RULES, mode, chain)
    for (const around of SURROUNDINGS) {
      const bg = surface(tokens, around)
      const p = paintOf(chain, mode, bg)
      expect(p.fill, `${s.name} paints no fill in ${mode}`).not.toBeNull()
      const ratio = contrast(p.fill!, bg)
      expect(ratio, `${s.name} in ${mode}: fill ${hex(p.fill!)} on ${around} ${hex(bg)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(EDGE_MIN)
    }
  })

  it.each(cases)('%s in %s: a drawn rim holds 3:1 off the panel and the page', (_n, mode, s) => {
    // The rim is what makes the pill hold wherever the fill and its surroundings are close —
    // Operate's strip tints itself red while keyed, and a red fill on a red wash is an edge
    // nobody can see.
    const chain = s.chain(true)
    const tokens = tokensOf(RULES, mode, chain)
    for (const around of SURROUNDINGS) {
      const bg = surface(tokens, around)
      const p = paintOf(chain, mode, bg)
      expect(parseFloat(p.rimWidth), `${s.name} in ${mode}: no border width`).toBeGreaterThan(0)
      expect(p.rim, `${s.name} in ${mode}: the border is transparent or missing`).not.toBeNull()
      const ratio = contrast(p.rim!, bg)
      expect(ratio, `${s.name} in ${mode}: rim ${hex(p.rim!)} on ${around} ${hex(bg)} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(EDGE_MIN)
    }
  })

  it.each(cases)('%s in %s: the fill is the TX red (the colour stays locked)', (_n, mode, s) => {
    const chain = s.chain(true)
    const p = paintOf(chain, mode, surface(tokensOf(RULES, mode, chain), '--panel'))
    const tx = surface(p.tokens, '--tx')
    expect(p.fill, `${s.name} paints no fill in ${mode}`).not.toBeNull()
    const gap = hueGap(hue(p.fill!), hue(tx))
    expect(gap, `${s.name} in ${mode}: fill ${hex(p.fill!)} is ${gap.toFixed(0)}° off --tx ${hex(tx)}`).toBeLessThanOrEqual(10)
  })
})

describe('the fill is DERIVED from --tx: a change to --tx reaches every sign', () => {
  // The hue check above says the fill LOOKS like the TX red today; it cannot tell a derived fill
  // from a hard-coded hex that happens to match — and a hex would silently keep the old red the
  // day `--tx` is retuned, so the sign and the TX colour would part company. This pins the
  // derivation by counterfactual instead of by reading the declaration: every `--tx` in the
  // sheet is replaced with a colour no theme uses, and each sign's fill must MOVE, to that
  // colour's hue. The sentinel is a blue, as far from any red as the wheel allows.
  const SENTINEL = '#2080e0'
  const SWAPPED: Rule[] = RULES.map((r) => ({
    ...r,
    decls: r.decls.map((d) => (d.prop === '--tx' ? { ...d, value: SENTINEL } : d)),
  }))

  it('the swap reaches the sheet (the counterfactual is not a no-op)', () => {
    const swapped = SWAPPED.filter((r, i) => r.decls !== RULES[i].decls).length
    expect(swapped, 'no rule declares --tx — the swap changed nothing').toBeGreaterThan(0)
  })

  it.each(cases)('%s in %s follows --tx', (_n, mode, s) => {
    const chain = s.chain(true)
    const panel = surface(tokensOf(RULES, mode, chain), '--panel')
    const now = paintOf(chain, mode, panel)
    const moved = paintOf(chain, mode, panel, SWAPPED)
    expect(now.fill, `${s.name} paints no fill in ${mode}`).not.toBeNull()
    expect(moved.fill, `${s.name} paints no fill in ${mode} once --tx is ${SENTINEL}`).not.toBeNull()
    expect(hex(moved.fill!), `${s.name} in ${mode}: the fill did not move with --tx — a literal, not a derivation`).not.toBe(
      hex(now.fill!),
    )
    const gap = hueGap(hue(moved.fill!), hue(toRgb(SENTINEL, panel)!))
    expect(gap, `${s.name} in ${mode}: with --tx ${SENTINEL} the fill is ${hex(moved.fill!)}, ${gap.toFixed(0)}° off it`).toBeLessThanOrEqual(10)
  })
})

describe("Operate's caption holds on the strip's own keyed wash", () => {
  // The Operate strip is the one sign that does not sit on --panel while keyed: the strip
  // behind it pulses between two red washes (`.cockpit-qso.tx`'s animation — the whole strip
  // going loud is Operate's double-click feedback, and it is left as it is). Measured on the
  // real frames, read out of the sheet's @keyframes, so a re-tuned wash is measured too.
  const RAW = sheet('styles.css')
  function frames(name: string): string[] {
    const at = RAW.search(new RegExp(`@keyframes\\s+${name}\\s*\\{`))
    expect(at, `@keyframes ${name} not found`).toBeGreaterThan(-1)
    let i = RAW.indexOf('{', at) + 1
    const start = i
    for (let depth = 1; depth > 0; i++) depth += RAW[i] === '{' ? 1 : RAW[i] === '}' ? -1 : 0
    return [...RAW.slice(start, i).matchAll(/background(?:-color)?\s*:\s*([^;}]+)/g)].map((m) => m[1].trim())
  }

  it.each(MODES)('in %s the rim stands 3:1 off every frame of the wash', (mode) => {
    const chain = SIGNS.find((s) => s.name === "Operate's strip caption")!.chain(true)
    const strip = chain.slice(0, -1)
    expect(strip[strip.length - 1].classes, 'the caption is not inside the keyed strip').toEqual(
      expect.arrayContaining(['cockpit-qso', 'tx']),
    )
    const anim = winnerAt(RULES, mode, strip, 'animation', 'animation-name')
    expect(anim, 'the keyed strip no longer animates — drop this block with it').not.toBeNull()
    const name = anim!.value.split(/\s+/).find((w) => new RegExp(`@keyframes\\s+${w}\\s*\\{`).test(RAW))
    expect(name, `no @keyframes for "${anim!.value}"`).toBeDefined()
    const tokens = tokensOf(RULES, mode, strip)
    const panel = surface(tokens, '--panel')
    const washes = frames(name!).map((v) => toRgb(expandWith(tokens, v), panel))
    expect(washes.length, 'the wash has no frames').toBeGreaterThan(0)
    const p = paintOf(chain, mode, panel)
    expect(p.rim, `no visible rim in ${mode}`).not.toBeNull()
    for (const w of washes) {
      expect(w, 'a wash frame did not compute').not.toBeNull()
      const ratio = contrast(p.rim!, w!)
      expect(ratio, `rim ${hex(p.rim!)} on the wash ${hex(w!)} in ${mode} = ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(EDGE_MIN)
    }
  })
})

describe('ON AIR is steady, and keying repaints without moving anything', () => {
  it.each(SIGNS.map((s) => [s.name, s] as const))('%s does not animate while keyed', (_n, s) => {
    const chain = s.chain(true)
    for (const mode of MODES) {
      const w = winnerAt(RULES, mode, chain, 'animation', 'animation-name')
      const v = w ? w.value.trim() : 'none'
      expect(v, `${s.name} animates in ${mode}: ${w?.rule.selector} { ${w?.prop}: ${v} }`).toMatch(/^none\b/)
    }
  })

  it.each(SIGNS.map((s) => [s.name, s] as const))('%s keeps one box through the key-down', (_n, s) => {
    // Padding and border width are what a state change can use to move its neighbours. The
    // keyed and unkeyed states must win the SAME values, so keying repaints and never re-lays
    // out the row it sits in (Stop TX sits just before the TX strip's caption).
    const keyed = s.chain(true)
    const idle = s.chain(false)
    for (const mode of MODES) {
      for (const props of [['padding'], ['border', 'border-width']]) {
        const k = winnerAt(RULES, mode, keyed, ...props)
        const i = winnerAt(RULES, mode, idle, ...props)
        const val = (w: typeof k) => (w ? (w.prop === 'border' ? borderWidthOf(w.value) : w.value) : '0')
        expect(val(k), `${s.name} in ${mode}: ${props[0]} changes when keyed (${val(i)} → ${val(k)})`).toBe(val(i))
      }
    }
  })

  it.each(SIGNS.filter((s) => s.quietWhenIdle).map((s) => [s.name, s] as const))(
    '%s stays quiet when not keyed: no fill, no visible rim',
    (_n, s) => {
      const idle = s.chain(false)
      for (const mode of MODES) {
        const p = paintOf(idle, mode, surface(tokensOf(RULES, mode, idle), '--panel'))
        expect(p.fill, `${s.name} unkeyed paints a fill in ${mode}: ${p.fillValue}`).toBeNull()
        expect(p.rim, `${s.name} unkeyed draws a visible rim in ${mode}`).toBeNull()
      }
    },
  )
})

describe('the measurement covers every rule that could reach these signs', () => {
  // A host-scoped rule (`.some-cockpit .cockpit-txstate { … }`) would restyle the sign on one
  // screen and never show up above, because the rendered chains carry no host. So: any rule
  // whose SUBJECT names one of these elements by its own class but which reaches neither
  // rendered state must not touch what this file measures. If one ever does, render that host
  // here too. (A bare `span` subject under some unrelated container — `.mv-field > span` — is
  // not about the sign, and a subject must NAME it to count.)
  const WATCHED = /^(background|background-color|color|border|border-color|border-width|padding|animation|animation-name)$/
  it.each(SIGNS.map((s) => [s.name, s] as const))('%s: no unplaced rule paints it', (_n, s) => {
    const keyed = s.chain(true)
    const idle = s.chain(false)
    const subject = (sel: string) => sel.replace(/\s*>\s*/g, ' ').trim().split(/\s+/).pop()!
    const names = (compound: string, el: El) =>
      compoundMatches(compound, el) && (compound.match(/\.[\w-]+/g) ?? []).some((c) => el.classes.includes(c.slice(1)))
    const unplaced = RULES.filter(
      (r) =>
        r.decls.some((d) => WATCHED.test(d.prop)) &&
        (names(subject(r.selector), keyed[keyed.length - 1]) || names(subject(r.selector), idle[idle.length - 1])) &&
        // Placed by ANY rendered sign: Operate's caption and the TX strip's share `.cq-statecap`,
        // so `.cockpit-qso.tx .cq-statecap` names the strip's caption too — and is measured on
        // Operate's, the only host it can reach. A rule no rendered sign reaches is still flagged.
        !MODES.some((m) => SIGNS.some((o) => reachesChain(r.selector, o.chain(true), m) || reachesChain(r.selector, o.chain(false), m))),
    ).map((r) => r.selector)
    expect(unplaced, `rules this guard cannot place:\n${unplaced.join('\n')}`).toEqual([])
  })
})

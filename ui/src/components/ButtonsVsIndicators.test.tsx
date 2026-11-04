// @vitest-environment jsdom
//
// A BUTTON LOOKS PRESSABLE; A STATUS INDICATOR DOES NOT. Operators reported, in the dark theme, that
// they could not tell the transmit strip's buttons from the status lights beside them, and they were
// right: `.op-btn` drew TX Off, Tune and ATU as bare words with no face and no outline, the read-only
// TX On/Off of Phone and CW (a span) wore exactly the button's classes and look, and the header's
// read-only chips (the mode badge, the RTTY keying pill, the band placeholder) wore outlines a button
// wears. The visual language this holds, in every theme:
//   - a live button has an OUTLINE, 3:1 against what it sits on (WCAG 1.4.11, the boundary that
//     identifies a control), lit or not, and hovering it changes its outline or its face;
//   - a disabled button is dimmed and says not-allowed, as a disabled switch does;
//   - an indicator draws NO outline (its border paints nothing over its own face), does not react to
//     the pointer, and never shows a pointer cursor. It may keep a tint: a tint is not a boundary.
//
// Each element is RENDERED (CockpitTxStrip, CockpitHeader with the chips a cockpit hands it) and
// placed in App's chain (.app › .shell › the cockpit's main), and every value is the cascade WINNER
// on that chain (cssCascade), never the presence of a declaration. What it cannot see: jsdom never
// lays out, so the before/after look is in the change's real-Chrome screenshots.
import { describe, it, expect, vi, beforeAll } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { CockpitTxStrip } from './CockpitTxStrip'
import { CockpitHeader } from './CockpitHeader'
import {
  MODES,
  chainOf,
  contrast,
  expandWith,
  parseRules,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import type { AppSnapshot, RadioStatus } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)), haltTx: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const noop = () => {}
const R = (o: Partial<RadioStatus>) =>
  ({ dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txAllowed: true, txEnabled: false, txBusyReason: null, rigKeyed: false, atu: false, ...o }) as unknown as RadioStatus

/** App's elements above a cockpit's own children. */
const ABOVE: El[] = [
  { tag: 'div', classes: ['app'], attrs: {} },
  { tag: 'div', classes: ['shell'], attrs: {} },
  { tag: 'main', classes: ['layout', 'single', 'rtty-cockpit'], attrs: {} },
]

interface Item {
  name: string
  chain: El[]
}
/** Every element matching `sel` in what `mount` renders, named by its words, on App's chain. */
function collect(mount: () => void, sel: string, label: string): Item[] {
  mount()
  const out = [...document.body.querySelectorAll(sel)].map((e) => ({
    name: `${label} "${e.textContent!.trim()}"`,
    chain: [...ABOVE, ...chainOf(e).slice(1)],
  }))
  cleanup()
  return out
}

let LIVE: Item[] = []
let DISABLED: Item[] = []
let INDICATORS: Item[] = []
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  const armsHere = (r: RadioStatus) => () =>
    render(<CockpitTxStrip radio={r} onSetTxEnabled={noop} onTune={noop} onAtuTune={noop} onStopTx={noop} hold={{ on: r.txEnabled, onChange: noop }} />)
  const armsItself = (r: RadioStatus) => () => render(<CockpitTxStrip radio={r} onTune={noop} onStopTx={noop} />)
  LIVE = [
    ...collect(armsHere(R({})), '.cq-txctl button', 'strip button (unlit)'),
    ...collect(armsHere(R({ txEnabled: true, atu: true })), '.cq-txctl button.on', 'strip button (lit)'),
  ]
  DISABLED = collect(armsHere(R({ txAllowed: false })), '.cq-txctl button:disabled', 'strip button (disabled)')
  const header = () =>
    render(
      <CockpitHeader
        snap={{ radio: R({}) } as unknown as AppSnapshot}
        modeIndicator={
          <>
            <span className="cw-mode-badge">RTTY</span>
            <span className="rtty-backend-pill">AFSK</span>
          </>
        }
        bandControl={<span className="cockpit-ph-pill">20m</span>}
      />,
    )
  INDICATORS = [
    ...collect(armsItself(R({})), '.cq-txctl .readonly', 'read-only latch (off)'),
    ...collect(armsItself(R({ txEnabled: true })), '.cq-txctl .readonly', 'read-only latch (on)'),
    ...collect(armsItself(R({ txEnabled: true })), '.cq-statecap', 'TX state'),
    ...collect(armsItself(R({})), '.cq-statecap', 'TX state'),
    ...collect(header, '.cw-mode-badge, .rtty-backend-pill, .cockpit-ph-pill', 'header chip'),
  ]
})

// ── The cascade ─────────────────────────────────────────────────────────────────────────────

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const ALL = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** :hover and :disabled named as attributes, so the resolver can put an element in that state. */
const STATED = ALL.map((r) => ({ ...r, selector: r.selector.replace(/:hover/g, '[data-hover]').replace(/:disabled/g, '[disabled]').replace(/:enabled/g, ':not([disabled])') }))
/** The rules that can reach these chains: every class they name is one the chains carry. The
 *  answer is the same with the others gone, and the sweep takes a second instead of minutes. */
let RULES: Rule[] = []
const relevant = (items: Item[]) => {
  const have = new Set(items.flatMap((i) => i.chain.flatMap((e) => e.classes)))
  return STATED.filter((r) => (r.selector.match(/\.[\w-]+/g) ?? []).every((c) => have.has(c.slice(1))))
}

const hovered = (chain: El[]) => chain.map((e) => ({ ...e, attrs: { ...e.attrs, 'data-hover': '' } }))
const FACE = ['background', 'background-color']
const BORDER = ['border', 'border-color', 'border-top-color', 'border-top']
const paintOf = (v: string) => {
  const parts = v.trim().split(/\s+(?![^(]*\))/)
  if (parts.some((p) => /^(none|hidden)$/.test(p) || /^0(px)?$/.test(p))) return 'transparent'
  return parts.filter((p) => !/^-?[\d.]+(px|em|rem)?$/.test(p) && !/^(solid|dashed|dotted|double)$/.test(p)).join(' ') || 'transparent'
}
const memo = new Map<string, Map<string, string>>()
const tokens = (mode: Mode, chain: El[]) => {
  const k = `${mode}|${JSON.stringify(chain)}`
  if (!memo.has(k)) memo.set(k, tokensAt(RULES, mode, chain))
  return memo.get(k)!
}
const colour = (mode: Mode, chain: El[], value: string, backdrop: Rgb): Rgb => {
  const v = expandWith(tokens(mode, chain), value).trim()
  const c = toRgb(v === '' || v === 'none' ? 'transparent' : v, backdrop)
  if (!c) throw new Error(`not a colour: "${value}" → "${v}"`)
  return c
}

interface Look {
  under: Rgb
  face: Rgb
  border: Rgb
  ink: string
  cursor: string
  opacity: number
  borderRule: string
}
/** How the element at the chain's end paints in `mode`. */
function look(mode: Mode, chain: El[]): Look {
  let under = colour(mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0])
  for (let i = 0; i < chain.length - 1; i++) {
    const bg = winnerAt(RULES, mode, chain.slice(0, i + 1), ...FACE)
    if (bg) under = colour(mode, chain.slice(0, i + 1), bg.value, under)
  }
  const faceW = winnerAt(RULES, mode, chain, ...FACE)
  const face = faceW ? colour(mode, chain, faceW.value, under) : under
  const borderW = winnerAt(RULES, mode, chain, ...BORDER)
  const border = borderW ? colour(mode, chain, paintOf(borderW.value), face) : face
  const inkW = winnerAt(RULES, mode, chain, 'color')
  const op = winnerAt(RULES, mode, chain, 'opacity')
  return {
    under,
    face,
    border,
    ink: inkW ? expandWith(tokens(mode, chain), inkW.value) : 'inherit',
    cursor: winnerAt(RULES, mode, chain, 'cursor')?.value ?? 'auto',
    opacity: op ? Number(expandWith(tokens(mode, chain), op.value)) : 1,
    borderRule: borderW?.rule.selector ?? 'nothing',
  }
}

const OUTLINE_MIN = 3
/** An outline this close to the face it is drawn on paints nothing a reader can see as a boundary. */
const NO_OUTLINE_MAX = 1.1

/** One line per element that misses, with how many modes it misses in and the first of them. */
function worst(items: Item[], ratio: (mode: Mode, i: Item) => { r: number; what: string }, ok: (r: number) => boolean): string[] {
  const out: string[] = []
  for (const i of items) {
    let bad: { r: number; what: string; mode: Mode } | null = null
    let n = 0
    for (const mode of MODES) {
      const x = ratio(mode, i)
      if (ok(x.r)) continue
      n++
      if (!bad) bad = { ...x, mode }
    }
    if (bad) out.push(`${i.name}: ${bad.what} in ${n} of ${MODES.length} modes, e.g. ${bad.mode}`)
  }
  return out
}

describe('buttons look pressable and status indicators do not, in every theme', () => {
  beforeAll(() => {
    RULES = relevant([...LIVE, ...DISABLED, ...INDICATORS])
  })

  it('renders what it measures', () => {
    expect(LIVE.map((i) => i.name)).toEqual([
      'strip button (unlit) "TX Off"',
      'strip button (unlit) "Tune"',
      'strip button (unlit) "ATU"',
      'strip button (unlit) "Stop TX"',
      'strip button (unlit) "Hold Tx"',
      'strip button (lit) "TX On"',
      'strip button (lit) "ATU ON"',
      'strip button (lit) "Hold Tx"',
    ])
    expect(DISABLED.map((i) => i.name)).toEqual(['strip button (disabled) "Tune"', 'strip button (disabled) "ATU"'])
    expect(INDICATORS.map((i) => i.name)).toEqual([
      'read-only latch (off) "TX Off"',
      'read-only latch (on) "TX On"',
      'TX state "▼ Receiving"',
      'TX state "■ TX off"',
      'header chip "RTTY"',
      'header chip "AFSK"',
      'header chip "20m"',
    ])
  })

  it('every live button has an outline 3:1 against what it sits on, lit or not', () => {
    expect(
      worst(LIVE, (mode, i) => {
        const l = look(mode, i.chain)
        const r = contrast(l.border, l.under)
        return { r, what: `outline ${hex(l.border)} on ${hex(l.under)} = ${r.toFixed(2)}:1 (${l.borderRule})` }
      }, (r) => r >= OUTLINE_MIN),
    ).toEqual([])
  })

  it("every live button's words read 4.5:1 on its face", () => {
    expect(
      worst(LIVE, (mode, i) => {
        const l = look(mode, i.chain)
        const ink = colour(mode, i.chain, l.ink, l.face)
        const r = contrast(ink, l.face)
        return { r, what: `words ${hex(ink)} on ${hex(l.face)} = ${r.toFixed(2)}:1` }
      }, (r) => r >= 4.5),
    ).toEqual([])
  })

  it('hovering an unlit live button changes its outline or its face', () => {
    const unlit = LIVE.filter((i) => i.name.includes('unlit'))
    const still: string[] = []
    for (const i of unlit)
      for (const mode of MODES) {
        const a = look(mode, i.chain)
        const b = look(mode, hovered(i.chain))
        if (hex(a.border) === hex(b.border) && hex(a.face) === hex(b.face)) {
          still.push(`${i.name} in ${mode}`)
          break
        }
      }
    expect(still).toEqual([])
  })

  it('a live button shows the pointer; a disabled one is dimmed and not-allowed', () => {
    for (const i of LIVE) expect(look('dark', i.chain).cursor, i.name).toBe('pointer')
    for (const i of DISABLED)
      for (const mode of MODES) {
        const l = look(mode, i.chain)
        expect(l.opacity, `${i.name} ${mode}`).toBeLessThanOrEqual(0.6)
        expect(l.cursor, `${i.name} ${mode}`).toBe('not-allowed')
      }
  })

  it('no status indicator draws an outline', () => {
    expect(
      worst(INDICATORS, (mode, i) => {
        const l = look(mode, i.chain)
        const r = contrast(l.border, l.face)
        return { r, what: `outline ${hex(l.border)} on its face ${hex(l.face)} = ${r.toFixed(2)}:1 (${l.borderRule})` }
      }, (r) => r <= NO_OUTLINE_MAX),
    ).toEqual([])
  })

  it('no status indicator reacts to the pointer or shows a pointer cursor', () => {
    const live: string[] = []
    for (const i of INDICATORS)
      for (const mode of MODES) {
        const a = look(mode, i.chain)
        const b = look(mode, hovered(i.chain))
        if (a.cursor === 'pointer') live.push(`${i.name}: pointer cursor`)
        if (hex(a.border) !== hex(b.border) || hex(a.face) !== hex(b.face) || a.ink !== b.ink) live.push(`${i.name}: changes on hover in ${mode} (${JSON.stringify([hex(a.border), hex(a.face), a.ink])} → ${JSON.stringify([hex(b.border), hex(b.face), b.ink])})`)
        if (live.length) break
      }
    expect(live).toEqual([])
  })
})

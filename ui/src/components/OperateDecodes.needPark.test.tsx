// @vitest-environment jsdom
//
// A NEEDED PARK CALLING CQ WEARS THE NEEDED-PARK COLOUR IN THE DECODE LIST (operator, 2026-10-04: "Give it
// the needed-park colour").
//
// A park the operator still needs is a reason to work the station, so it outranks a CQ for the row's colour
// (`rowClass` gives the row `need-pota` in place of `cq`). The sheet had no `.decode-row.need-pota` rule, so
// that row lost the CQ's green and gained nothing: a needed park calling CQ read PLAINER than an ordinary CQ.
// The map and the band strip already paint it in the needed-park colour.
//
// Band Activity is rendered in Operate's own chain, and the cascade is resolved on the rows it rendered with
// the app's resolver (cssCascade.ts), in every base mode (day and night, standard and high contrast), bare and
// on every built-in theme of its base. The needed-park row must lead with the theme's own `--need-pota` on its
// edge, be tinted at least as far off the panel as an ordinary CQ, and be a visible step (ΔE_OK ≥ 0.02, one
// just-noticeable difference) away from the ordinary CQ's colour.
import { describe, it, expect, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { OperateDecodes } from './OperateDecodes'
import { NEED_TIER } from '../features/needs'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  baseTheme,
  chainOf,
  deltaE,
  expandWith,
  parseRules,
  rgbHex as hex,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import type { DecodeRow, NeedAlert, NeedTag } from '../types'

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The sheet before the needed-park row had a colour of its own. */
const SHIPPED = RULES.filter((r) => !/^\.decode-row\.need-pota\b/.test(r.selector))

const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every base mode, bare and on every built-in theme of its base. */
const MODES: Mode[] = BASE_MODES.flatMap((b) => skinsOf(baseTheme(b)).map((skin) => (skin ? withRoles(b, { skin }) : b)))

const alert = (call: string, tags: NeedTag[]): NeedAlert =>
  ({ call, entity: 'United States', band: '20m', zone: 5, tags, priority: NEED_TIER[tags[0]], headline: '', mode: 'FT8', freqMhz: 14.074 }) as NeedAlert
const decode = (from: string, message: string, freqHz: number, over: Partial<DecodeRow> = {}): DecodeRow =>
  ({ from, snr: -10, dtSec: 0.1, freqHz, message, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0, ...over }) as DecodeRow

/** K1ABC activates a park the operator still needs; W1CQ is an ordinary CQ; both call CQ. */
function renderRows() {
  const feed = render(
    <div className="app">
      <div className="operate-host">
        <main className="layout single operate-cockpit">
          <div className="cockpit-lower classic" data-cols="three">
            <div className="cockpit-decodes panel">
              <OperateDecodes
                decodes={[decode('K1ABC', 'CQ POTA K1ABC FN42', 1200), decode('W1CQ', 'CQ W1CQ FN31', 1400)]}
                slot={100}
                rxOffsetHz={1200}
                band="20m"
                tier="FT8"
                harqRescues={0}
                onCall={() => {}}
                needAlertsByCall={new Map([['K1ABC', [alert('K1ABC', ['NewPark', 'Pota'])]]])}
              />
            </div>
          </div>
        </main>
      </div>
    </div>,
  ).container
  const row = (call: string) => {
    const el = [...feed.querySelectorAll('.decode-row')].find((r) => r.textContent?.includes(call))
    if (!el) throw new Error(`no row for ${call}`)
    return { classes: [...el.classList], chain: [BODY, ...chainOf(el)] }
  }
  const out = { park: row('K1ABC'), cq: row('W1CQ') }
  cleanup()
  return out
}
const BODY: El = { tag: 'body', classes: [], attrs: {} }
const ROWS = renderRows()

const colourOf = (rules: Rule[], mode: Mode, chain: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokensAt(rules, mode, chain), value), under)
  if (!c) throw new Error(`${mode}: not a colour: "${value}"`)
  return c
}
const BLANK = /^(transparent|none|inherit|initial|unset)$/i
/** What the row sits on: the first opaque background out from its parent, composited back up, over the page. */
function under(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const layers: Array<[El[], string]> = []
  for (let i = chain.length - 2; i >= 0; i--) {
    const w = winnerAt(rules, mode, chain.slice(0, i + 1), 'background', 'background-color')
    if (!w || BLANK.test(w.value)) continue
    layers.push([chain.slice(0, i + 1), w.value])
    const at = chain.slice(0, i + 1)
    if (hex(colourOf(rules, mode, at, w.value, [0, 0, 0])) === hex(colourOf(rules, mode, at, w.value, [255, 255, 255]))) break
  }
  return layers.reduceRight((below, [at, v]) => colourOf(rules, mode, at, v, below), colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0]))
}
/** The row's own background over what it sits on (the surface itself where it paints none). */
function rowColour(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const base = under(rules, mode, chain)
  const w = winnerAt(rules, mode, chain, 'background', 'background-color')
  return !w || BLANK.test(w.value) ? base : colourOf(rules, mode, chain, w.value, base)
}
/** The colour of the row's left edge, or null where it has none. */
function edgeColour(rules: Rule[], mode: Mode, chain: El[]): Rgb | null {
  const w = winnerAt(rules, mode, chain, 'border-left', 'border-left-color')
  if (!w) return null
  const v = w.value.replace(/^\s*[\d.]+px\s+/, '').replace(/^(solid|dashed|dotted)\s+/, '').trim()
  return BLANK.test(v) ? null : colourOf(rules, mode, chain, v, under(rules, mode, chain))
}

/** Every mode where the needed-park row does not wear the needed-park colour, or reads plainer than a CQ. */
function plainParkRows(rules: Rule[]): string[] {
  const out: string[] = []
  for (const mode of MODES) {
    const surface = under(rules, mode, ROWS.park.chain)
    const park = rowColour(rules, mode, ROWS.park.chain)
    const cq = rowColour(rules, mode, ROWS.cq.chain)
    const edge = edgeColour(rules, mode, ROWS.park.chain)
    const want = colourOf(rules, mode, ROWS.park.chain, 'var(--need-pota)', surface)
    if (!edge || hex(edge) !== hex(want)) out.push(`${mode}: edge ${edge ? hex(edge) : 'none'}, not the theme's --need-pota ${hex(want)}`)
    const [tinted, cqTinted] = [deltaE(park, surface), deltaE(cq, surface)]
    if (tinted < cqTinted) out.push(`${mode}: tinted ${tinted.toFixed(3)} off the panel, plainer than a CQ's ${cqTinted.toFixed(3)}`)
    const step = deltaE(park, cq)
    if (step < 0.02) out.push(`${mode}: ${hex(park)} is ${step.toFixed(3)} from a CQ's ${hex(cq)}`)
  }
  return out
}

describe('a needed park calling CQ, in the decode list, in every theme', () => {
  it('renders the rows it measures: the park outranks the CQ for the row colour', () => {
    expect(ROWS.park.classes).toContain('need-pota')
    expect(ROWS.park.classes).not.toContain('cq')
    expect(ROWS.cq.classes).toContain('cq')
    expect(MODES.filter((m) => baseTheme(m) === 'light').length).toBeGreaterThan(0)
    expect(MODES.filter((m) => baseTheme(m) === 'dark').length).toBeGreaterThan(0)
  })

  it("wears the theme's needed-park colour, tinted at least as far as a CQ and a visible step from a CQ's", () => {
    expect(plainParkRows(RULES)).toEqual([])
  }, 60_000)

  it('FIRES: without its rule the row has no edge and is plainer than a CQ, in every theme', () => {
    const found = plainParkRows(SHIPPED)
    for (const mode of MODES) {
      expect(found.some((f) => f.startsWith(`${mode}: edge none`)), `${mode} edge`).toBe(true)
      expect(found.some((f) => f.startsWith(`${mode}: tinted`)), `${mode} tint`).toBe(true)
    }
  }, 60_000)
})

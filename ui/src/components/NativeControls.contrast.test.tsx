// @vitest-environment jsdom
//
// THE CONTROLS THE NATIVE-CONTROL CENSUS FOUND PAINTED BY THE BROWSER NOW PAINT THEMSELVES
// (2026-09-27, "Put it in test3").
//
// The census ran real Chrome over every view and Settings tab, in both themes, with the computer
// set to light and to dark. It listed every control whose face or lettering came from the browser
// and so followed the COMPUTER's scheme rather than Nexus's theme. The root colour-scheme rule
// (styles-color-scheme.test.ts) ends the dependence on the computer. What this file holds is that
// the controls it named do not depend on the browser at all, so they read in both themes however
// that rule is later changed.
//
// THE MIXED CONTROLS: the theme's ink on the browser's face, the amp strip's pattern (G7). The
// setup-health chips in Settings ▸ Radio (`--text` on a grey face: 3.09:1 and 1.02:1 across the
// two mismatches), Operate's roster Spot (`--text-dim`: 2.21:1 in the dark theme even on a dark
// computer) and the SSTV overlay remove (`--state-weak`: 1.57:1 in the dark theme on a dark
// computer). Each now paints its own face and border, the box the browser's was, and keeps the
// ink it had.
//
// THE MACRO KEYS. `.cw-macro` gave the key a face and a border and no ink. The keys that put their
// label in a `.cw-macro-label` child were fine, because that child pins `--text`. The ones with
// bare text inherited the browser's button ink: black or white by the computer. That was
// SSTV's manual-receive Start (1.03:1 in the light theme on a dark computer, 1.19:1 the other way),
// the SSTV picture viewer's Previous/Next/Save, JS8's station queries, inbox Read/Delete, Cancel and
// Drop, and two Remote Refresh/retry keys. The key now carries `--text` itself.
//
// The cascade guards read only the author sheet, never the browser's defaults, so the first check
// is that the author sheet declares the thing. The second is that the word reads 4.5:1 on its
// own face in every mode. The chains carry the real parent where the components name one; the
// `.cw-macro` rule is the same wherever the key sits, and the one ancestor-scoped rule in its
// family (`.rtty-macros .cw-macro:disabled`) sets only opacity.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { SetupHealth } from './SetupHealth'
import { OperateRoster } from './OperateRoster'
import { StationControlContext } from '../stationAccess'
import type { Station } from '../types'
import {
  BASE_MODES,
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

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getDeclination: vi.fn(async () => 0),
}))
afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
/** The resolver treats a state pseudo-class as a state the element is not in. A button's
 *  `:disabled` IS its `disabled` attribute, so it is read as one. */
const withDisabled = (rules: Rule[]) =>
  rules.map((r) => (r.selector.includes(':disabled') ? { ...r, selector: r.selector.replace(/:disabled/g, '[disabled]') } : r))
const RULES = withDisabled(parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css')))

const TEXT_MIN = 4.5
/** What these controls' hosts sit on. */
const BACKDROPS = ['--bg', '--panel', '--bg-elev'] as const

/** One control as the cascade sees it. */
interface Control {
  name: string
  chain: El[]
  disabled?: boolean
}
const el = (tag: string, classes: string[], attrs: Record<string, string> = {}): El => ({ tag, classes, attrs })

const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const keyOf = (chain: El[]) => JSON.stringify(chain.map((e) => [e.tag, e.classes, e.attrs]))
const win = (rules: Rule[], mode: Mode, chain: El[], ...props: string[]) =>
  once(rules, `w|${mode}|${keyOf(chain)}|${props.join()}`, () => winnerAt(rules, mode, chain, ...props))
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) => once(rules, `t|${mode}|${keyOf(chain)}`, () => tokensAt(rules, mode, chain))
const colour = (tokens: Map<string, string>, value: string, backdrop: Rgb): Rgb => {
  const c = toRgb(expandWith(tokens, value), backdrop)
  if (!c) throw new Error(`not a colour: "${value}" → "${expandWith(tokens, value)}"`)
  return c
}
const through = (c: Rgb, under: Rgb, opacity: number): Rgb => [0, 1, 2].map((i) => Math.round(c[i] * opacity + under[i] * (1 - opacity))) as unknown as Rgb

const FACE = ['background', 'background-color'] as const
const BORDER = ['border', 'border-color', 'border-top-color'] as const

/** Every control the author sheet leaves to the browser for its face, ink or border. */
function leftToTheBrowser(rules: Rule[], controls: Control[]): string[] {
  const out = new Set<string>()
  for (const c of controls) {
    for (const mode of BASE_MODES) {
      for (const [what, props] of [['face', FACE], ['ink', ['color']], ['border', BORDER]] as const) {
        if (!win(rules, mode, c.chain, ...props)) out.add(`${c.name}: the browser draws its ${what}`)
      }
    }
  }
  return [...out]
}

/** Every enabled control whose word reads under 4.5:1 on its own face, in any mode and backdrop. */
function unreadable(rules: Rule[], controls: Control[]): string[] {
  const out: string[] = []
  for (const c of controls.filter((x) => !x.disabled)) {
    for (const mode of BASE_MODES) {
      const tokens = tokensFor(rules, mode, c.chain)
      const face = win(rules, mode, c.chain, ...FACE)
      const ink = win(rules, mode, c.chain, 'color')
      if (!face || !ink) continue // leftToTheBrowser names it
      const opacity = Number(win(rules, mode, c.chain, 'opacity')?.value ?? 1)
      for (const s of BACKDROPS) {
        const under = colour(tokens, `var(${s})`, [0, 0, 0])
        const bg = through(colour(tokens, face.value, under), under, opacity)
        const fg = through(colour(tokens, ink.value, colour(tokens, face.value, under)), under, opacity)
        const r = contrast(fg, bg)
        if (r < TEXT_MIN) out.push(`${c.name} ${mode} on ${s}: ${hex(fg)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
      }
    }
  }
  return out
}

// ── The macro keys with bare text (SstvView.tsx, Js8Cockpit.tsx) ─────────────────────────────

const header = [el('div', ['cockpit-header']), el('div', ['ch-mode-extras'])]
const MACRO_KEYS: Control[] = [
  { name: 'SSTV manual-receive Start', chain: [...header, el('button', ['cw-macro', 'sstv-manual-rx'], { type: 'button' })] },
  { name: 'SSTV image retry', chain: [el('div', ['sstv-remote-image']), el('button', ['cw-macro'], { type: 'button' })] },
  { name: 'JS8 station query', chain: [el('div', []), el('button', ['cw-macro', 'js8-query'], { type: 'button' })] },
  { name: 'JS8 inbox Read/Delete', chain: [el('div', []), el('button', ['cw-macro', 'js8-inbox-act'], { type: 'button' })] },
  { name: 'JS8 Cancel pending', chain: [el('div', []), el('button', ['cw-macro', 'js8-cancel'], { type: 'button' })] },
  { name: 'JS8 Drop queue', chain: [el('div', []), el('button', ['cw-macro', 'js8-drop'], { type: 'button' })] },
  { name: 'JS8 Refresh', chain: [el('div', []), el('button', ['cw-macro'], { type: 'button' })] },
  { name: 'SSTV viewer Previous/Next/Save', chain: [el('div', ['sstv-viewer']), el('button', ['cw-macro'], { type: 'button' })] },
]

describe('the macro keys carry their own ink', () => {
  it('no bare-text macro key takes its lettering from the browser', () => {
    expect(leftToTheBrowser(RULES, MACRO_KEYS)).toEqual([])
  })

  it('every one reads 4.5:1 on its own face, in every mode', () => {
    expect(unreadable(RULES, MACRO_KEYS)).toEqual([])
  })

  it('FIRES: the key as it shipped, a face and a border but no ink, is caught', () => {
    const shipped = RULES.map((r) => (r.selector === '.cw-macro' ? { ...r, decls: r.decls.filter((d) => d.prop !== 'color') } : r))
    expect(leftToTheBrowser(shipped, MACRO_KEYS)).toContain('SSTV manual-receive Start: the browser draws its ink')
  })
})

// ── The mixed controls (SetupHealth.tsx, OperateRoster.tsx, SstvView.tsx) ──────────────────────

function healthChips(): Control[] {
  const r = render(<SetupHealth radio={{ catOk: true, rxLevel: 0.3, txEnabled: false }} catResult={null} onGoTo={() => {}} />)
  const out = [...r.container.querySelectorAll<HTMLButtonElement>('button.health-item')].map((b) => ({
    name: `Setup health "${b.textContent?.trim()}"`,
    chain: chainOf(b),
    disabled: b.disabled,
  }))
  cleanup()
  return out
}

const heard: Station = { call: 'W1AW', grid: 'FN31', snr: -10, lastHeardSlot: 100, heardCount: 1, presence: 'heard' as Station['presence'], worked: false }
function rosterSpot(): Control[] {
  const out: Control[] = []
  for (const selectedCall of ['W1AW', null]) {
    const r = render(
      <StationControlContext.Provider value={true}>
        <OperateRoster stations={[heard]} myGrid="EN52" currentSlot={100} needByCall={new Map()} selectedCall={selectedCall}
          onSelect={() => {}} onCall={() => {}} onToggleIgnore={() => {}} onSpot={() => {}} />
      </StationControlContext.Provider>,
    )
    const b = r.container.querySelector<HTMLButtonElement>('button.or-spot')
    expect(b, 'the roster has no Spot button').not.toBeNull()
    out.push({ name: `Operate roster Spot${b!.disabled ? ' (disabled)' : ''}`, chain: chainOf(b!), disabled: b!.disabled })
    cleanup()
  }
  return out
}

/** SstvView.tsx: the overlay rows sit in the compose `section.sstv-compose.panel`. */
const OVERLAY_REMOVE: Control = {
  name: 'SSTV overlay remove',
  chain: [el('section', ['sstv-compose', 'panel']), el('div', ['sstv-ov-row']), el('button', ['sstv-ov-remove'], { type: 'button' })],
}

describe('the three mixed controls paint their own face', () => {
  const controls = () => [...healthChips(), ...rosterSpot(), OVERLAY_REMOVE]

  it('renders the chips and both states of Spot (the census cannot silently empty out)', () => {
    expect(controls().map((c) => c.name)).toEqual(
      expect.arrayContaining(['Operate roster Spot', 'Operate roster Spot (disabled)', 'SSTV overlay remove']),
    )
    expect(healthChips().length).toBeGreaterThanOrEqual(2)
  })

  it('none of them is left to the browser for its face, ink or border', () => {
    expect(leftToTheBrowser(RULES, controls())).toEqual([])
  })

  it('each reads 4.5:1 on its own face, in both themes and every mode', () => {
    expect(unreadable(RULES, controls())).toEqual([])
  })

  it('a disabled Spot looks disabled', () => {
    for (const c of controls().filter((x) => x.disabled)) {
      expect(Number(win(RULES, 'light', c.chain, 'opacity')?.value ?? 1), c.name).toBeLessThan(1)
    }
  })

  it('FIRES: the three as they shipped, the theme\'s ink on the browser\'s face, are caught', () => {
    const shipped = RULES.filter((r) => !/health-item-link|or-spot/.test(r.selector)).map((r) =>
      r.selector === '.sstv-ov-remove' ? { ...r, decls: r.decls.filter((d) => !/^(background|border)/.test(d.prop)) } : r,
    )
    const left = leftToTheBrowser(shipped, controls())
    expect(left.some((m) => m.startsWith('Setup health') && m.endsWith('its face')), left.join('\n')).toBe(true)
    expect(left).toContain('Operate roster Spot: the browser draws its face')
    expect(left).toContain('SSTV overlay remove: the browser draws its face')
  })
})

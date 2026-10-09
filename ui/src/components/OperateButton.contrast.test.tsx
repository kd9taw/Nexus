// @vitest-environment jsdom
//
// EVERY BUTTON THAT SAYS "OPERATE" READS, IN BOTH THEMES (operator, 2026-09-27: "also, the operate
// button is hard to read in light mode").
//
// The amplifier strip's Operate/Standby button (AmpStrip, in the header of every cockpit with an
// amplifier behind it) was the browser's own button. The sheet set its size and, for Operate, a
// green ink and border, and nothing else. A native button's face follows the COMPUTER's light or
// dark setting, not Nexus's theme (index.html declares `color-scheme: dark light` and the sheet
// sets none), so the green word landed on a face the theme never chose. Measured in Chrome: in
// the light theme 4.47:1 on a light computer and 1.04:1 on a dark one; in the dark theme 1.53:1
// and 3.02:1. The fix is the band chip's, which the operator approved: the word in the theme's
// text colour, and the state's colour on the border and as a tint of the face. The strip's
// buttons now paint their own face, ink and border from the theme, as the Remote page's quick
// header already did.
//
// The cascade guards read the author sheet and never the browser's defaults, so the first check
// is that no amplifier button is left to them: each declares its own face, ink and border. Then
// the word clears 4.5:1 on that face in every mode and under every OK preset (the green is the OK
// role's), locked or not: the strip locks while the rig is keyed, which is exactly when the
// amplifier's state matters, so a locked button is measured through its own dimming. And Operate
// keeps the good-state colour on its border at 3:1 from any surface the header sits on: a status
// colour is held to 3:1, which a border needs and lettering does not meet. The other buttons that say Operate are held to the same floor: the Remote page's quick
// navigation and the amplifier strip in its quick header. Settings ▸ Features names Operate as a
// label and a heading, not a button; both read over 5:1 in Chrome and are not re-measured here.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { AmpStrip } from './AmpStrip'
import { QuickNavigation, RemotePresentationContext } from '../remote-web/presentation'
import { StationDataContext } from '../stationAccess'
import { PALETTE_ROLES } from '../features/paletteRoles'
import {
  BASE_MODES,
  SENTINEL_MODES,
  baseOf,
  chainOf,
  contrast,
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
import type { AmpStatus } from '../types'

// THE BUDGET (2026-10-09). The slowest case without a budget of its own, "no amplifier button is left to the browser:
// each paints…", takes 0.54 s and 0.50 s on one core (two runs); a loaded full suite on this box has run cases up to
// 20 times slower than one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails,
// after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  ampCommand: vi.fn(async () => true),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
/** The resolver treats a state pseudo-class as a state the element is not in. A button's
 *  `:disabled` IS its `disabled` attribute, which the rendered chain carries, so it is read as one. */
const withDisabled = (rules: Rule[]) =>
  rules.map((r) => (r.selector.includes(':disabled') ? { ...r, selector: r.selector.replace(/:disabled/g, '[disabled]') } : r))
const DESKTOP = withDisabled(parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css')))
const REMOTE = withDisabled(parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css') + '\n' + sheet('remote-web/application.css')))

const TEXT_MIN = 4.5
const STATE_MIN = 3
/** What a cockpit header can sit on. */
const BACKDROPS = ['--bg', '--panel', '--bg-elev'] as const
/** Every base mode under every OK preset: the good-state green is the OK role's. The base modes
 *  include the built-in themes MODES carries as the worst case of each base (SENTINEL_MODES). */
const OK = PALETTE_ROLES.find((r) => r.id === 'ok')!
const MODES: Mode[] = [...BASE_MODES, ...SENTINEL_MODES].flatMap((b) => OK.presets.map((p, i) => (i === 0 ? b : withRoles(b, { ok: p.id }))))

const linked = (over: Partial<AmpStatus> = {}): AmpStatus =>
  ({
    family: 'spe', model: '15K', linked: true, reason: '', operate: false, transmitting: false, outputWatts: 0,
    bandLabel: '80m', alarm: 'none', alarmRaised: false, warning: 'none', warningRaised: false, ...over,
  }) as AmpStatus

/** One rendered button: where it is, what it says, its chain and whether it is disabled. */
interface Button {
  host: string
  label: string
  chain: El[]
  disabled: boolean
  rules: Rule[]
  operate: boolean
}

function ampButtons(host: string, headerClass: string, rules: Rule[]): Button[] {
  const out: Button[] = []
  for (const [operate, keyed] of [[false, false], [true, false], [null, false], [true, true], [false, true]] as const) {
    const r = render(
      <div className={headerClass}>
        <div className="ch-actions">
          <AmpStrip amp={linked({ operate })} radioTransmitting={keyed} />
        </div>
      </div>,
    )
    for (const b of r.container.querySelectorAll<HTMLButtonElement>('.amp-strip button')) {
      const label = `${b.textContent?.trim() || '?'}${b.disabled ? ' (disabled)' : ''}`
      out.push({ host, label, chain: chainOf(b), disabled: b.disabled, rules, operate: b.classList.contains('on') })
    }
    cleanup()
  }
  return out
}

function quickNavOperate(): Button {
  const r = render(
    <StationDataContext.Provider value={false}>
      <RemotePresentationContext.Provider value={{ presentation: 'quick', change: () => {}, radioDetails: false, setRadioDetails: () => {} }}>
        <div className="app">
          <QuickNavigation view="needed" available={() => true} onSelect={() => {}} />
        </div>
      </RemotePresentationContext.Provider>
    </StationDataContext.Provider>,
  )
  const b = [...r.container.querySelectorAll<HTMLButtonElement>('.remote-quick-nav button')].find((x) => x.textContent?.trim() === 'Operate')
  expect(b, 'the quick navigation has no Operate button').toBeTruthy()
  const out = { host: 'Remote quick navigation', label: 'Operate', chain: chainOf(b!), disabled: b!.disabled, rules: REMOTE, operate: false }
  cleanup()
  return out
}

/** The cascade for one chain shape, once per rule set and mode (the ids and labels a render
 *  mints differ per render; no rule selects on them). */
const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|aria-label|aria-controls|title)$/.test(k))]))
const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) =>
  once(rules, `t|${mode}|${chainKey(chain)}`, () => tokensAt(rules, mode, chain))
/** The rule that wins a property. An OK preset changes only custom properties (a preset block may
 *  declare nothing else: styles-palette-roles.test.ts), so the base mode decides the winner. */
const win = (rules: Rule[], mode: Mode, chain: El[], ...props: string[]) =>
  once(rules, `w|${baseOf(mode)}|${chainKey(chain)}|${props.join()}`, () => winnerAt(rules, baseOf(mode), chain, ...props))
const colour = (tokens: Map<string, string>, value: string, backdrop: Rgb): Rgb => {
  const c = toRgb(expandWith(tokens, value), backdrop)
  if (!c) throw new Error(`not a colour: "${value}" → "${expandWith(tokens, value)}"`)
  return c
}

const FACE = ['background', 'background-color'] as const
const BORDER = ['border', 'border-color', 'border-top-color'] as const

/** Every button left to the browser for its face, ink or border, under any mode. */
function leftToTheBrowser(buttons: Button[]): string[] {
  const out = new Set<string>()
  for (const b of buttons) {
    for (const mode of MODES) {
      for (const [what, props] of [['face', FACE], ['ink', ['color']], ['border', BORDER]] as const) {
        if (!win(b.rules, mode, b.chain, ...props)) out.add(`${b.host} ${b.label}: the browser draws its ${what}`)
      }
    }
  }
  return [...out]
}

const through = (c: Rgb, under: Rgb, opacity: number): Rgb => [0, 1, 2].map((i) => Math.round(c[i] * opacity + under[i] * (1 - opacity))) as unknown as Rgb

/** Every button whose word reads under 4.5:1 on its own face, in any mode and backdrop. A locked
 *  one is measured too, through its own dimming: the word is the amplifier's state, and the strip
 *  locks exactly while the rig is keyed, which is when that state matters most. */
function unreadable(buttons: Button[]): string[] {
  const out: string[] = []
  for (const b of buttons) {
    for (const mode of MODES) {
      const tokens = tokensFor(b.rules, mode, b.chain)
      const face = win(b.rules, mode, b.chain, ...FACE)
      const ink = win(b.rules, mode, b.chain, 'color')
      if (!face || !ink) continue // leftToTheBrowser names it
      const opacity = Number(win(b.rules, mode, b.chain, 'opacity')?.value ?? 1)
      for (const s of BACKDROPS) {
        const under = colour(tokens, `var(${s})`, [0, 0, 0])
        const bg = through(colour(tokens, face.value, under), under, opacity)
        const fg = through(colour(tokens, ink.value, colour(tokens, face.value, under)), under, opacity)
        const r = contrast(fg, bg)
        if (r < TEXT_MIN) out.push(`${b.host} ${b.label} ${mode} on ${s}: ${hex(fg)} on ${hex(bg)} = ${r.toFixed(2)}:1`)
      }
    }
  }
  return out
}

describe('every button that says Operate reads, in both themes', () => {
  // Rendered once the file's ResizeObserver stub is in (the quick navigation measures itself).
  let desktop: Button[] = []
  let quick: Button[] = []
  let nav: Button
  beforeAll(() => {
    desktop = ampButtons('cockpit header', 'cockpit-header', DESKTOP)
    quick = ampButtons('Remote quick header', 'cockpit-header cockpit-header--quick', REMOTE)
    nav = quickNavOperate()
  })

  it('renders the amplifier strip in every state, in both headers (the census cannot silently empty out)', () => {
    for (const set of [desktop, quick]) {
      expect(set.map((b) => b.label)).toEqual(expect.arrayContaining(['Operate', 'Standby', '—', '◀', '▶', 'Operate (disabled)', 'Standby (disabled)']))
      expect(set.filter((b) => b.operate).length, 'Operate is rendered with its "on" state').toBeGreaterThan(0)
    }
  })

  it('no amplifier button is left to the browser: each paints its own face, ink and border', () => {
    expect(leftToTheBrowser([...desktop, ...quick, nav])).toEqual([])
  })

  // Twenty base modes (the eight standard ones and three worst-case themes' four each) under three
  // OK presets, on two sheets: about 3.5 s on a loaded box, so the budget is explicit.
  it('every button letters at 4.5:1 on its own face, locked or not, in every mode, under every OK preset', () => {
    expect(unreadable([...desktop, ...quick, nav])).toEqual([])
  }, 20_000)

  it('Operate keeps the good-state colour on its border, 3:1 from any surface the header sits on', () => {
    for (const b of desktop.filter((x) => x.operate)) {
      for (const mode of MODES) {
        const tokens = tokensFor(b.rules, mode, b.chain)
        const border = win(b.rules, mode, b.chain, ...BORDER)
        expect(border?.value, `${b.label} ${mode}`).toContain('var(--state-good)')
        const green = colour(tokens, 'var(--state-good)', [0, 0, 0])
        for (const s of BACKDROPS) {
          const r = contrast(green, colour(tokens, `var(${s})`, [0, 0, 0]))
          expect(r, `${b.label} ${mode}: the border on ${s}`).toBeGreaterThanOrEqual(STATE_MIN)
        }
      }
    }
    // …and Standby's border is not that colour, so the state is not the word's alone.
    for (const b of desktop.filter((x) => !x.operate && /^Standby/.test(x.label))) {
      expect(win(b.rules, 'light', b.chain, ...BORDER)?.value ?? '', b.label).not.toContain('--state-good')
    }
  })

  it('a locked amplifier button looks locked (and, above, still reads through its dimming)', () => {
    for (const b of [...desktop, ...quick].filter((x) => x.disabled)) {
      const o = win(b.rules, 'light', b.chain, 'opacity')
      expect(Number(o?.value ?? 1), `${b.host} ${b.label}`).toBeLessThan(1)
    }
  })

  // The same sweep as above (every mode, every OK preset), run twice: 1.6 s alone, 4.5–5.1 s under the
  // full suite on a loaded box, so the budget is explicit here too.
  it('FIRES: the strip as it shipped is caught, and so is a green word on a themed face', () => {
    // Before 2026-09-27: `.amp-strip button` set only font-size, line-height and padding, and
    // `.amp-op.on` a green ink and border; there was no disabled rule.
    const shipped = DESKTOP.filter((r) => r.selector !== '.amp-strip button[disabled]').map((r) =>
      r.selector === '.amp-strip button'
        ? { ...r, decls: r.decls.filter((d) => ['font-size', 'line-height', 'padding'].includes(d.prop)) }
        : r.selector === '.amp-op.on'
          ? { ...r, decls: [{ prop: 'color', value: 'var(--state-good)' }, { prop: 'border-color', value: 'var(--state-good)' }] }
          : r,
    )
    const left = leftToTheBrowser(desktop.map((b) => ({ ...b, rules: shipped })))
    expect(left).toContain('cockpit header Operate: the browser draws its face')
    expect(left).toContain('cockpit header Standby: the browser draws its face')
    // The other way out, a themed face with the green word kept, is caught in the light theme.
    const greenWord = [
      ...shipped,
      ...parseRules(
        `.amp-strip button { color: var(--text); background: var(--panel); border: 1px solid var(--border); }
         .amp-op.on { color: var(--state-good); background: color-mix(in srgb, var(--state-good) 16%, var(--panel)); }`,
        { n: 1e6 },
      ),
    ]
    const found = unreadable(desktop.map((b) => ({ ...b, rules: greenWord })))
    expect(found.some((m) => m.startsWith('cockpit header Operate light on')), found.join('\n')).toBe(true)
  }, 20_000)
})

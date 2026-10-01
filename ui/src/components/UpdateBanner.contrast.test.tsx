// @vitest-environment jsdom
//
// THE UPDATE BANNER'S BUTTON READS IN EVERY LIGHT THEME (found measuring, 2026-09-29).
//
// The banner's one filled button — "Install and restart" when an update is ready, "Download" when an
// install failed — lettered a fixed near-black (#0d1117) on the theme's accent. The dark themes'
// accents are light, and there it reads 7.2:1 and up (4.97:1 under the violet preset at night). The
// light themes darken their accent so that white reads on it, and the near-black read, in Chrome,
// 3.68:1 on the standard light accent, 2.64:1 on Silver's, 2.99:1 on Paper's and 2.13:1 under the
// violet preset at night. In the light themes the button now letters in the theme's own ink for the
// accent (--accent-ink, what every other accent-filled control letters in); the dark themes keep the
// near-black they had.
//
// This renders both of the banner's filled buttons and resolves, with the app's own resolver
// (cssCascade.ts), the word and the face it sits on: in every light theme (the four light modes, bare
// and on each built-in light theme, under every accent preset) the word must clear 4.5:1; in every
// dark theme it is painted by the same declaration as before.
import { describe, it, expect, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { UpdateBanner } from './UpdateBanner'
import type { SelfUpdate } from '../useSelfUpdate'
import { PALETTE_ROLES } from '../features/paletteRoles'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  baseTheme,
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

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The button as it shipped before this fix: the light-theme rule this guard exists for, removed. */
const SHIPPED = RULES.filter((r) => !(r.selector.startsWith("[data-theme='light'] ") && r.selector.includes('.update-install')))

const TEXT_MIN = 4.5
const ACCENT = PALETTE_ROLES.find((r) => r.id === 'accent')!
const PRESETS: Record<string, string>[] = [{}, ...ACCENT.presets.slice(1).map((p) => ({ accent: p.id }))]
const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every built-in light theme: the four light modes, bare and on each light theme, under every accent preset. */
const LIGHT: Mode[] = BASE_MODES.filter((b) => baseTheme(b) === 'light').flatMap((b) =>
  skinsOf('light').flatMap((skin) => PRESETS.map((roles) => withRoles(b, skin ? { skin, ...roles } : roles))),
)
/** Every dark theme, bare: a preset declares only custom properties (styles-palette-roles.test.ts),
 *  so it cannot change WHICH declaration paints the word, which is what "unchanged" is held to. */
const DARK: Mode[] = BASE_MODES.filter((b) => baseTheme(b) === 'dark').flatMap((b) => skinsOf('dark').map((skin) => (skin ? withRoles(b, { skin }) : b)))

const update = (over: Partial<SelfUpdate>): SelfUpdate => ({
  phase: 'ready',
  version: '9.9.9',
  blockReason: null,
  progress: null,
  error: null,
  install: () => {},
  dismiss: () => {},
  downloadInstead: () => {},
  ...over,
})

interface Button {
  label: string
  chain: El[]
}

/** Both filled buttons, as App mounts the banner: a direct child of `.app`, under <body>. */
function renderButtons(): Button[] {
  const out: Button[] = []
  for (const phase of ['ready', 'error'] as const) {
    const r = render(
      <div className="app">
        <UpdateBanner update={update({ phase, error: phase === 'error' ? 'the download was cut short' : null })} />
      </div>,
    )
    for (const b of r.container.querySelectorAll<HTMLButtonElement>('.update-install')) {
      out.push({ label: b.textContent?.trim() ?? '?', chain: [{ tag: 'body', classes: [], attrs: {} }, ...chainOf(b)] })
    }
    cleanup()
  }
  return out
}

const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-\w+)$/.test(k))]))
const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) => once(rules, `t|${mode}|${chainKey(chain)}`, () => tokensAt(rules, mode, chain))
/** A preset changes only custom properties, so the mode without its presets decides a winner. */
const winnerMode = (mode: Mode): Mode => mode.split(' ').filter((p, i) => i === 0 || p.startsWith('skin=')).join(' ') as Mode
const win = (rules: Rule[], mode: Mode, chain: El[], ...props: string[]) =>
  once(rules, `w|${winnerMode(mode)}|${chainKey(chain)}|${props.join()}`, () => winnerAt(rules, winnerMode(mode), chain, ...props))
const colourOf = (rules: Rule[], mode: Mode, chain: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokensFor(rules, mode, chain), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}

/** The word and its face. The button paints its own face and its own ink (asserted below), so
 *  both are the button's; the banner's panel only shows at the face's edge. */
function wordOf(rules: Rule[], mode: Mode, b: Button) {
  const face = win(rules, mode, b.chain, 'background', 'background-color')
  const ink = win(rules, mode, b.chain, 'color')
  if (!face || !ink) throw new Error(`${b.label} ${mode}: the button does not paint its own ${face ? 'ink' : 'face'}`)
  const panel = colourOf(rules, mode, b.chain, 'var(--panel)', [0, 0, 0])
  const bg = colourOf(rules, mode, b.chain, face.value, panel)
  const fg = colourOf(rules, mode, b.chain, ink.value, bg)
  return { fg, bg, ratio: contrast(fg, bg), ink }
}

function unreadable(rules: Rule[], buttons: Button[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const b of buttons)
    for (const mode of modes) {
      const { fg, bg, ratio } = wordOf(rules, mode, b)
      if (ratio < TEXT_MIN) out.push(`${b.label} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
    }
  return out
}

describe("the update banner's button reads in every light theme", () => {
  const buttons = renderButtons()

  it('renders both filled buttons, Install and Download (the census cannot silently empty out)', () => {
    expect(buttons.map((b) => b.label)).toEqual(['Install and restart', 'Download'])
  })

  it('every word clears 4.5:1 on the accent in every light theme, under every accent preset', () => {
    expect(unreadable(RULES, buttons, LIGHT)).toEqual([])
  }, 30_000)

  it('in the light themes it letters in the theme’s ink for the accent', () => {
    for (const b of buttons) for (const mode of LIGHT) expect(win(RULES, mode, b.chain, 'color')?.value, `${b.label} ${mode}`).toBe('var(--accent-ink)')
  })

  it('in every dark theme the word is painted by the same declaration as before, the near-black', () => {
    const moved: string[] = []
    for (const b of buttons)
      for (const mode of DARK) {
        const now = win(RULES, mode, b.chain, 'color')
        const was = win(SHIPPED, mode, b.chain, 'color')
        if (now?.rule !== was?.rule || now?.value !== '#0d1117') moved.push(`${b.label} ${mode}: ${was?.value} → ${now?.value}`)
      }
    expect(moved).toEqual([])
  }, 30_000)

  it('FIRES: the button as it shipped is caught in the light themes, at the ratios Chrome measured, and not in the dark ones', () => {
    const found = unreadable(SHIPPED, buttons, LIGHT)
    // The resolver must agree with what Chrome painted, or it is not measuring the same thing.
    expect(found).toContain('Install and restart light: #0d1117 on #0174ab = 3.68:1')
    expect(found).toContain('Download light skin=silver: #0d1117 on #3b5a7a = 2.64:1')
    expect(found).toContain('Install and restart light-night accent=violet: #0d1117 on #5b2da5 = 2.13:1')
    expect(unreadable(SHIPPED, buttons, DARK)).toEqual([])
  }, 30_000)
})

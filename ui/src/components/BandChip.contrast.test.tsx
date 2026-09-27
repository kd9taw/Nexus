// @vitest-environment jsdom
//
// THE BAND CHIP'S BAND NAME READS IN EVERY THEME (operator, 2026-09-27).
//
// The band chip — the top bar's channel trigger and the cockpit headers' (FrequencyControl), the
// same control as a select in Settings ▸ Station (its `full` variant), and Phone's and CW's band
// picker (BandPicker) — is painted in its band's colour from bandColors.ts,
// so "what band am I on" reads across the room. That palette was tuned for the dark map's spot
// dots. As LETTERING it read 1.2–4.2:1 on the light theme's surfaces (20m's green 1.5–1.8:1), and
// six violets read 3.4–4.5:1 on the dark theme's. The operator's two picks:
//   · LIGHT: "Text colour + band border" — the band name takes the theme's text colour and the
//     band's colour stays on the chip's border, glow and dot;
//   · DARK: "Tune the six violets" — 2200m, 630m, 160m, 70cm, 33cm and 23cm letter in a slightly
//     lighter violet that reads 4.5:1; every other band, and the dark look, stay exactly as they
//     were. The lift is the chip's only (`bandChipInk`): the palette itself, which the globes and
//     the Field Day board also paint, is untouched.
//
// The colour arrives as an INLINE style, which no cascade guard reads, so this renders the real
// chips for every band and resolves what actually paints the lettering in every mode: the inline
// ink, unless an author rule beats it (only `!important` can). It is measured against both
// surfaces the chip is drawn on — its own `--bg` fill in the cockpit headers and the picker, and
// the top bar's `--bg-elev` behind its transparent trigger.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { FrequencyControl } from './FrequencyControl'
import { BandPicker } from './BandPicker'
import { BAND_COLOR } from '../bandColors'
import {
  BASE_MODES,
  baseTheme,
  chainOf,
  contrast,
  expandWith,
  oklch,
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
import type { AppSnapshot } from '../types'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getLicensedBandPlan: vi.fn(async () => []),
}))

beforeAll(() => {
  // Radix Popper observes its trigger with a ResizeObserver jsdom lacks.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

const TEXT_MIN = 4.5
/** The two surfaces the chip is drawn on (see the header). */
const SURFACES = ['--bg', '--bg-elev'] as const
const BANDS = Object.keys(BAND_COLOR)
const VIOLETS = ['2200m', '630m', '160m', '70cm', '33cm', '23cm']

/** What one rendered chip paints, read off the DOM: its chain, and its inline ink and border. */
interface Chip {
  host: string
  band: string
  chain: El[]
  ink: Rgb
  border: Rgb
}

const rgbOfStyle = (v: string): Rgb => {
  const c = toRgb(v, [0, 0, 0])
  if (!c) throw new Error(`not a colour: "${v}"`)
  return c
}

function chipOf(host: string, band: string, container: HTMLElement, selector = '.band-menu-trigger'): Chip {
  const trigger = container.querySelector(selector) as HTMLElement | null
  expect(trigger, `${host} ${band}: no band chip rendered`).not.toBeNull()
  return { host, band, chain: chainOf(trigger!), ink: rgbOfStyle(trigger!.style.color), border: rgbOfStyle(trigger!.style.borderTopColor || trigger!.style.borderColor) }
}

function renderChips(): Chip[] {
  const out: Chip[] = []
  for (const band of BANDS) {
    let r = render(
      <FrequencyControl channels={[]} dialMhz={14.074} band={band} mode="USB" showReadout={false} showModeToggle={false} onSet={() => {}} />,
    )
    out.push(chipOf('FrequencyControl', band, r.container))
    cleanup()
    // The same chip as a select in Settings ▸ Station (the `full` variant).
    r = render(
      <FrequencyControl channels={[]} dialMhz={14.074} band={band} mode="USB" variant="full" showReadout={false} showModeToggle={false} onSet={() => {}} />,
    )
    out.push(chipOf('FrequencyControl(full)', band, r.container, 'select.freq-channel'))
    cleanup()
    const snap = { radio: { band, dialMhz: 14.2, sideband: 'USB', catOk: true, txAllowed: true, transmitting: false, txEnabled: false, tuning: false } } as unknown as AppSnapshot
    r = render(<BandPicker snap={snap} mode="phone" />)
    out.push(chipOf('BandPicker', band, r.container))
    cleanup()
  }
  return out
}

/** The cascade for one chain, resolved once per rule set, mode and chain SHAPE. Every band's chip
 *  from one host has the same tags, classes and selectable attributes; only per-render ids and
 *  labels differ, and no rule in either sheet selects on `id`, `aria-label`, `aria-controls` or
 *  `title`. Resolving the whole sheet for all 57 chips × 8 modes took seconds, and a guard that
 *  only passes on an idle box is the flake class this batch removed. */
const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|aria-label|aria-controls|title)$/.test(k))]))
const resolved = new WeakMap<Rule[], Map<string, { tokens: Map<string, string>; color: ReturnType<typeof winnerAt> }>>()
function cascadeOf(rules: Rule[], mode: Mode, chain: El[]) {
  let byKey = resolved.get(rules)
  if (!byKey) resolved.set(rules, (byKey = new Map()))
  const key = `${mode}|${chainKey(chain)}`
  let hit = byKey.get(key)
  if (!hit) byKey.set(key, (hit = { tokens: tokensAt(rules, mode, chain), color: winnerAt(rules, mode, chain, 'color') }))
  return hit
}

/** The lettering a chip paints under `mode`: its inline ink, unless an author rule beats it. */
function letteringOf(rules: Rule[], chip: Chip, mode: Mode): Rgb {
  const { tokens, color: w } = cascadeOf(rules, mode, chip.chain)
  const important = !!w && w.rule.decls.some((d) => d.prop === 'color' && /!\s*important\s*$/.test(d.value))
  if (!important) return chip.ink
  return rgbOfStyle(expandWith(tokens, w!.value))
}

/** Every chip × mode × surface where the band name reads under 4.5:1. */
function unreadable(rules: Rule[], chips: Chip[], modes: readonly Mode[] = BASE_MODES): string[] {
  const out: string[] = []
  for (const chip of chips) {
    for (const mode of modes) {
      const { tokens } = cascadeOf(rules, mode, chip.chain)
      const ink = letteringOf(rules, chip, mode)
      for (const s of SURFACES) {
        const bg = rgbOfStyle(expandWith(tokens, `var(${s})`))
        const r = contrast(ink, bg)
        if (r < TEXT_MIN) out.push(`${chip.host} ${chip.band} ${mode}: ${hex(ink)} on ${s} ${hex(bg)} = ${r.toFixed(2)}:1`)
      }
    }
  }
  return out
}

describe("the band chip's band name reads, in every theme", () => {
  const chips = renderChips()

  it('renders a chip for every band, from all three hosts (the census cannot silently empty out)', () => {
    expect(chips).toHaveLength(BANDS.length * 3)
  })

  it('every band name clears 4.5:1 on both surfaces, in every mode', () => {
    expect(unreadable(RULES, chips)).toEqual([])
  })

  it('the band colour stays on the border, for every band, in every theme', () => {
    for (const chip of chips) expect(hex(chip.border), `${chip.host} ${chip.band}`).toBe(BAND_COLOR[chip.band])
  })

  it('in the dark theme every band but the six violets letters in its palette colour, byte for byte', () => {
    const dark = BASE_MODES.filter((m) => baseTheme(m) === 'dark')
    for (const chip of chips.filter((c) => !VIOLETS.includes(c.band))) {
      for (const mode of dark) expect(hex(letteringOf(RULES, chip, mode)), `${chip.host} ${chip.band} ${mode}`).toBe(BAND_COLOR[chip.band])
    }
  })

  it('the six violets letter a LIGHTER violet of the same hue in the dark theme', () => {
    for (const chip of chips.filter((c) => VIOLETS.includes(c.band))) {
      const was = oklch(rgbOfStyle(BAND_COLOR[chip.band]))
      const now = oklch(letteringOf(RULES, chip, 'dark'))
      expect(now.L, `${chip.host} ${chip.band} is not lighter`).toBeGreaterThan(was.L)
      expect(Math.abs(now.H - was.H), `${chip.host} ${chip.band} changed hue`).toBeLessThan(5)
    }
  })

  it('in the light theme the band name takes the theme text colour', () => {
    const light = BASE_MODES.filter((m) => baseTheme(m) === 'light')
    for (const chip of chips) {
      for (const mode of light) {
        const text = rgbOfStyle(expandWith(cascadeOf(RULES, mode, chip.chain).tokens, 'var(--text)'))
        expect(hex(letteringOf(RULES, chip, mode)), `${chip.host} ${chip.band} ${mode}`).toBe(hex(text))
      }
    }
  })

  it('FIRES: the palette as lettering, with no light rule, is caught in both themes', () => {
    // The shipped state before this change: the inline band colour everywhere.
    const palette = chips.map((c) => ({ ...c, ink: rgbOfStyle(BAND_COLOR[c.band]) }))
    const noLightRule = RULES.filter((r) => !(r.selector.includes("[data-theme='light']") && /band-menu-trigger|freq-control\.full/.test(r.selector)))
    const found = unreadable(noLightRule, palette)
    expect(found.some((m) => m.startsWith('FrequencyControl 20m light:')), 'light 20m').toBe(true)
    expect(found.some((m) => m.startsWith('BandPicker 15m light-high:')), 'light-high 15m').toBe(true)
    expect(found.some((m) => m.startsWith('FrequencyControl 2200m dark:')), 'dark 2200m').toBe(true)
    expect(found.some((m) => m.startsWith('BandPicker 20m dark:')), 'dark 20m passes, so it is not reported').toBe(false)
  })
})

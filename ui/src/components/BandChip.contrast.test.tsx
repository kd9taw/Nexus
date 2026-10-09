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
//
// THE SAME FIX ON THE LOGBOOK GLOBE AND THE FIELD DAY BOARD (operator, 2026-09-27: "Same small fix
// as the chip, same guard; no colour changes in dark or on the globe itself"). The globe's band
// select and the Field Day band board letter a band name in the same palette. In the light theme
// both now take the theme's ink and keep the band's colour on a marker: the select on its border,
// and the board's name, which has no border, on an underline. In the dark theme both keep the
// palette exactly, violets included (the lift above is the chip's alone), and the globe's own
// drawing is not touched.
//
// THE BUILT-IN THEMES (features/skins.ts) take the light treatment in all four hosts: the palette
// reads as lettering only on the standard dark theme's surfaces, and on the lighter raised surfaces
// of four dark themes it fell to 3.7–4.4:1. The sweeps below walk the standard modes and the
// themes MODES carries as the worst case of each base (SENTINEL_MODES), so a theme whose surfaces
// a band name cannot read on is caught here whichever it is.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { FrequencyControl } from './FrequencyControl'
import { BandPicker } from './BandPicker'
import QsoGlobe from './QsoGlobe'
import { FdBandOccupancy } from './ContestView'
import { BAND_COLOR } from '../bandColors'
import {
  BASE_MODES,
  SENTINEL_MODES,
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
import type { AppSnapshot, FdClubStatus } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "every band name clears 4.5:1 on both surfaces, in every…", takes
// 0.42 s and 0.38 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getLicensedBandPlan: vi.fn(async () => []),
}))
// jsdom has no WebGL. The globe never mounts here anyway (its box measures 0×0); the band select
// in the HUD above it is what is measured, and the log answers with every band so it offers all.
vi.mock('react-globe.gl', () => ({ default: () => null }))
vi.mock('../features/logSource', async (importOriginal) => {
  const { BAND_COLOR } = await import('../bandColors')
  return {
    ...(await importOriginal<typeof import('../features/logSource')>()),
    useLogAnswer: (q: { kind: string } | null) => (q?.kind === 'bandsInLog' ? Object.keys(BAND_COLOR) : undefined),
  }
})

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
/** The standard modes and the worst-case themes' (see the header). */
const SWEPT: readonly Mode[] = [...BASE_MODES, ...SENTINEL_MODES]
/** Where a band name takes the theme's ink: the light theme, and every built-in theme. */
const THEME_INKED: readonly Mode[] = [...BASE_MODES.filter((m) => baseTheme(m) === 'light'), ...SENTINEL_MODES]
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
/** A lettered band name, whatever carries its band's colour besides. */
type Lettered = Pick<Chip, 'host' | 'band' | 'chain' | 'ink'>

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
function letteringOf(rules: Rule[], chip: Lettered, mode: Mode): Rgb {
  const { tokens, color: w } = cascadeOf(rules, mode, chip.chain)
  const important = !!w && w.rule.decls.some((d) => d.prop === 'color' && /!\s*important\s*$/.test(d.value))
  if (!important) return chip.ink
  return rgbOfStyle(expandWith(tokens, w!.value))
}

/** Every chip × mode × surface where the band name reads under 4.5:1. */
function unreadable(rules: Rule[], chips: Lettered[], modes: readonly Mode[] = SWEPT): string[] {
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

  it('in the light theme and on every built-in theme the band name takes the theme text colour', () => {
    for (const chip of chips) {
      for (const mode of THEME_INKED) {
        const text = rgbOfStyle(expandWith(cascadeOf(RULES, mode, chip.chain).tokens, 'var(--text)'))
        expect(hex(letteringOf(RULES, chip, mode)), `${chip.host} ${chip.band} ${mode}`).toBe(hex(text))
      }
    }
  })

  it('FIRES: the palette as lettering, with no light or theme rule, is caught in both themes and on a theme', () => {
    // The shipped state before this change: the inline band colour everywhere.
    const palette = chips.map((c) => ({ ...c, ink: rgbOfStyle(BAND_COLOR[c.band]) }))
    const noLightRule = RULES.filter((r) => !(/\[data-theme='light'\]|\[data-skin\]/.test(r.selector) && /band-menu-trigger|freq-control\.full/.test(r.selector)))
    const found = unreadable(noLightRule, palette)
    expect(found.some((m) => m.startsWith('FrequencyControl 20m light:')), 'light 20m').toBe(true)
    expect(found.some((m) => m.startsWith('BandPicker 15m light-high:')), 'light-high 15m').toBe(true)
    expect(found.some((m) => m.startsWith('FrequencyControl 2200m dark:')), 'dark 2200m').toBe(true)
    expect(found.some((m) => m.startsWith('BandPicker 20m dark:')), 'dark 20m passes, so it is not reported').toBe(false)
    expect(found.some((m) => m.startsWith('FrequencyControl 80m dark skin=lagoon:')), 'Lagoon 80m').toBe(true)
  })
})

// ── THE LOGBOOK GLOBE'S BAND SELECT AND THE FIELD DAY BAND BOARD (see the header) ──────────────────

/** A band name on the globe's select or the board, and the inline colour of what carries its band
 *  besides the lettering: the select's border, the board name's underline ('' when there is none). */
interface Name extends Lettered {
  marker: string
}

/** One live position per band that has worked somebody, so every band is busy and, one to a band
 *  and one mode, none clashes. */
const clubOn = (bands: string[], mode = 'CW'): FdClubStatus =>
  ({
    board: bands.map((band, i) => ({ posid: `p${i}`, posName: `tent ${i}`, band, mode, operator: 'W9AAA', qsos: 1, rate: 1, lastSeenSecs: 1 })),
  }) as unknown as FdClubStatus

/** The board's band-name cell for `band`: the grid cell whose text is the band. */
function boardName(container: HTMLElement, band: string): HTMLElement {
  const board = container.querySelector('[data-band-occupancy]')
  expect(board, 'no Field Day band board rendered').not.toBeNull()
  const cell = [...board!.children].find((e) => e.textContent?.trim() === band) as HTMLElement | undefined
  expect(cell, `the board has no ${band} row`).toBeTruthy()
  return cell!
}

function renderNames(): Name[] {
  const out: Name[] = []
  render(<QsoGlobe />)
  const select = document.querySelector('select.qso-globe-band-pick') as HTMLSelectElement | null
  expect(select, 'QsoGlobe: no band select rendered').not.toBeNull()
  for (const band of BANDS) {
    fireEvent.change(select!, { target: { value: band } })
    expect(select!.value, `QsoGlobe does not offer ${band}`).toBe(band)
    out.push({ host: 'QsoGlobe', band, chain: chainOf(select!), ink: rgbOfStyle(select!.style.color), marker: select!.style.borderColor })
  }
  cleanup()
  // The board at both sizes: the dashboard's, and the torn-off Field Day window's `big` one.
  for (const big of [false, true]) {
    const r = render(<FdBandOccupancy club={clubOn(BANDS)} big={big} />)
    for (const band of BANDS) {
      const cell = boardName(r.container, band)
      out.push({ host: big ? 'FdBandOccupancy(big)' : 'FdBandOccupancy', band, chain: chainOf(cell), ink: rgbOfStyle(cell.style.color), marker: cell.style.textDecorationColor })
    }
    cleanup()
  }
  return out
}

const underlineOf = (rules: Rule[], mode: Mode, chain: El[]) =>
  winnerAt(rules, mode, chain, 'text-decoration', 'text-decoration-line')?.value ?? 'none'

describe('the same band name on the Logbook globe and the Field Day board', () => {
  // Rendered once the file's ResizeObserver stub is in: the globe measures its box on mount.
  let names: Name[] = []
  beforeAll(() => {
    names = renderNames()
  })
  const light = BASE_MODES.filter((m) => baseTheme(m) === 'light')
  const dark = BASE_MODES.filter((m) => baseTheme(m) === 'dark')
  const onBoard = (n: Name) => n.host.startsWith('FdBandOccupancy')
  const inked = (mode: Mode) => THEME_INKED.includes(mode)

  it('renders a name for every band, on the globe select and both boards (the census cannot silently empty out)', () => {
    expect(names).toHaveLength(BANDS.length * 3)
  })

  it('in the light theme and on every built-in theme every band name takes the theme text colour, and clears 4.5:1 on both surfaces', () => {
    for (const n of names) {
      for (const mode of THEME_INKED) {
        const text = rgbOfStyle(expandWith(cascadeOf(RULES, mode, n.chain).tokens, 'var(--text)'))
        expect(hex(letteringOf(RULES, n, mode)), `${n.host} ${n.band} ${mode}`).toBe(hex(text))
      }
    }
    expect(unreadable(RULES, names, THEME_INKED)).toEqual([])
  })

  it("the band's colour stays on the select's border and the board name's underline, drawn wherever the name takes the theme's ink", () => {
    for (const n of names) expect(n.marker && hex(rgbOfStyle(n.marker)), `${n.host} ${n.band}`).toBe(BAND_COLOR[n.band])
    for (const n of names.filter(onBoard)) {
      for (const mode of SWEPT) {
        expect(underlineOf(RULES, mode, n.chain), `${n.host} ${n.band} ${mode}`).toBe(inked(mode) ? 'underline' : 'none')
      }
    }
  })

  it('in the dark theme every band letters in its palette colour, byte for byte, the six violets included', () => {
    for (const n of names) {
      for (const mode of dark) expect(hex(letteringOf(RULES, n, mode)), `${n.host} ${n.band} ${mode}`).toBe(BAND_COLOR[n.band])
    }
  })

  it("in the dark theme every band but the six violets clears 4.5:1 (the violets are the operator's call: no colour change in dark)", () => {
    expect(unreadable(RULES, names.filter((n) => !VIOLETS.includes(n.band)), dark)).toEqual([])
  })

  it('a clash keeps its alarm ink and no underline in the light theme: the fix never paints over it', () => {
    const r = render(<FdBandOccupancy club={clubOn(['20m', '20m'], 'DIG')} big />)
    const cell = boardName(r.container, '20m')
    expect(cell.nextElementSibling?.getAttribute('data-band-clash'), 'the fixture must clash').toBe('20m')
    const chain = chainOf(cell)
    // Control: a light rule written over every name on the board WOULD paint over the alarm, so
    // the pass below is the rule's selector at work, not an assertion that cannot fail.
    const overEveryName = RULES.map((rule) => (rule.selector.includes('[data-band-ink]') ? { ...rule, selector: rule.selector.replace('[data-band-ink]', 'span') } : rule))
    for (const mode of light) {
      const { tokens } = cascadeOf(RULES, mode, chain)
      const alarm = { host: 'FdBandOccupancy(big)', band: '20m', chain, ink: rgbOfStyle(expandWith(tokens, cell.style.color)) }
      expect(hex(alarm.ink), `${mode}: the clash is not in the alert ink`).toBe(hex(rgbOfStyle(expandWith(tokens, 'var(--alert-critical)'))))
      expect(hex(letteringOf(RULES, alarm, mode)), mode).toBe(hex(alarm.ink))
      expect(underlineOf(RULES, mode, chain), mode).toBe('none')
      expect(hex(letteringOf(overEveryName, alarm, mode)), `${mode}: control`).not.toBe(hex(alarm.ink))
    }
  })

  it('FIRES: without the light and theme rule these names letter in the palette, and the sweep catches them', () => {
    const noLightRule = RULES.filter((r) => !(/\[data-theme='light'\]|\[data-skin\]/.test(r.selector) && /qso-globe-band-pick|data-band-ink/.test(r.selector)))
    const found = unreadable(noLightRule, names, THEME_INKED)
    expect(found.some((m) => m.startsWith('QsoGlobe 20m light:')), 'globe 20m light').toBe(true)
    expect(found.some((m) => m.startsWith('FdBandOccupancy(big) 15m light-high:')), 'board 15m light-high').toBe(true)
    expect(found.some((m) => m.startsWith('FdBandOccupancy 6m light-night:')), 'board 6m light-night').toBe(true)
    expect(found.some((m) => m.startsWith('QsoGlobe 80m dark skin=lagoon:')), 'globe 80m Lagoon').toBe(true)
  })
})

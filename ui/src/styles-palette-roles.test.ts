// COLOUR ROLES — every preset an operator can pick (Settings ▸ Appearance ▸ Colours) is what the
// sheet actually paints, reads on every surface it lands on, and stays clear of the transmit red
// and the critical orange. Operator picks of 2026-09-26: "Presets first, hex later", "Agent picks,
// you review", "Locked + solid ON AIR".
//
// The table in features/paletteRoles.ts is the spec; styles.css is the implementation. Nothing
// here pins a preset's hex — every assertion is computed on the cascade winner, in dark, light,
// dark-high and light-high and their four Night twins, at <html> and INSIDE A DISPLAY WELL (a well
// declares the dark palette on itself, so a preset that only arrives by inheritance stops at its
// edge). At night the accent and readout paint their table's `night` values, and OK, Amber and
// Cyan paint exactly their day ones. The one place hexes ARE pinned is the defaults by day,
// because "nothing changes for anyone who does not touch it" is a statement about exact values.
//
// THE FLOORS, and why each is the number it is:
//   · TEXT 4.5:1 (WCAG 1.4.3) — the accent where it is lettering, the ink ON an accent fill, your
//     own chat bubble's text, and the frequency digits.
//   · STATUS 3:1 (WCAG 1.4.11) — OK, amber and info, the accent as a fill edge on the page, and the
//     focus ring (the floor ui/design/verify.mjs holds every status role to).
//   · ΔE_OK ≥ 0.15 from --tx, --alert-critical and --snr-weak, for every colour a role paints.
//     ΔE_OK is distance in OKLab; ≈0.02 is a just-noticeable difference, so 0.15 is about seven.
//     Today's closest default is the light theme's warning amber at 0.18 from --tx, so the floor
//     costs no default anything; a red accent sits ≈0.02 from --tx and an orange one ≈0.05 from
//     --alert-critical, so it refuses exactly the confusion "TX locked" exists to prevent.
//   · ΔE_OK ≥ 0.06 from the same three under deuteranopia, protanopia and tritanopia (Machado
//     2009) — verify.mjs's good↔bad floor. Today's light OK green clears it by 0.001.
//   · The readout ≥ 0.10 from --state-pending: the digits dim to that ink while a retune is
//     unconfirmed, and must visibly change when they do (today's closest default: 0.11).
//   · OK and amber ≥ 0.12 apart for every pair of presets: strong and marginal signal share the
//     SNR column (today: 0.16 dark, 0.17 light).
//
// TODAY'S SHORTFALLS are recorded, not excused: the default accent and readout of the LIGHT theme
// read below 4.5:1 as lettering (DEFAULT_SHORTFALLS). They are today's values, which the defaults
// must keep byte for byte; every non-default preset clears every floor, with no exemption.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import {
  BASE_MODES,
  CVDS,
  baseTheme,
  contrast,
  deltaE,
  expandWith,
  isNight,
  oklab,
  oklch,
  parseRules,
  rgbHex as hex,
  rootTokensFrom,
  simulateCvd,
  toRgb,
  tokensAt,
  withRoles,
  type BaseMode,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'
import { PALETTE_ROLES, isLockedToken, type PalettePreset, type PaletteRole } from './features/paletteRoles'

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const read = (name: string) => readFileSync(fileURLToPath(new URL(`./${name}`, import.meta.url)), 'utf8')
const SHEET = blank(read('styles.css')) + '\n' + blank(read('cockpit-panes.css'))
const ORDER = { n: 0 }
const RULES = parseRules(SHEET, ORDER)

const TEXT_MIN = 4.5
const STATUS_MIN = 3
const LOCKED_DE = 0.15
const LOCKED_DE_CVD = 0.06
const PENDING_DE = 0.1
const OK_AMBER_DE = 0.12
/** One 8-bit step in the green channel moves a mid-tone's contrast by about this much. */
const ROUNDING = 0.05

type Scope = 'root' | 'well'
const SCOPES: readonly Scope[] = ['root', 'well']
const WELL: El[] = [{ tag: 'div', classes: ['well'], attrs: {} }]
const LOCKED_REDS = ['--tx', '--alert-critical', '--snr-weak'] as const

const role = (id: string) => PALETTE_ROLES.find((r) => r.id === id)!
const isDefault = (r: PaletteRole, p: PalettePreset) => p === r.presets[0]
const modeFor = (r: PaletteRole, p: PalettePreset, base: BaseMode): Mode =>
  isDefault(r, p) ? base : withRoles(base, { [r.id]: p.id })
/** Which of the table's two columns paints here: a well is the dark theme in either. */
const columnOf = (mode: Mode, scope: Scope) => (scope === 'well' ? 'dark' : baseTheme(mode))
/** What the table says `p` paints in `mode`: its night values at night, if its role dims. */
const valuesOf = (p: PalettePreset, mode: Mode, scope: Scope) => (isNight(mode) && p.night ? p.night : p)[columnOf(mode, scope)]
/** The four day modes: where "today's values" is a statement about exact hexes. */
const DAY_MODES = BASE_MODES.filter((b) => !isNight(b))

const memo = new WeakMap<Rule[], Map<string, Map<string, string>>>()
/** Every custom property visible in `scope` under `mode`, var()s expanded. */
function tokensIn(rules: Rule[], mode: Mode, scope: Scope): Map<string, string> {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  const key = `${mode}|${scope}`
  let out = byKey.get(key)
  if (!out) {
    if (scope === 'well') out = tokensAt(rules, mode, WELL)
    else {
      const raw = rootTokensFrom(rules, mode)
      out = new Map([...raw].map(([k, v]) => [k, expandWith(raw, v).trim()]))
    }
    byKey.set(key, out)
  }
  return out
}
const valueOf = (tokens: Map<string, string>, token: string) => expandWith(tokens, `var(${token})`).trim().toLowerCase()
function rgbOf(tokens: Map<string, string>, token: string): Rgb {
  // An undeclared token expands to "", which `toRgb` would hand back as the backdrop — black, and
  // black on the light theme's white passes every floor. So an empty value is a failure here.
  const v = valueOf(tokens, token)
  const c = v === '' ? null : toRgb(v, [0, 0, 0])
  if (!c) throw new Error(`${token} does not resolve to a colour: "${v}"`)
  return c
}

/** The surfaces a role's colour lands on: the frame's four at <html>, the face inside a well. */
const surfacesOf = (scope: Scope) =>
  scope === 'well' ? (['--well-bg', '--bg-elev', '--bg-elev-2'] as const) : (['--panel', '--bg', '--bg-elev', '--bg-elev-2'] as const)
const textSurfacesOf = (scope: Scope) =>
  scope === 'well' ? (['--well-bg', '--bg-elev'] as const) : (['--panel', '--bg-elev', '--bg-elev-2'] as const)

function inHue(h: number, [from, to]: readonly [number, number]): boolean {
  return from <= to ? h >= from && h <= to : h >= from || h <= to
}

interface Problem {
  /** Stable identity, no numbers: what DEFAULT_SHORTFALLS records. */
  key: string
  message: string
}

/** Everything wrong with `preset` of `role`, over every base mode and both scopes. */
function roleProblems(rules: Rule[], r: PaletteRole, p: PalettePreset): Problem[] {
  const out: Problem[] = []
  const add = (key: string, message: string) => out.push({ key, message })
  for (const base of BASE_MODES) {
    const mode = modeFor(r, p, base)
    for (const scope of SCOPES) {
      const tk = tokensIn(rules, mode, scope)
      const where = `${r.id}=${p.id} ${base} ${scope}`
      const ratio = (fg: string, bg: string) => contrast(rgbOf(tk, fg), rgbOf(tk, bg))
      const floor = (fg: string, bg: string, min: number, kind: string) => {
        const v = ratio(fg, bg)
        if (v < min) add(`${where}: ${fg} on ${bg}`, `${where}: ${kind} ${fg} ${hex(rgbOf(tk, fg))} on ${bg} ${hex(rgbOf(tk, bg))} = ${v.toFixed(2)}:1 < ${min}:1`)
      }
      // Readable where it is lettering, visible where it is a mark.
      if (r.id === 'accent') {
        for (const s of textSurfacesOf(scope)) floor('--accent', s, TEXT_MIN, 'accent lettering')
        if (scope === 'root') floor('--accent', '--bg', STATUS_MIN, 'accent fill edge')
        floor('--accent-ink', '--accent', TEXT_MIN, 'ink on the accent fill')
        floor('--bubble-mine-text', '--bubble-mine', TEXT_MIN, 'your chat bubble')
        for (const s of surfacesOf(scope)) floor('--focus-ring', s, STATUS_MIN, 'focus ring')
      } else if (r.id === 'readout') {
        for (const s of textSurfacesOf(scope)) floor('--readout', s, TEXT_MIN, 'frequency digits')
        const d = deltaE(rgbOf(tk, '--readout'), rgbOf(tk, '--state-pending'))
        if (d < PENDING_DE) add(`${where}: --readout vs --state-pending`, `${where}: the digits ${hex(rgbOf(tk, '--readout'))} are ΔE ${d.toFixed(3)} from the unconfirmed ink ${hex(rgbOf(tk, '--state-pending'))} < ${PENDING_DE}`)
      } else {
        for (const t of [...r.tokens, ...r.aliases]) for (const s of surfacesOf(scope)) floor(t, s, STATUS_MIN, 'status colour')
      }
      // Clear of the reds, in normal vision and under each colour-vision deficiency.
      for (const t of [...r.tokens, ...r.aliases]) {
        const c = rgbOf(tk, t)
        for (const red of LOCKED_REDS) {
          const d = deltaE(c, rgbOf(tk, red))
          if (d < LOCKED_DE) add(`${where}: ${t} vs ${red}`, `${where}: ${t} ${hex(c)} is ΔE ${d.toFixed(3)} from ${red} ${hex(rgbOf(tk, red))} < ${LOCKED_DE}`)
          for (const cvd of CVDS) {
            const dc = deltaE(simulateCvd(c, cvd), simulateCvd(rgbOf(tk, red), cvd))
            if (dc < LOCKED_DE_CVD) add(`${where}: ${t} vs ${red} (${cvd})`, `${where}: ${t} ${hex(c)} is ΔE ${dc.toFixed(3)} from ${red} to a ${cvd} eye < ${LOCKED_DE_CVD}`)
          }
        }
        const { C, H } = oklch(c)
        if (C >= 0.04 && !inHue(H, r.hue)) add(`${where}: ${t} hue`, `${where}: ${t} ${hex(c)} hue ${H.toFixed(0)}° is outside ${r.id}'s window ${r.hue[0]}°–${r.hue[1]}°`)
      }
      // DESIGN.md rule 3: good stays lighter than bad, so a red-green colour-blind eye still
      // tells them apart by lightness alone.
      if (r.id === 'ok') {
        for (const t of r.tokens) for (const red of ['--tx', '--snr-weak']) {
          const [lg, lr] = [oklab(rgbOf(tk, t))[0], oklab(rgbOf(tk, red))[0]]
          if (lg <= lr) add(`${where}: ${t} lightness`, `${where}: ${t} ${hex(rgbOf(tk, t))} (L ${lg.toFixed(3)}) is not lighter than ${red} (L ${lr.toFixed(3)})`)
        }
      }
    }
  }
  return out
}

/** Does the sheet paint exactly what the table says, at <html> and inside a well? */
function parityProblems(rules: Rule[], r: PaletteRole, p: PalettePreset): string[] {
  const out: string[] = []
  for (const base of BASE_MODES) {
    const mode = modeFor(r, p, base)
    for (const scope of SCOPES) {
      const tk = tokensIn(rules, mode, scope)
      const want = valuesOf(p, mode, scope)
      for (const t of r.tokens) {
        const got = valueOf(tk, t)
        if (got !== want[t]) out.push(`${r.id}=${p.id} ${base} ${scope}: ${t} paints "${got}", the table says ${want[t]}`)
      }
    }
  }
  return out
}

// ── The table is well formed ────────────────────────────────────────────────────────────────

describe('the role table', () => {
  it('holds the five roles, each with a default and at least two presets', () => {
    expect(PALETTE_ROLES.map((r) => r.id)).toEqual(['accent', 'readout', 'ok', 'amber', 'cyan'])
    for (const r of PALETTE_ROLES) expect(r.presets.length, r.id).toBeGreaterThanOrEqual(3)
  })

  it('gives every preset a value for every token the role drives, in both themes, and nothing else', () => {
    for (const r of PALETTE_ROLES) {
      for (const p of r.presets) {
        for (const col of ['dark', 'light'] as const) {
          expect(Object.keys(p[col]).sort(), `${r.id}=${p.id} ${col}`).toEqual([...r.tokens].sort())
          for (const v of Object.values(p[col])) expect(v, `${r.id}=${p.id} ${col}`).toMatch(/^#[0-9a-f]{6}$/)
        }
      }
      expect(new Set(r.presets.map((p) => p.id)).size, `${r.id} repeats a preset id`).toBe(r.presets.length)
    }
  })

  it('gives every preset of a role Night dims its night values, for the same tokens — and none to the rest', () => {
    // Night (styles.css NIGHT) dims the accent and the readout; OK, Amber and Cyan are signal
    // colours it never touches. A preset of a dimming role with no night column would show its
    // DAY colour at night, and one on a signal role would be a night retune of a signal colour.
    expect(PALETTE_ROLES.filter((r) => r.dimsAtNight).map((r) => r.id)).toEqual(['accent', 'readout'])
    for (const r of PALETTE_ROLES) {
      for (const p of r.presets) {
        if (!r.dimsAtNight) {
          expect(p.night, `${r.id}=${p.id} has night values, but ${r.id} is a signal colour`).toBeUndefined()
          continue
        }
        expect(p.night, `${r.id}=${p.id} has no night values`).toBeDefined()
        for (const col of ['dark', 'light'] as const) {
          expect(Object.keys(p.night![col]).sort(), `${r.id}=${p.id} night ${col}`).toEqual([...r.tokens].sort())
          for (const v of Object.values(p.night![col])) expect(v, `${r.id}=${p.id} night ${col}`).toMatch(/^#[0-9a-f]{6}$/)
        }
      }
    }
  })
})

// ── The sheet paints the table ──────────────────────────────────────────────────────────────

describe('every preset paints exactly its table values, at <html> and inside a well', () => {
  const cases = PALETTE_ROLES.flatMap((r) => r.presets.map((p) => [`${r.id}=${p.id}`, r, p] as const))
  it.each(cases)('%s', (_n, r, p) => {
    expect(parityProblems(RULES, r, p)).toEqual([])
  })

  it('no rule selects a preset the table does not have, or the default (the default is no attribute)', () => {
    expect(selectorProblems(RULES)).toEqual([])
  })
})

/** A rule written against a preset id the table does not know is dead CSS that looks finished;
 *  one written against the default would paint what the absence of the attribute already does. */
function selectorProblems(rules: Rule[]): string[] {
  const bad: string[] = []
  for (const rule of rules) {
    for (const r of PALETTE_ROLES) {
      for (const m of rule.selector.matchAll(new RegExp(`\\[${r.attr}='([^']*)'\\]`, 'g'))) {
        const p = r.presets.find((x) => x.id === m[1])
        if (!p) bad.push(`${rule.selector}: ${r.id} has no preset "${m[1]}"`)
        else if (isDefault(r, p)) bad.push(`${rule.selector}: selects ${r.id}'s default, which is the absence of ${r.attr}`)
      }
    }
  }
  return bad
}

/** A preset block may declare its own role's tokens and nothing else — so never a locked one. */
function blockProblems(rules: Rule[]): string[] {
  const bad: string[] = []
  for (const rule of rules) {
    for (const r of PALETTE_ROLES) {
      if (!rule.selector.includes(`[${r.attr}=`)) continue
      for (const d of rule.decls) {
        if (isLockedToken(d.prop)) bad.push(`${rule.selector} { ${d.prop} } — a LOCKED colour`)
        else if (!r.tokens.includes(d.prop)) bad.push(`${rule.selector} { ${d.prop} } — not ${r.id}'s`)
      }
    }
  }
  return bad
}

// ── Defaults are today's values ─────────────────────────────────────────────────────────────

/** main at ab84af6b, before this change: what every role painted, dark (which is also every
 *  well) and light. The readout's is the accent's, because the digits were painted in it. */
const TODAY: Record<string, { dark: Record<string, string>; light: Record<string, string> }> = {
  accent: {
    dark: { '--accent': '#4cc9f0', '--accent-ink': '#06222c', '--focus-ring': '#4cc9f0', '--bubble-mine': '#1f5d73', '--bubble-mine-text': '#eaf7fc' },
    light: { '--accent': '#0d8ecf', '--accent-ink': '#ffffff', '--focus-ring': '#0b83c0', '--bubble-mine': '#0d8ecf', '--bubble-mine-text': '#ffffff' },
  },
  readout: { dark: { '--readout': '#4cc9f0' }, light: { '--readout': '#0d8ecf' } },
  ok: {
    dark: { '--snr-strong': '#69d98d', '--rx': '#69d98d', '--band-open': '#69d98d' },
    light: { '--snr-strong': '#007f35', '--rx': '#007f35', '--band-open': '#007f35' },
  },
  amber: {
    dark: { '--alert-warning': '#f7c243', '--snr-marginal': '#f1ca47', '--band-marginal': '#f1ca47' },
    light: { '--alert-warning': '#a76d00', '--snr-marginal': '#a27000', '--band-marginal': '#a27000' },
  },
  cyan: { dark: { '--alert-info': '#79c0f1' }, light: { '--alert-info': '#0070a6' } },
}

describe('the defaults are byte-identical to today', () => {
  it.each(PALETTE_ROLES.map((r) => [r.id, r] as const))('%s: the default preset is today’s values', (_n, r) => {
    expect({ dark: r.presets[0].dark, light: r.presets[0].light }).toEqual(TODAY[r.id])
  })

  it.each(PALETTE_ROLES.map((r) => [r.id, r] as const))('%s: with no attribute set, the sheet paints today’s values', (_n, r) => {
    for (const base of DAY_MODES) {
      for (const scope of SCOPES) {
        const tk = tokensIn(RULES, base, scope)
        const want = TODAY[r.id][columnOf(base, scope)]
        for (const t of r.tokens) expect(valueOf(tk, t), `${r.id} ${base} ${scope}: ${t}`).toBe(want[t])
      }
    }
  })

  it('the frequency digits are painted in --readout (so the default is the accent they wore)', () => {
    for (const sel of ['.readout-val', '.readout-input']) {
      const colour = [...RULES].reverse().find((x) => x.selector === sel && x.decls.some((d) => d.prop === 'color'))
      expect(colour?.decls.filter((d) => d.prop === 'color').pop()?.value, sel).toBe('var(--readout)')
    }
  })
})

// ── Readable, visible, clear of the reds ────────────────────────────────────────────────────

/** Where today's colours fall short of the floors — the light theme's accent and readout as
 *  lettering. Recorded rather than exempted: the defaults must stay today's, so this is what an
 *  operator who never opens Colours sees, and every non-default preset clears all of it. */
const DEFAULT_SHORTFALLS = [
  ...(['light', 'light-high'] as const).flatMap((b) => [
    `accent=cyan ${b} root: --accent on --panel`,
    `accent=cyan ${b} root: --accent on --bg-elev`,
    `accent=cyan ${b} root: --accent on --bg-elev-2`,
    `accent=cyan ${b} root: --accent on --bg`,
    `accent=cyan ${b} root: --accent-ink on --accent`,
    `accent=cyan ${b} root: --bubble-mine-text on --bubble-mine`,
    `readout=cyan ${b} root: --readout on --panel`,
    `readout=cyan ${b} root: --readout on --bg-elev`,
    `readout=cyan ${b} root: --readout on --bg-elev-2`,
  ]),
].sort()

describe('every preset is readable and clear of the reds, in every mode and inside a well', () => {
  const nonDefault = PALETTE_ROLES.flatMap((r) => r.presets.slice(1).map((p) => [`${r.id}=${p.id}`, r, p] as const))
  it.each(nonDefault)('%s', (_n, r, p) => {
    expect(roleProblems(RULES, r, p).map((x) => x.message)).toEqual([])
  })

  it('the defaults fall short exactly where today does, and nowhere else', () => {
    const found = PALETTE_ROLES.flatMap((r) => roleProblems(RULES, r, r.presets[0]).map((x) => x.key))
    expect([...new Set(found)].sort()).toEqual(DEFAULT_SHORTFALLS)
  })
})

describe('OK and amber stay apart for every pair of presets', () => {
  const ok = role('ok')
  const amber = role('amber')
  const pairs = ok.presets.flatMap((g) => amber.presets.map((a) => [`ok=${g.id} amber=${a.id}`, g, a] as const))
  it.each(pairs)('%s', (_n, g, a) => {
    const bad: string[] = []
    for (const base of BASE_MODES) {
      const mode = withRoles(base, { ...(isDefault(ok, g) ? {} : { ok: g.id }), ...(isDefault(amber, a) ? {} : { amber: a.id }) })
      for (const scope of SCOPES) {
        const tk = tokensIn(RULES, mode, scope)
        const d = deltaE(rgbOf(tk, '--snr-strong'), rgbOf(tk, '--snr-marginal'))
        if (d < OK_AMBER_DE) bad.push(`${base} ${scope}: strong ${hex(rgbOf(tk, '--snr-strong'))} vs marginal ${hex(rgbOf(tk, '--snr-marginal'))} ΔE ${d.toFixed(3)}`)
      }
    }
    expect(bad).toEqual([])
  })
})

// ── Every fill the sheet paints in a role colour keeps its lettering ────────────────────────

interface Fill {
  selector: string
  fill: string
  ink: string
}
/** Solid role-colour fills with lettering on them, discovered from the sheet, so a new chip is
 *  covered the day it lands (the need-fill sweep in styles-theme-cascade.test.ts, one family on). */
function fillsIn(rules: Rule[]): Fill[] {
  const owned = new Set(PALETTE_ROLES.flatMap((r) => [...r.tokens, ...r.aliases]))
  const declOf = (selector: string, prop: string) =>
    [...rules].reverse().find((x) => x.selector === selector && x.decls.some((d) => d.prop === prop))?.decls.filter((d) => d.prop === prop).pop()?.value
  const out: Fill[] = []
  for (const rule of rules) {
    const bg = rule.decls.filter((d) => d.prop === 'background' || d.prop === 'background-color').pop()
    const m = bg && /^var\((--[\w-]+)\)$/.exec(bg.value.trim())
    if (!m || !owned.has(m[1])) continue
    const base = rule.selector.replace(/\.[\w-]+$/, '')
    const ink = declOf(rule.selector, 'color') ?? (base && base !== rule.selector ? declOf(base, 'color') : undefined)
    // No ink = a dot or a bar, not lettering; an ink that IS the fill is a dot drawn in its colour.
    if (!ink || ink.replace(/\s/g, '') === bg!.value.replace(/\s/g, '')) continue
    out.push({ selector: rule.selector, fill: bg!.value.trim(), ink: ink.trim() })
  }
  return out
}
const roleOfToken = (v: string) => PALETTE_ROLES.find((r) => [...r.tokens, ...r.aliases].some((t) => v.includes(`var(${t})`)))

/** A preset may not make lettering on a role fill worse than today — or below 4.5:1 where
 *  today clears it. Every combination of the fill's role and the ink's role is walked. */
function fillProblems(rules: Rule[], fills: Fill[]): string[] {
  const out: string[] = []
  for (const f of fills) {
    const roles = [roleOfToken(f.fill)!, roleOfToken(f.ink)].filter((r, i, a): r is PaletteRole => !!r && a.indexOf(r) === i)
    const combos = roles.reduce<Record<string, string>[]>(
      (acc, r) => acc.flatMap((c) => r.presets.map((p) => (isDefault(r, p) ? c : { ...c, [r.id]: p.id }))),
      [{}],
    )
    for (const base of BASE_MODES) {
      const measure = (mode: Mode) => {
        const tk = tokensIn(rules, mode, 'root')
        const bg = toRgb(expandWith(tk, f.fill), [0, 0, 0])!
        const ink = toRgb(expandWith(tk, f.ink), bg)!
        return { ratio: contrast(ink, bg), bg, ink }
      }
      const today = measure(base).ratio
      const min = today >= TEXT_MIN ? TEXT_MIN : today - ROUNDING
      for (const c of combos) {
        const now = measure(withRoles(base, c))
        if (now.ratio < min) {
          out.push(`${f.selector} ${base} ${JSON.stringify(c)}: ink ${hex(now.ink)} on ${hex(now.bg)} = ${now.ratio.toFixed(2)}:1, today ${today.toFixed(2)}:1`)
        }
      }
    }
  }
  return out
}

describe('lettering on a role-colour fill', () => {
  const FILLS = fillsIn(RULES)
  it('finds the known fills (the discovery cannot silently empty out)', () => {
    expect(FILLS.map((f) => f.selector)).toEqual(
      expect.arrayContaining(['.band-chip.active', '.theme-chip.active', '.bubble.mine', '.decode-tag.me', '.cockpit-txarm.armed', '.opening-new']),
    )
  })
  it('stays readable under every preset combination, in every mode', () => {
    expect(fillProblems(RULES, FILLS)).toEqual([])
  })
})

// ── The locked colours ──────────────────────────────────────────────────────────────────────

describe('TX, the critical orange and the need set stay locked', () => {
  it('no role drives a locked token', () => {
    const hits = PALETTE_ROLES.flatMap((r) => [...r.tokens, ...r.aliases].filter(isLockedToken).map((t) => `${r.id}: ${t}`))
    expect(hits).toEqual([])
  })

  it('a preset block declares only its own role’s tokens — never a locked one, never another role’s', () => {
    expect(blockProblems(RULES)).toEqual([])
  })

  it('stores nothing for a locked colour: every palette key in the tree is a role’s key', () => {
    const src = fileURLToPath(new URL('.', import.meta.url))
    const keys = new Set<string>()
    const walk = (dir: string) => {
      for (const e of readdirSync(dir)) {
        const p = join(dir, e)
        if (statSync(p).isDirectory()) walk(p)
        else if (/\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p)) for (const m of readFileSync(p, 'utf8').matchAll(/nexus-palette-[\w-]+/g)) keys.add(m[0])
      }
    }
    walk(src)
    for (const m of read('../index.html').matchAll(/nexus-palette-[\w-]+/g)) keys.add(m[0])
    expect([...keys].sort()).toEqual(PALETTE_ROLES.map((r) => r.storage).sort())
  })
})

// ── Positive controls: each check can say no ────────────────────────────────────────────────

/** The real sheet plus a preset block of our own, parsed on the same order counter. */
function withBlock(css: string): Rule[] {
  return [...RULES, ...parseRules(css, { n: ORDER.n })]
}

describe('the checks fire', () => {
  it('a RED accent is refused: too close to --tx, and outside the accent window', () => {
    const accent = role('accent')
    const red: PalettePreset = {
      id: 'red',
      labelKey: accent.presets[0].labelKey,
      dark: { '--accent': '#e0444e', '--accent-ink': '#1a0507', '--focus-ring': '#e0444e', '--bubble-mine': '#7a2228', '--bubble-mine-text': '#fdeced' },
      light: { '--accent': '#b3261e', '--accent-ink': '#ffffff', '--focus-ring': '#b3261e', '--bubble-mine': '#b3261e', '--bubble-mine-text': '#ffffff' },
    }
    const rules = withBlock(`
      [data-theme='dark'][data-accent='red'], [data-accent='red'] .well { ${Object.entries(red.dark).map(([k, v]) => `${k}: ${v};`).join(' ')} }
      [data-theme='light'][data-accent='red'] { ${Object.entries(red.light).map(([k, v]) => `${k}: ${v};`).join(' ')} }`)
    expect(parityProblems(rules, accent, red), 'the block did not take — the control proves nothing').toEqual([])
    const keys = roleProblems(rules, accent, red).map((x) => x.key)
    expect(keys).toContain('accent=red dark root: --accent vs --tx')
    expect(keys).toContain('accent=red light well: --accent vs --tx')
    expect(keys).toContain('accent=red dark root: --accent hue')
  })

  it('a LOW-CONTRAST readout is refused on the well and on the page', () => {
    const readout = role('readout')
    const dim: PalettePreset = { id: 'dim', labelKey: readout.presets[0].labelKey, dark: { '--readout': '#3a4656' }, light: { '--readout': '#b8c2cc' } }
    const rules = withBlock(`
      [data-theme='dark'][data-readout='dim'], [data-readout='dim'] .well { --readout: #3a4656; }
      [data-theme='light'][data-readout='dim'] { --readout: #b8c2cc; }`)
    expect(parityProblems(rules, readout, dim)).toEqual([])
    const keys = roleProblems(rules, readout, dim).map((x) => x.key)
    expect(keys).toContain('readout=dim light well: --readout on --well-bg')
    expect(keys).toContain('readout=dim light root: --readout on --panel')
  })

  it('a preset with no well rule is caught at the well’s edge', () => {
    const readout = role('readout')
    const nowell: PalettePreset = { id: 'nowell', labelKey: readout.presets[0].labelKey, dark: { '--readout': '#fdbe45' }, light: { '--readout': '#936823' } }
    const rules = withBlock(`
      [data-theme='dark'][data-readout='nowell'] { --readout: #fdbe45; }
      [data-theme='light'][data-readout='nowell'] { --readout: #936823; }`)
    expect(parityProblems(rules, readout, nowell)).toContain('readout=nowell light well: --readout paints "#4cc9f0", the table says #fdbe45')
  })

  it('an OK green darker than the reds is refused (good must stay lighter than bad)', () => {
    const ok = role('ok')
    const dark: PalettePreset = { id: 'murk', labelKey: ok.presets[0].labelKey, dark: Object.fromEntries(ok.tokens.map((t) => [t, '#2f7a45'])), light: Object.fromEntries(ok.tokens.map((t) => [t, '#1f4d2c'])) }
    const decl = (v: string) => ok.tokens.map((t) => `${t}: ${v};`).join(' ')
    const rules = withBlock(`
      [data-theme='dark'][data-ok='murk'], [data-ok='murk'] .well { ${decl('#2f7a45')} }
      [data-theme='light'][data-ok='murk'] { ${decl('#1f4d2c')} }`)
    expect(parityProblems(rules, ok, dark)).toEqual([])
    expect(roleProblems(rules, ok, dark).map((x) => x.key)).toContain('ok=murk dark root: --snr-strong lightness')
  })

  it('a fill made worse than today is caught', () => {
    const f: Fill = { selector: '.probe', fill: 'var(--accent)', ink: 'var(--accent-ink)' }
    const rules = withBlock(`[data-theme='dark'][data-accent='blue'], [data-accent='blue'] .well { --accent-ink: #5a7090; }`)
    expect(fillProblems(rules, [f]).some((m) => m.startsWith('.probe dark {"accent":"blue"}'))).toBe(true)
  })

  it('a preset block that reaches a locked token, or another role’s, is caught', () => {
    const rules = withBlock(`[data-theme='dark'][data-accent='blue'] { --tx: #4cc9f0; --alert-info: #4cc9f0; }`)
    expect(blockProblems(rules)).toEqual([
      "[data-theme='dark'][data-accent='blue'] { --tx } — a LOCKED colour",
      "[data-theme='dark'][data-accent='blue'] { --alert-info } — not accent's",
    ])
  })

  it('a block for a preset the table does not know, or for the default, is caught', () => {
    const rules = withBlock(`[data-theme='dark'][data-accent='bleu'] { --accent: #6fa4fc; } [data-theme='light'][data-ok='green'] { --rx: #007f35; }`)
    expect(selectorProblems(rules)).toEqual([
      `[data-theme='dark'][data-accent='bleu']: accent has no preset "bleu"`,
      `[data-theme='light'][data-ok='green']: selects ok's default, which is the absence of data-ok`,
    ])
  })
})

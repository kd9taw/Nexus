// THEMES — every built-in theme (features/skins.ts; Settings ▸ Appearance ▸ Theme) is what the sheet
// paints, declares only what a theme may, and reads on every surface in every mode it can be in.
// Operator picks of 2026-09-27: "All ten", "Keep locked inks", "Per-theme Night", "HC wins
// surfaces".
//
// The table in features/skins.ts is the spec; styles.css is the implementation. Nothing here pins a
// theme's hex: every assertion is computed on the cascade winner (cssCascade.ts), for every skin in
// each of its base theme's four modes (day, High contrast, Night, Night + High contrast), bare and
// under every colour-role set (SKIN_MODES), at <html> and INSIDE A DISPLAY WELL.
//
// WHAT HOLDS, and where each rule comes from — the floors are the ones every theme is already held
// to by the suites named, applied to the theme's own tokens:
//   · lettering: --text, --text-dim, --text-faint 4.5:1 on the page and every panel surface, 7:1
//     under High contrast, and --text 7:1 on the panels (styles-theme-cascade, styles-contrast,
//     styles-night-contrast); every need ink 4.5:1 on the panel (styles-theme-cascade);
//   · marks: every status, SNR, band and alert colour 3:1 on the panel and on a well's face, the OK /
//     Amber / Cyan colours 3:1 on the page and the raised surfaces, TX 3:1 on the panel and the page
//     (styles-night-contrast, styles-wells, styles-palette-roles);
//   · wells: the well ink 7:1 on the well's face, its labels 4.5:1 (styles-wells);
//   · the accent and the readout: the rules of styles-palette-roles — lettering 4.5:1, the fill edge
//     and the focus ring 3:1, the ink on an accent fill and both chat bubbles 4.5:1, the digits ΔE ≥
//     0.1 from the unconfirmed ink, ΔE ≥ 0.15 from the TX red, the critical orange and the weak-SNR
//     red (0.06 to a deutan, protan or tritan eye), inside the accent's and the readout's hue
//     windows; and lettering on every role-colour fill no worse than the standard theme gives it;
//   · ON AIR: white ink 4.5:1 on the fill, fill and rim 3:1 off the panel and the page
//     (styles-on-air);
//   · good stays lighter than bad and apart from it to a colour-blind eye (DESIGN.md rule 3);
//   · NIGHT (D3): warm neutral surfaces and inks, no surface brighter than by day, the dark text at
//     most 80% of its day light, the accent and readout dimmer, and every locked colour exactly as
//     by day (styles-night-contrast).
// Plus what is new with themes: the sheet paints exactly the table (PARITY); under High contrast
// the contrast block, not the theme, owns the surfaces and inks (D4); a colour-role pick beats the
// theme's accent and readout; and no theme block declares anything but a theme's tokens — never a
// LOCKED one (D2) — or names a theme the table does not have.
//
// The positive controls at the end splice a bad theme into the real sheet where the themes sit and
// watch each check refuse it: a TX-red accent, a Nord-grey panel (#2e3440: its ON AIR fill stands
// only 2.6:1 off it), a block that names --tx, a stray value, a night that does not dim, a
// contrast-mode surface a theme took over, and an accent fill its own ink cannot be read on.
import { describe, it, expect, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  BASE_MODES,
  CVDS,
  PALETTE_SETS,
  SENTINEL_SKINS,
  SKIN_MODES,
  baseTheme,
  contrast,
  dayOf,
  deltaE,
  expandWith,
  isHigh,
  isNight,
  matchesRoot,
  oklab,
  oklch,
  parseRules,
  rgbHex as hex,
  rolesOf,
  rootTokensFrom,
  simulateCvd,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type BaseMode,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'
import { PALETTE_ROLES, isLockedToken, type PaletteRole } from './features/paletteRoles'
import { MAP_TOKENS, SKINS, SKIN_TOKENS, SKY_TOKENS, STANDARD_MAP, STANDARD_SKY, type Skin } from './features/skins'

// THE BUDGET (2026-10-09). The slowest case here, "an accent fill its own ink cannot be read on is refused", takes
// 0.71 s and 0.65 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const read = (name: string) => readFileSync(fileURLToPath(new URL(`./${name}`, import.meta.url)), 'utf8')
const RAW = read('styles.css')
const PANES = blank(read('cockpit-panes.css'))
const RULES = parseRules(blank(RAW) + '\n' + PANES)

/** Where the themes sit: after the light palette, before the contrast block (skins.ts). */
const THEMES_END = RAW.indexOf('/* ---- FIELD MODE: HIGH-CONTRAST TOKENS')

const TEXT_MIN = 4.5
const TEXT_MIN_HIGH = 7
const PANEL_TEXT_MIN = 7
const STATUS_MIN = 3
const WELL_INK_MIN = 7
const LOCKED_DE = 0.15
const LOCKED_DE_CVD = 0.06
const PENDING_DE = 0.1
const INK_DIM = 0.8
const WARM = [30, 110] as const
const NEUTRAL_C = 0.04
/** One 8-bit step in the green channel moves a mid-tone's contrast by about this much. */
const ROUNDING = 0.05

const SURFACES = ['--bg', '--panel', '--bg-elev', '--bg-elev-2', '--border', '--border-soft']
const INKS = ['--text', '--text-dim', '--text-faint']
const STATUS = [
  '--status-new-entity', '--status-new-band', '--status-new-mode', '--status-worked', '--status-confirmed',
  '--snr-strong', '--snr-marginal', '--snr-weak', '--tx', '--rx', '--band-open', '--band-marginal',
  '--alert-critical', '--alert-warning', '--alert-info',
]
/** The OK, Amber and Cyan colours: 3:1 on the page and the raised surfaces too. */
const SIGNAL_ROLES = ['--snr-strong', '--rx', '--band-open', '--alert-warning', '--snr-marginal', '--band-marginal', '--alert-info']
/** Night never moves these. */
const NIGHT_LOCKED = [...STATUS, '--on-air-bg', '--on-air-ink', '--status-dupe', '--band-closed']
const LOCKED_REDS = ['--tx', '--alert-critical', '--snr-weak'] as const
const ACCENT_HUE = [185, 310] as const
const READOUT_HUE = [70, 320] as const

type Theme = 'dark' | 'light'
type Scope = 'root' | 'well'
const SCOPES: readonly Scope[] = ['root', 'well']
const WELL: El[] = [{ tag: 'div', classes: ['well'], attrs: {} }]

const skinOfMode = (m: Mode) => rolesOf(m).skin
/** `id`'s modes: SKIN_MODES for a theme in the table; for a control, a table theme's of the same base. */
function modesOf(id: string, base: Theme): Mode[] {
  const own = SKIN_MODES.filter((m) => skinOfMode(m) === id)
  if (own.length) return own
  const twin = SKINS.find((s) => s.base === base)!.id
  return SKIN_MODES.filter((m) => skinOfMode(m) === twin).map((m) => m.replace(`skin=${twin}`, `skin=${id}`) as Mode)
}
/** `m` without its theme: the standard theme in the same mode, with the same colour roles. */
const standardOf = (m: Mode): Mode => m.replace(/ skin=[\w-]+/, '') as Mode

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
  // black on a light theme's white passes every floor. So an empty value is a failure here.
  const v = valueOf(tokens, token)
  const c = v === '' ? null : toRgb(v, [0, 0, 0])
  if (!c) throw new Error(`${token} does not resolve to a colour: "${v}"`)
  return c
}
const lum = (c: Rgb) => {
  const f = (v: number) => {
    const s = v / 255
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}
function inHue(h: number, [from, to]: readonly [number, number]): boolean {
  return from <= to ? h >= from && h <= to : h >= from || h <= to
}

interface Problem {
  /** Stable identity, no numbers: what the controls at the end look for. */
  key: string
  message: string
}

/** Everything below a floor for theme `id` (on `base`), in every one of its modes and both scopes. */
function floorProblems(rules: Rule[], id: string, base: Theme): Problem[] {
  const out: Problem[] = []
  const add = (key: string, message: string) => out.push({ key, message })
  for (const mode of modesOf(id, base)) {
    for (const scope of SCOPES) {
      const tk = tokensIn(rules, mode, scope)
      const where = `${mode} ${scope}`
      const floor = (fg: string, bg: string, min: number, kind: string) => {
        const v = contrast(rgbOf(tk, fg), rgbOf(tk, bg))
        if (v < min) add(`${kind}: ${fg} on ${bg}`, `${where}: ${kind} ${fg} ${hex(rgbOf(tk, fg))} on ${bg} ${hex(rgbOf(tk, bg))} = ${v.toFixed(2)}:1 < ${min}:1`)
      }
      const inkMin = isHigh(mode) ? TEXT_MIN_HIGH : TEXT_MIN
      // Lettering: the three inks on the page and every panel surface, --text 7:1 on the panels.
      const textOn = scope === 'well' ? ['--well-bg'] : ['--panel', '--bg', '--bg-elev', '--bg-elev-2']
      for (const ink of INKS) {
        for (const s of textOn) floor(ink, s, ink === '--text' && !isHigh(mode) && scope === 'root' && s !== '--bg' ? PANEL_TEXT_MIN : inkMin, 'text')
      }
      // Marks: 3:1 on the panel or the well's face; the signal roles on the page and raised surfaces
      // too; TX on the panel and the page.
      for (const t of STATUS) floor(t, scope === 'well' ? '--well-bg' : '--panel', STATUS_MIN, 'status')
      if (scope === 'root') for (const t of SIGNAL_ROLES) for (const s of ['--bg', '--bg-elev', '--bg-elev-2']) floor(t, s, STATUS_MIN, 'role colour')
      for (const s of scope === 'well' ? ['--well-bg'] : ['--panel', '--bg']) floor('--tx', s, STATUS_MIN, 'TX')
      // The need inks are lettering on the panel.
      if (scope === 'root') for (const t of [...tk.keys()].filter((k) => k.startsWith('--need-') && k !== '--need-ink')) floor(t, '--panel', TEXT_MIN, 'need ink')
      // Wells: the ink 7:1, the labels 4.5:1.
      if (scope === 'well') {
        floor('--well-ink', '--well-bg', WELL_INK_MIN, 'well ink')
        for (const t of INKS) floor(t, '--well-bg', TEXT_MIN, 'well label')
      }
      // The accent and the readout (styles-palette-roles' rules).
      const textSurf = scope === 'well' ? ['--well-bg', '--bg-elev'] : ['--panel', '--bg-elev', '--bg-elev-2']
      const surf = scope === 'well' ? ['--well-bg', '--bg-elev', '--bg-elev-2'] : ['--panel', '--bg', '--bg-elev', '--bg-elev-2']
      for (const s of textSurf) floor('--accent', s, TEXT_MIN, 'accent lettering')
      if (scope === 'root') floor('--accent', '--bg', STATUS_MIN, 'accent fill edge')
      floor('--accent-ink', '--accent', TEXT_MIN, 'ink on the accent fill')
      floor('--bubble-mine-text', '--bubble-mine', TEXT_MIN, 'your chat bubble')
      floor('--bubble-theirs-text', '--bubble-theirs', TEXT_MIN, 'their chat bubble')
      for (const s of surf) floor('--focus-ring', s, STATUS_MIN, 'focus ring')
      for (const s of textSurf) floor('--readout', s, TEXT_MIN, 'frequency digits')
      {
        const d = deltaE(rgbOf(tk, '--readout'), rgbOf(tk, '--state-pending'))
        if (d < PENDING_DE) add('--readout vs --state-pending', `${where}: the digits ${hex(rgbOf(tk, '--readout'))} are ΔE ${d.toFixed(3)} from the unconfirmed ink ${hex(rgbOf(tk, '--state-pending'))} < ${PENDING_DE}`)
      }
      // Clear of the reds, in normal vision and to each colour-blind eye; inside the hue windows.
      for (const t of ['--accent', '--accent-ink', '--focus-ring', '--bubble-mine', '--bubble-mine-text', '--readout']) {
        const c = rgbOf(tk, t)
        for (const red of LOCKED_REDS) {
          const d = deltaE(c, rgbOf(tk, red))
          if (d < LOCKED_DE) add(`${t} vs ${red}`, `${where}: ${t} ${hex(c)} is ΔE ${d.toFixed(3)} from ${red} ${hex(rgbOf(tk, red))} < ${LOCKED_DE}`)
          for (const cvd of CVDS) {
            const dc = deltaE(simulateCvd(c, cvd), simulateCvd(rgbOf(tk, red), cvd))
            if (dc < LOCKED_DE_CVD) add(`${t} vs ${red} (${cvd})`, `${where}: ${t} ${hex(c)} is ΔE ${dc.toFixed(3)} from ${red} to a ${cvd} eye < ${LOCKED_DE_CVD}`)
          }
        }
        const { C, H } = oklch(c)
        const window = t === '--readout' ? READOUT_HUE : t === '--accent' || t === '--focus-ring' ? ACCENT_HUE : null
        if (window && C >= 0.04 && !inHue(H, window)) add(`${t} hue`, `${where}: ${t} ${hex(c)} hue ${H.toFixed(0)}° is outside ${window[0]}°–${window[1]}°`)
      }
      // ON AIR: white on the fill 4.5:1; the fill and the rim 3:1 off the panel and the page.
      if (scope === 'root') {
        const page = rgbOf(tk, '--bg')
        const fill = toRgb(expandWith(tk, 'var(--on-air-bg)'), page)
        const ink = fill && toRgb(expandWith(tk, 'var(--on-air-ink)'), fill)
        const rim = toRgb(expandWith(tk, 'var(--on-air-rim)'), page)
        if (!fill || !ink || !rim) add('ON AIR resolves', `${where}: the ON AIR sign did not resolve`)
        else {
          const v = contrast(ink, fill)
          if (v < TEXT_MIN) add('ON AIR ink', `${where}: ON AIR ink ${hex(ink)} on the fill ${hex(fill)} = ${v.toFixed(2)}:1 < ${TEXT_MIN}:1`)
          for (const s of ['--panel', '--bg']) {
            const bg = rgbOf(tk, s)
            const e = contrast(fill, bg)
            if (e < STATUS_MIN) add(`ON AIR fill on ${s}`, `${where}: ON AIR fill ${hex(fill)} on ${s} ${hex(bg)} = ${e.toFixed(2)}:1 < ${STATUS_MIN}:1`)
            const r = contrast(rim, bg)
            if (r < STATUS_MIN) add(`ON AIR rim on ${s}`, `${where}: ON AIR rim ${hex(rim)} on ${s} ${hex(bg)} = ${r.toFixed(2)}:1 < ${STATUS_MIN}:1`)
          }
        }
      }
      // Good stays lighter than bad, and apart from it to a colour-blind eye.
      for (const g of ['--snr-strong', '--rx', '--band-open']) {
        for (const red of ['--tx', '--snr-weak']) {
          const [lg, lr] = [oklab(rgbOf(tk, g))[0], oklab(rgbOf(tk, red))[0]]
          if (lg <= lr) add(`${g} lightness`, `${where}: ${g} (L ${lg.toFixed(3)}) is not lighter than ${red} (L ${lr.toFixed(3)})`)
          for (const cvd of CVDS) {
            const dc = deltaE(simulateCvd(rgbOf(tk, g), cvd), simulateCvd(rgbOf(tk, red), cvd))
            if (dc < LOCKED_DE_CVD) add(`${g} vs ${red} (${cvd})`, `${where}: ${g} vs ${red} to a ${cvd} eye ΔE ${dc.toFixed(3)} < ${LOCKED_DE_CVD}`)
          }
        }
      }
      // Night: warm neutrals, less light, the locked colours exactly as by day.
      if (isNight(mode)) {
        const day = tokensIn(rules, dayOf(mode), scope)
        for (const t of [...SURFACES, ...INKS]) {
          const c = rgbOf(tk, t)
          const { C, H } = oklch(c)
          if (C >= 0.005 && (H < WARM[0] || H > WARM[1])) add(`night ${t} warm`, `${where}: ${t} ${hex(c)} is not warm (hue ${H.toFixed(0)}°)`)
          if (C > NEUTRAL_C) add(`night ${t} neutral`, `${where}: ${t} ${hex(c)} is a colour, not a warm neutral (chroma ${C.toFixed(3)})`)
          // "Nothing brighter" is the night guard's rule for the surfaces; a light theme's dark inks
          // are held only by the accent and readout rule below.
          if (SURFACES.includes(t) && lum(c) > lum(rgbOf(day, t)) + 1e-9) add(`night ${t} brighter`, `${where}: ${t} ${hex(c)} is brighter at night than by day ${hex(rgbOf(day, t))}`)
        }
        if (base === 'dark' || scope === 'well') {
          const [yn, yd] = [lum(rgbOf(tk, '--text')), lum(rgbOf(day, '--text'))]
          if (yn > yd * INK_DIM) add('night --text dim', `${where}: --text keeps ${((yn / yd) * 100).toFixed(0)}% of its day light (> ${INK_DIM * 100}%)`)
        }
        for (const t of ['--accent', '--readout']) {
          const strict = t === '--accent' || scope === 'well' || base === 'dark'
          const [yn, yd] = [lum(rgbOf(tk, t)), lum(rgbOf(day, t))]
          if (strict ? yn >= yd : yn > yd) add(`night ${t} dimmer`, `${where}: ${t} ${hex(rgbOf(tk, t))} is ${strict ? 'not dimmer than' : 'brighter than'} by day ${hex(rgbOf(day, t))}`)
        }
        for (const t of NIGHT_LOCKED) {
          if (valueOf(tk, t) !== valueOf(day, t)) add(`night ${t} locked`, `${where}: locked ${t} moved at night (${valueOf(day, t)} → ${valueOf(tk, t)})`)
        }
      }
    }
  }
  return out
}

/** The tokens the contrast blocks take on <html>: D4, High contrast wins them over any theme. */
const HC_TOKENS = [
  ...new Set(
    RULES.filter((r) => BASE_MODES.some((b) => isHigh(b) && matchesRoot(r.selector, b)) && !BASE_MODES.some((b) => !isHigh(b) && matchesRoot(r.selector, b)))
      .flatMap((r) => r.decls.map((d) => d.prop))
      .filter((p) => p.startsWith('--')),
  ),
]

/** What the table says `skin` paints in `base` (no colour role picked) in `scope` — less, under
 *  High contrast, the tokens the contrast blocks take. */
function expectedOf(skin: Skin, base: BaseMode, scope: Scope): Record<string, string> {
  const night = isNight(base)
  const values = scope === 'well' && skin.base === 'light' ? (night ? skin.nightWell : skin.well) : night ? skin.night : skin.day
  return Object.fromEntries(Object.entries(values ?? {}).filter(([t]) => !(isHigh(base) && HC_TOKENS.includes(t))))
}

/** Does the sheet paint exactly the table, at <html> and in a well — and under High contrast,
 *  exactly the contrast block's surfaces and inks, whatever the theme (D4)? */
function parityProblems(rules: Rule[], skin: Skin): string[] {
  const out: string[] = []
  for (const mode of modesOf(skin.id, skin.base).filter((m) => Object.keys(rolesOf(m)).length === 1)) {
    const base = mode.split(' ')[0] as BaseMode
    for (const scope of SCOPES) {
      const tk = tokensIn(rules, mode, scope)
      for (const [t, want] of Object.entries(expectedOf(skin, base, scope))) {
        const got = valueOf(tk, t)
        if (got !== want) out.push(`${mode} ${scope}: ${t} paints "${got}", the table says ${want}`)
      }
      if (isHigh(base)) {
        const standard = tokensIn(rules, base, scope)
        for (const t of HC_TOKENS) {
          if (valueOf(tk, t) !== valueOf(standard, t)) out.push(`${mode} ${scope}: ${t} paints "${valueOf(tk, t)}", High contrast's is ${valueOf(standard, t)} (D4: High contrast wins the surfaces)`)
        }
      }
    }
  }
  return out
}

/** A colour-role pick beats the theme: under every non-default Accent and Readout preset the role's
 *  tokens paint the preset's own values (its night ones at night; its dark ones in a well). */
function rolePickProblems(rules: Rule[], skin: Skin): string[] {
  const out: string[] = []
  for (const r of PALETTE_ROLES.filter((x) => x.dimsAtNight)) {
    for (const p of r.presets.slice(1)) {
      for (const base of BASE_MODES.filter((b) => baseTheme(b) === skin.base)) {
        const mode = withRoles(`${base} skin=${skin.id}` as Mode, { [r.id]: p.id })
        for (const scope of SCOPES) {
          const want = (isNight(base) && p.night ? p.night : p)[scope === 'well' ? 'dark' : skin.base]
          const tk = tokensIn(rules, mode, scope)
          for (const t of r.tokens) if (valueOf(tk, t) !== want[t]) out.push(`${mode} ${scope}: ${t} paints "${valueOf(tk, t)}", ${r.id}=${p.id} says ${want[t]}`)
        }
      }
    }
  }
  return out
}

/** A theme block declares a theme's tokens and nothing else, and names a theme the table has, on
 *  that theme's own base. */
function blockProblems(rules: Rule[]): string[] {
  const bad: string[] = []
  for (const rule of rules) {
    for (const m of rule.selector.matchAll(/\[data-skin='([^']*)'\]/g)) {
      const skin = SKINS.find((s) => s.id === m[1])
      if (!skin) bad.push(`${rule.selector}: no theme "${m[1]}" in features/skins.ts`)
      else {
        const theme = /\[data-theme='(\w+)'\]/.exec(rule.selector)?.[1]
        if (theme && theme !== skin.base) bad.push(`${rule.selector}: ${skin.id} rides on ${skin.base}, not ${theme}`)
      }
      for (const d of rule.decls) {
        if (isLockedToken(d.prop)) bad.push(`${rule.selector} { ${d.prop} } — a LOCKED colour`)
        else if (!SKIN_TOKENS.includes(d.prop) && !(MAP_TOKENS as readonly string[]).includes(d.prop)) bad.push(`${rule.selector} { ${d.prop} } — not a theme's token`)
      }
    }
  }
  return bad
}

// ── Lettering on the role-colour fills ──────────────────────────────────────────────────────

interface Fill {
  selector: string
  fill: string
  ink: string
}
/** Solid role-colour fills with lettering on them, discovered from the sheet (the discovery
 *  styles-palette-roles.test.ts uses), so a new chip is covered the day it lands. */
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
const FILLS = fillsIn(RULES)

/** A fill's element, when its selector is one compound (`.band-chip.active`, `button.cq-skiptx1.on`):
 *  then the cascade itself says what paints it in each mode, a theme-scoped override included
 *  (`[data-theme='light'] .opening-new`). A selector with a combinator or a state is measured on
 *  the values it declares. */
function elOfCompound(sel: string): El | null {
  if (/[\s>+~:]/.test(sel)) return null
  const attrs = Object.fromEntries([...sel.matchAll(/\[([\w-]+)(?:=['"]?([^'"\]]*)['"]?)?\]/g)].map((m) => [m[1], m[2] ?? '']))
  return { tag: /^[a-z][\w-]*/.exec(sel)?.[0] ?? 'div', classes: [...sel.matchAll(/\.([\w-]+)/g)].map((m) => m[1]), attrs }
}
/** The rules that can reach `el` at all: a subject naming one of its classes, or none. The whole
 *  sheet per fill, per mode, per theme was seconds a test; this is the same cascade, smaller. */
const reachable = new WeakMap<Rule[], Map<string, Rule[]>>()
function rulesFor(rules: Rule[], el: El): Rule[] {
  let byKey = reachable.get(rules)
  if (!byKey) reachable.set(rules, (byKey = new Map()))
  const key = JSON.stringify(el)
  let out = byKey.get(key)
  if (!out) {
    out = rules.filter((r) => {
      // The classes a subject REQUIRES: not those inside `:not(…)` or an attribute's value.
      const subject = (r.selector.split(/\s+|\s*>\s*/).pop() ?? '').replace(/:not\([^)]*\)|\[[^\]]*\]/g, '')
      const classes = subject.match(/\.[\w-]+/g)
      return !classes || classes.some((c) => el.classes.includes(c.slice(1)))
    })
    byKey.set(key, out)
  }
  return out
}

/** A theme may not make lettering on a role fill worse than the standard theme in the same mode
 *  gives it — or below 4.5:1 where the standard clears it. */
function fillProblems(rules: Rule[], id: string, base: Theme): string[] {
  const out: string[] = []
  const measure = (f: Fill, mode: Mode) => {
    const tk = tokensIn(rules, mode, 'root')
    const el = elOfCompound(f.selector)
    const fill = (el && winnerAt(rulesFor(rules, el), mode, [el], 'background', 'background-color')?.value) || f.fill
    const paint = (el && winnerAt(rulesFor(rules, el), mode, [el], 'color')?.value) || f.ink
    const bg = toRgb(expandWith(tk, fill), [0, 0, 0])!
    const ink = toRgb(expandWith(tk, paint), bg)!
    return { ratio: contrast(ink, bg), bg, ink }
  }
  for (const mode of modesOf(id, base)) {
    for (const f of FILLS) {
      const standard = measure(f, standardOf(mode)).ratio
      const min = standard >= TEXT_MIN ? TEXT_MIN : standard - ROUNDING
      const now = measure(f, mode)
      if (now.ratio < min) out.push(`${f.selector} ${mode}: ink ${hex(now.ink)} on ${hex(now.bg)} = ${now.ratio.toFixed(2)}:1, the standard theme ${standard.toFixed(2)}:1`)
    }
  }
  return out
}

// ── The table is well formed ────────────────────────────────────────────────────────────────

describe('the theme table', () => {
  it('holds the ten themes, the four rig looks first, each id once', () => {
    expect(SKINS.map((s) => s.id)).toEqual(['amber-lcd', 'green-lcd', 'blue-vfd', 'silver', 'midnight', 'slate', 'lagoon', 'ember', 'nebula', 'paper'])
    expect(SKINS.map((s) => s.family)).toEqual(['rig', 'rig', 'rig', 'rig', 'modern', 'modern', 'modern', 'modern', 'modern', 'modern'])
    expect(SKINS.filter((s) => s.base === 'light').map((s) => s.id)).toEqual(['silver', 'paper'])
  })

  it('gives every theme the same tokens by day and at night: all of them on a dark theme, the wells apart on a light one', () => {
    const wellTrio = ['--well-bg', '--well-ink', '--well-grid']
    const wellPick = ['--accent', '--accent-ink', '--focus-ring', '--readout']
    for (const s of SKINS) {
      const want = s.base === 'dark' ? SKIN_TOKENS : SKIN_TOKENS.filter((t) => !wellTrio.includes(t))
      expect(Object.keys(s.day).sort(), `${s.id} day`).toEqual([...want].sort())
      expect(Object.keys(s.night).sort(), `${s.id} night`).toEqual([...want].sort())
      if (s.base === 'dark') {
        expect(s.well, `${s.id}: a dark theme's well is the theme`).toBeUndefined()
        expect(s.nightWell, s.id).toBeUndefined()
      } else {
        expect(Object.keys(s.well ?? {}).sort(), `${s.id} well`).toEqual([...wellPick].sort())
        expect(Object.keys(s.nightWell ?? {}).sort(), `${s.id} night well`).toEqual([...wellPick].sort())
      }
      for (const v of [s.day, s.night, s.well ?? {}, s.nightWell ?? {}].flatMap((x) => Object.values(x))) expect(v, s.id).toMatch(/^#[0-9a-f]{6}$/)
    }
  })

  it('names no locked colour and no signal colour among a theme’s tokens', () => {
    expect(SKIN_TOKENS.filter(isLockedToken)).toEqual([])
    const signal = PALETTE_ROLES.filter((r) => !r.dimsAtNight).flatMap((r: PaletteRole) => [...r.tokens, ...r.aliases])
    expect(SKIN_TOKENS.filter((t) => signal.includes(t))).toEqual([])
  })

  it('sweeps every theme in each of its base modes, bare and under every colour-role set', () => {
    expect(SKIN_MODES.length).toBe(SKINS.length * 4 * (1 + PALETTE_SETS.length))
    for (const m of SKIN_MODES) expect(baseTheme(m), m).toBe(SKINS.find((s) => s.id === skinOfMode(m))!.base)
  })

  it('hands the other guards the worst case of each base: the lightest dark surfaces, the darkest light ones', () => {
    // MODES carries these (cssCascade.ts SENTINEL_SKINS), so every guard that walks MODES measures
    // its component on the surfaces it reads worst on. Stated here so a table change that moves
    // the worst case shows up as a change of this list, in review.
    expect(SENTINEL_SKINS).toEqual(['silver', 'slate', 'lagoon'])
  })
})

// ── The sheet paints the table ──────────────────────────────────────────────────────────────

describe('every theme paints exactly its table values, at <html> and inside a well', () => {
  it.each(SKINS.map((s) => [s.id, s] as const))('%s', (_n, s) => {
    expect(parityProblems(RULES, s)).toEqual([])
  })

  it.each(SKINS.map((s) => [s.id, s] as const))('%s: an Accent or Readout pick paints the pick', (_n, s) => {
    expect(rolePickProblems(RULES, s)).toEqual([])
  })

  it('no theme block declares anything but a theme’s tokens, names a theme the table lacks, or rides the wrong base', () => {
    expect(blockProblems(RULES)).toEqual([])
  })
})

// ── Readable, visible, clear of the reds ────────────────────────────────────────────────────

describe('every theme clears every floor, in every mode and inside a well', () => {
  it.each(SKINS.map((s) => [s.id, s] as const))('%s', (_n, s) => {
    expect(floorProblems(RULES, s.id, s.base).map((p) => p.message)).toEqual([])
  })
})

describe('lettering on a role-colour fill', () => {
  it('finds the known fills (the discovery cannot silently empty out)', () => {
    expect(FILLS.map((f) => f.selector)).toEqual(expect.arrayContaining(['.band-chip.active', '.theme-chip.active', '.bubble.mine', '.decode-tag.me']))
  })
  it.each(SKINS.map((s) => [s.id, s] as const))('%s keeps it at least as readable as the standard theme', (_n, s) => {
    expect(fillProblems(RULES, s.id, s.base)).toEqual([])
  })
})

// ── The map's basemap (MapView bakes it from these tokens) ───────────────────────────────────

/** What the sheet gives each basemap token at <html> in `mode`, where it is not what `want` says. */
function mapProblems(rules: Rule[], mode: Mode, want: Readonly<Record<string, string>>): string[] {
  const tk = tokensIn(rules, mode, 'root')
  return MAP_TOKENS.filter((t) => valueOf(tk, t) !== want[t]).map((t) => `${mode}: ${t} paints "${valueOf(tk, t)}", wanted ${want[t]}`)
}

describe('the map basemap is a theme’s, and every mode declares all of it', () => {
  it('the standard basemap is declared in both themes, in every standard mode and colour-role set', () => {
    // Both themes declare it (the layout contract), with one value: the same basemap in both.
    const standard = [...BASE_MODES, ...PALETTE_SETS.flatMap((set) => BASE_MODES.map((b): Mode => `${b} ${set}`))]
    expect(standard.flatMap((m) => mapProblems(RULES, m, STANDARD_MAP))).toEqual([])
  })

  it.each(SKINS.map((s) => [s.id, s] as const))('%s paints its own basemap (a dark theme) or the standard one (a light theme), in every mode', (_n, s) => {
    expect(s.base === 'dark' ? Object.keys(s.map ?? {}).sort() : s.map, s.id).toEqual(s.base === 'dark' ? [...MAP_TOKENS].sort() : undefined)
    expect(modesOf(s.id, s.base).flatMap((m) => mapProblems(RULES, m, s.map ?? STANDARD_MAP))).toEqual([])
  })
})

// ── The sun and the moon on the map (MapView and Globe3D paint them from these tokens) ──────────

/** What the sheet gives each sky token at <html> in `mode`, where it is not the standard sky. */
function skyProblems(rules: Rule[], mode: Mode): string[] {
  const tk = tokensIn(rules, mode, 'root')
  return SKY_TOKENS.filter((t) => valueOf(tk, t) !== STANDARD_SKY[t]).map((t) => `${mode}: ${t} paints "${valueOf(tk, t)}", wanted ${STANDARD_SKY[t]}`)
}

describe('the sun and the moon wear the same inks in every theme and mode', () => {
  it('both standard themes declare them, in every standard mode and colour-role set', () => {
    const standard = [...BASE_MODES, ...PALETTE_SETS.flatMap((set) => BASE_MODES.map((b): Mode => `${b} ${set}`))]
    expect(standard.flatMap((m) => skyProblems(RULES, m))).toEqual([])
  })

  it('no built-in theme retunes them (a theme block may not declare one: blockProblems)', () => {
    expect(SKIN_MODES.flatMap((m) => skyProblems(RULES, m))).toEqual([])
  })
})

// ── Positive controls: each check can say no ────────────────────────────────────────────────

/** The real sheet with `css` where the themes sit (after them, so it wins their ties). */
function spliced(css: string): Rule[] {
  return parseRules(blank(RAW.slice(0, THEMES_END)) + '\n' + css + '\n' + blank(RAW.slice(THEMES_END)) + '\n' + PANES)
}
const keysOf = (p: Problem[]) => new Set(p.map((x) => x.key))

describe('the checks fire', () => {
  it('a TX-RED accent is refused: too close to --tx, and outside the accent window', () => {
    const rules = spliced(`[data-theme='dark'][data-skin='bad-red'] { --accent: #e64343; --focus-ring: #e64343; }`)
    const keys = keysOf(floorProblems(rules, 'bad-red', 'dark'))
    expect(keys).toContain('--accent vs --tx')
    expect(keys).toContain('--accent hue')
  })

  it('a NORD-GREY panel (#2e3440) is refused: the ON AIR fill no longer stands 3:1 off it', () => {
    const rules = spliced(`[data-theme='dark'][data-skin='bad-nord'] { --panel: #2e3440; --bg: #242933; }`)
    expect(keysOf(floorProblems(rules, 'bad-nord', 'dark'))).toContain('ON AIR fill on --panel')
  })

  it('a theme block that names --tx, a signal colour or an unknown theme is refused', () => {
    const rules = spliced(
      `[data-theme='dark'][data-skin='amber-lcd'] { --tx: #ff2020; --snr-strong: #00ff00; }\n` +
        `[data-theme='dark'][data-skin='amber'] { --bg: #000000; }\n[data-theme='light'][data-skin='midnight'] { --bg: #000000; }`,
    )
    const bad = blockProblems(rules)
    expect(bad.some((b) => b.includes('--tx } — a LOCKED colour'))).toBe(true)
    expect(bad.some((b) => b.includes("--snr-strong } — not a theme's token"))).toBe(true)
    expect(bad.some((b) => b.includes('no theme "amber"'))).toBe(true)
    expect(bad.some((b) => b.includes('midnight rides on dark, not light'))).toBe(true)
  })

  it('a stray value is caught by the parity check, at <html> and in a well', () => {
    const rules = spliced(`[data-theme='dark'][data-skin='amber-lcd'],\n[data-skin='amber-lcd'] .well { --panel: #15110c; }`)
    const bad = parityProblems(rules, SKINS[0])
    expect(bad.some((b) => b.startsWith('dark skin=amber-lcd root: --panel'))).toBe(true)
    expect(bad.some((b) => b.startsWith('dark skin=amber-lcd well: --panel'))).toBe(true)
  })

  it('a theme that takes a surface from High contrast is caught (D4)', () => {
    const rules = spliced(`[data-theme='dark'][data-contrast='high'][data-night='1'][data-skin='amber-lcd'] { --panel: #14110c; }`)
    expect(parityProblems(rules, SKINS[0]).some((b) => b.includes('D4'))).toBe(true)
  })

  it('a Night that does not dim the accent is refused', () => {
    const rules = spliced(`[data-theme='dark'][data-night='1'][data-skin='amber-lcd'] { --accent: #e9dcc3; }`)
    expect(keysOf(floorProblems(rules, 'amber-lcd', 'dark'))).toContain('night --accent dimmer')
  })

  it('a mode missing a basemap token, or a theme painting another theme’s, is caught', () => {
    const rules = parseRules(blank(RAW).replace(/--map-rim:[^;]*;/g, '') + '\n' + PANES)
    expect(mapProblems(rules, 'dark', STANDARD_MAP)).toEqual([`dark: --map-rim paints "", wanted ${STANDARD_MAP['--map-rim']}`])
    expect(mapProblems(RULES, 'dark skin=lagoon', SKINS.find((x) => x.id === 'slate')!.map!).length).toBeGreaterThan(0)
  })

  it('a mode missing the sun’s ink, or a theme retuning it, is caught', () => {
    const rules = parseRules(blank(RAW).replace(/--map-sun:[^;]*;/g, '') + '\n' + PANES)
    expect(skyProblems(rules, 'light')).toEqual([`light: --map-sun paints "", wanted ${STANDARD_SKY['--map-sun']}`])
    const retuned = spliced(`[data-theme='dark'][data-skin='amber-lcd'] { --map-sun: #ff0000; }`)
    expect(skyProblems(retuned, 'dark skin=amber-lcd')).toEqual([`dark skin=amber-lcd: --map-sun paints "#ff0000", wanted ${STANDARD_SKY['--map-sun']}`])
    expect(blockProblems(retuned)).toContain(`[data-theme='dark'][data-skin='amber-lcd'] { --map-sun } — not a theme's token`)
  })

  it('an accent fill its own ink cannot be read on is refused', () => {
    const rules = spliced(`[data-theme='dark'][data-skin='bad-fill'] { --accent: #0b1a24; }`)
    expect(fillProblems(rules, 'bad-fill', 'dark').length).toBeGreaterThan(0)
  })
})

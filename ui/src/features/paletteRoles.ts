// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The role and preset
// WORDS come from the catalog; the ids and the colour values are code.
//
// THE COLOUR ROLES — the operator's own colours, picked from pre-checked presets
// (Settings ▸ Appearance ▸ Colours). Operator picks of 2026-09-26: "Presets first, hex later",
// "Agent picks, you review", "Locked + solid ON AIR".
//
// A ROLE is a named job a colour does on screen, with the custom properties that do it: the
// Accent (selected chips, focus rings, your own chat bubbles), the Readout (the frequency
// digits), OK / green (good signal, RX, confirmations), Amber (warnings and marginal signal) and
// Cyan (information chips). Each role has a few PRESETS, and presets[0] is the default: today's
// values, exactly, so nothing changes for an operator who never opens the section.
//
// WHAT IS NOT HERE, AND MAY NEVER BE: the transmit red (`--tx`, and the ON AIR sign it derives),
// the critical orange (`--alert-critical`, which must stay the loudest thing on the screen) and
// the need set (`--status-*`, `--need-*`). `LOCKED_TOKENS` names them; styles-palette-roles
// .test.ts proves no role drives one, no preset block declares one, and nothing stores one.
//
// THE MECHANISM, and why each half is where it is:
//   · one attribute per role on <html> (`data-accent='violet'`), ABSENT at the default, so the
//     default is the untouched theme blocks rather than a copy of them;
//   · one webview-local key per role (`nexus-palette-accent`), preseeded in index.html so the
//     first paint is already right (index-preseed.test.ts executes that copy against this one);
//   · one `[data-theme='…'][data-<role>='<preset>']` block per preset per theme in styles.css,
//     plus a `[data-<role>='<preset>'] .well` rule: a display well DECLARES the dark palette on
//     itself, so a preset arriving by inheritance from <html> would stop at the well's edge.
// The DARK values are also what every display well paints, in both themes (DISPLAY WELLS).
//
// THE VALUES BELOW ARE THE SPEC, the sheet is the implementation: styles-palette-roles.test.ts
// resolves the real cascade for every role × preset × mode (dark, light, dark-high, light-high,
// their four Night twins, and inside a well) and fails when a token paints anything other than
// what this table says. NIGHT dims the accent and the readout, each preset to its own `night`
// values (same hue, less light); the OK, Amber and Cyan roles are signal colours and Night
// leaves them exactly as picked.
// The same suite holds every preset to the contrast floors and keeps it clear of the TX red and
// the critical orange; a new preset goes in here AND in the sheet, and the guard says whether it
// is readable. No hex fields this round — a typed colour needs the OKLCH clamp first.

import type { MessageKey } from '../i18n'

export type PaletteRoleId = 'accent' | 'readout' | 'ok' | 'amber' | 'cyan'

/** One theme's values for the tokens a role drives. */
export type PaletteValues = Readonly<Record<string, string>>

export interface PalettePreset {
  /** The invariant token: the attribute value and the stored value. Never translated. */
  id: string
  labelKey: MessageKey
  /** The dark theme's values — also what every display well paints, in either theme. */
  dark: PaletteValues
  light: PaletteValues
  /** Night (useNight.ts, styles.css NIGHT): what this preset paints after dark — dimmer, the
   *  same hue. Only the roles Night dims carry it (`dimsAtNight`), on every preset; `dark` is
   *  also what every well paints at night. */
  night?: { dark: PaletteValues; light: PaletteValues }
}

export interface PaletteRole {
  id: PaletteRoleId
  /** The attribute on <html> that carries a non-default preset. */
  attr: string
  /** The webview-local key that remembers it. Classified SHARED in storage-scope.test.ts. */
  storage: string
  labelKey: MessageKey
  /** The plain-language line: what this colour paints. */
  hintKey: MessageKey
  /** Every custom property this role's presets declare, in declaration order. */
  tokens: readonly string[]
  /** Tokens that follow one of `tokens` by a `var()` in the theme blocks, so a preset reaches
   *  them without declaring them (`--state-good: var(--snr-strong)`). */
  aliases: readonly string[]
  /** The token whose colour the Settings swatch shows. */
  swatch: string
  /** The OKLCH hue window every chromatic value must sit in, degrees, clockwise from → to. */
  hue: readonly [number, number]
  /** Night dims this role (the accent and the readout). The other three are signal colours, and
   *  Night never touches a signal colour: their presets carry no `night` values. */
  dimsAtNight: boolean
  /** presets[0] is the default: today's values. */
  presets: readonly PalettePreset[]
}

// ── The five roles ────────────────────────────────────────────────────────────────────────────
//
// Hue windows. The ACCENT's excludes red, orange, amber and green: those are state colours, and
// an accent in one of them would make a selected chip read as a warning or a good signal. The
// READOUT's excludes red and orange, because the digits turn `--tx` red when the dial leaves the
// operator's privileges — that cue has to stay the only red the number can be. The three state
// roles stay inside their own families: a "green" preset that drifted to yellow would stop saying
// "good" and start saying "marginal".
//
// Light-theme notes, measured by the guard rather than asserted here: the light theme paints
// white, page-coloured and near-black lettering on the amber fills (Tune while keyed, TX armed,
// a new opening), so an Amber preset keeps today's lightness there and moves only its hue; the
// full change shows in the dark theme and in every display well.

const ACCENT: PaletteRole = {
  id: 'accent',
  attr: 'data-accent',
  storage: 'nexus-palette-accent',
  labelKey: 'palette.accent.label',
  hintKey: 'palette.accent.hint',
  tokens: ['--accent', '--accent-ink', '--focus-ring', '--bubble-mine', '--bubble-mine-text'],
  aliases: [],
  swatch: '--accent',
  hue: [185, 310],
  dimsAtNight: true,
  presets: [
    {
      id: 'cyan',
      labelKey: 'palette.preset.cyan',
      dark: {
        '--accent': '#4cc9f0',
        '--accent-ink': '#06222c',
        '--focus-ring': '#4cc9f0',
        '--bubble-mine': '#1f5d73',
        '--bubble-mine-text': '#eaf7fc',
      },
      // Darkened on 2026-09-27 from #0d8ecf, same hue (operator: "Darken it slightly"): that
      // read 3.3–3.6:1 as lettering on the light panels, this reads 4.7:1 or better. The focus
      // ring follows the accent now; it was a shade darker only because #0d8ecf missed 3:1 on
      // the page.
      light: {
        '--accent': '#0174ab',
        '--accent-ink': '#ffffff',
        '--focus-ring': '#0174ab',
        '--bubble-mine': '#0174ab',
        '--bubble-mine-text': '#ffffff',
      },
      night: {
        dark: {
          '--accent': '#42a7c7',
          '--accent-ink': '#001620',
          '--focus-ring': '#42a7c7',
          '--bubble-mine': '#124658',
          '--bubble-mine-text': '#bdc9ce',
        },
        // Darker again on the warm night panel: Night must dim the day cyan above.
        light: {
          '--accent': '#006fae',
          '--accent-ink': '#ffffff',
          '--focus-ring': '#006fae',
          '--bubble-mine': '#006fae',
          '--bubble-mine-text': '#ffffff',
        },
      },
    },
    {
      id: 'blue',
      labelKey: 'palette.preset.blue',
      dark: {
        '--accent': '#6fa4fc',
        '--accent-ink': '#0d1e3b',
        '--focus-ring': '#6fa4fc',
        '--bubble-mine': '#345284',
        '--bubble-mine-text': '#eef4fe',
      },
      light: {
        '--accent': '#255ebc',
        '--accent-ink': '#ffffff',
        '--focus-ring': '#255ebc',
        '--bubble-mine': '#255ebc',
        '--bubble-mine-text': '#ffffff',
      },
      night: {
        dark: {
          '--accent': '#5a86ce',
          '--accent-ink': '#04122e',
          '--focus-ring': '#5a86ce',
          '--bubble-mine': '#243d65',
          '--bubble-mine-text': '#c0c6d0',
        },
        light: {
          '--accent': '#0e49a5',
          '--accent-ink': '#ffffff',
          '--focus-ring': '#0e49a5',
          '--bubble-mine': '#0e49a5',
          '--bubble-mine-text': '#ffffff',
        },
      },
    },
    {
      id: 'violet',
      labelKey: 'palette.preset.violet',
      dark: {
        '--accent': '#ac8ff8',
        '--accent-ink': '#221838',
        '--focus-ring': '#ac8ff8',
        '--bubble-mine': '#56477f',
        '--bubble-mine-text': '#f4f2fd',
      },
      light: {
        '--accent': '#6e44bc',
        '--accent-ink': '#ffffff',
        '--focus-ring': '#6e44bc',
        '--bubble-mine': '#6e44bc',
        '--bubble-mine-text': '#ffffff',
      },
      night: {
        dark: {
          '--accent': '#8c75ca',
          '--accent-ink': '#170c2b',
          '--focus-ring': '#8c75ca',
          '--bubble-mine': '#403461',
          '--bubble-mine-text': '#c6c4cf',
        },
        light: {
          '--accent': '#5b2da5',
          '--accent-ink': '#ffffff',
          '--focus-ring': '#5b2da5',
          '--bubble-mine': '#5b2da5',
          '--bubble-mine-text': '#ffffff',
        },
      },
    },
  ],
}

const READOUT: PaletteRole = {
  id: 'readout',
  attr: 'data-readout',
  storage: 'nexus-palette-readout',
  labelKey: 'palette.readout.label',
  hintKey: 'palette.readout.hint',
  tokens: ['--readout'],
  aliases: [],
  swatch: '--readout',
  hue: [70, 320],
  dimsAtNight: true,
  presets: [
    // The digits were painted in the accent until this role existed, so the default is the
    // accent's own two values: nothing moves for anyone who does not pick one. At night too.
    {
      id: 'cyan',
      labelKey: 'palette.preset.cyan',
      dark: { '--readout': '#4cc9f0' },
      light: { '--readout': '#0174ab' },
      night: { dark: { '--readout': '#42a7c7' }, light: { '--readout': '#006fae' } },
    },
    // At night the digits dim in the dark theme and in every well. Amber dims less than the
    // others: taken as far, it would come within ΔE 0.15 of the critical orange. In the light
    // theme darker ink on a light panel dims nothing, and amber and green darker would come
    // within a deutan eye's ΔE 0.06 of the TX red, so they keep their day ink there.
    {
      id: 'amber',
      labelKey: 'palette.preset.amber',
      dark: { '--readout': '#fdbe45' },
      light: { '--readout': '#936823' },
      night: { dark: { '--readout': '#d9ad48' }, light: { '--readout': '#936823' } },
    },
    {
      id: 'green',
      labelKey: 'palette.preset.green',
      dark: { '--readout': '#70ec90' },
      light: { '--readout': '#11813c' },
      night: { dark: { '--readout': '#62c77b' }, light: { '--readout': '#11813c' } },
    },
    {
      id: 'violet',
      labelKey: 'palette.preset.violet',
      dark: { '--readout': '#bf9bfc' },
      light: { '--readout': '#6e44bc' },
      night: { dark: { '--readout': '#9e80d0' }, light: { '--readout': '#5e31a8' } },
    },
  ],
}

const OK_TOKENS = ['--snr-strong', '--rx', '--band-open'] as const
const okValues = (v: string): PaletteValues => Object.fromEntries(OK_TOKENS.map((t) => [t, v]))

const OK: PaletteRole = {
  id: 'ok',
  attr: 'data-ok',
  storage: 'nexus-palette-ok',
  labelKey: 'palette.ok.label',
  hintKey: 'palette.ok.hint',
  // `--status-confirmed` is NOT here: it belongs to the need set, which is locked. The log's
  // confirmation marks, the journey cells and the delivered-message tick paint `--state-good`.
  tokens: OK_TOKENS,
  aliases: ['--state-good'],
  swatch: '--snr-strong',
  hue: [120, 200],
  dimsAtNight: false,
  presets: [
    { id: 'green', labelKey: 'palette.preset.green', dark: okValues('#69d98d'), light: okValues('#007f35') },
    // Bluish green: further from the reds for a red-green colour-blind operator.
    { id: 'teal', labelKey: 'palette.preset.teal', dark: okValues('#49d9b9'), light: okValues('#007e68') },
    { id: 'mint', labelKey: 'palette.preset.mint', dark: okValues('#84eeb3'), light: okValues('#0d8557') },
  ],
}

const AMBER_TOKENS = ['--alert-warning', '--snr-marginal', '--band-marginal'] as const
const amberValues = (v: string): PaletteValues => Object.fromEntries(AMBER_TOKENS.map((t) => [t, v]))

const AMBER: PaletteRole = {
  id: 'amber',
  attr: 'data-amber',
  storage: 'nexus-palette-amber',
  labelKey: 'palette.amber.label',
  hintKey: 'palette.amber.hint',
  tokens: AMBER_TOKENS,
  aliases: ['--state-ok'],
  swatch: '--snr-marginal',
  hue: [65, 110],
  dimsAtNight: false,
  presets: [
    {
      id: 'amber',
      labelKey: 'palette.preset.amber',
      // Today's two values: the warning ink was authored a shade apart from the marginal one.
      dark: { '--alert-warning': '#f7c243', '--snr-marginal': '#f1ca47', '--band-marginal': '#f1ca47' },
      light: { '--alert-warning': '#a76d00', '--snr-marginal': '#a27000', '--band-marginal': '#a27000' },
    },
    // A deeper, metallic gold (operator, 2026-09-27: "Make them distinct"). It was #f0cf4c and
    // #977500, ΔE 0.012 from Amber in the dark theme and 0.021 from Yellow in the light one. Now at
    // least 0.071 from both in the dark theme and 0.046 in the light one, the same hue in both.
    // The light value could only get duller: lighter fails the page's 3:1 and the keyed Tune
    // button's white ink on it, darker the broadcast button's dark ink and a deutan eye's distance
    // from the TX red, which holds its lightness to Amber's and Yellow's.
    { id: 'gold', labelKey: 'palette.preset.gold', dark: amberValues('#e0b030'), light: amberValues('#8e7640') },
    { id: 'yellow', labelKey: 'palette.preset.yellow', dark: amberValues('#f2e252'), light: amberValues('#8c7900') },
  ],
}

const CYAN: PaletteRole = {
  id: 'cyan',
  attr: 'data-cyan',
  storage: 'nexus-palette-cyan',
  labelKey: 'palette.cyan.label',
  hintKey: 'palette.cyan.hint',
  tokens: ['--alert-info'],
  aliases: [],
  swatch: '--alert-info',
  hue: [190, 300],
  dimsAtNight: false,
  presets: [
    { id: 'sky', labelKey: 'palette.preset.sky', dark: { '--alert-info': '#79c0f1' }, light: { '--alert-info': '#0070a6' } },
    { id: 'cyan', labelKey: 'palette.preset.cyan', dark: { '--alert-info': '#50d9ef' }, light: { '--alert-info': '#10798c' } },
    { id: 'blue', labelKey: 'palette.preset.blue', dark: { '--alert-info': '#7daafd' }, light: { '--alert-info': '#305eb7' } },
  ],
}

/** The roles, in the order Settings lists them. */
export const PALETTE_ROLES: readonly PaletteRole[] = [ACCENT, READOUT, OK, AMBER, CYAN]

/** The colours no preset may touch, and no control or key may exist for: transmit (and the ON
 *  AIR sign derived from it), the critical orange, and the need set. */
export const LOCKED_TOKENS: readonly (string | RegExp)[] = [
  '--tx',
  '--on-air-bg',
  '--on-air-ink',
  '--on-air-rim',
  '--alert-critical',
  /^--status-/,
  /^--need-/,
]

export const isLockedToken = (name: string): boolean =>
  LOCKED_TOKENS.some((l) => (typeof l === 'string' ? l === name : l.test(name)))

/** The preset each role is on. */
export type PaletteSelection = Readonly<Record<PaletteRoleId, string>>

export const defaultPresetId = (role: PaletteRole): string => role.presets[0].id

export const DEFAULT_SELECTION: PaletteSelection = Object.fromEntries(
  PALETTE_ROLES.map((r) => [r.id, defaultPresetId(r)]),
) as PaletteSelection

/** The preset a stored value selects: one this role knows, else the default. A value from a
 *  build with more presets, or a hand-edited one, lands on the default rather than on nothing. */
export function presetIdOf(role: PaletteRole, stored: string | null | undefined): string {
  return role.presets.some((p) => p.id === stored) ? (stored as string) : defaultPresetId(role)
}

/** What the role's attribute carries for `presetId`: null at the default, which is no attribute
 *  at all — the default IS the theme blocks, not a copy of them. */
export function attrValueOf(role: PaletteRole, presetId: string): string | null {
  const id = presetIdOf(role, presetId)
  return id === defaultPresetId(role) ? null : id
}

export function presetOf(role: PaletteRole, presetId: string): PalettePreset {
  return role.presets.find((p) => p.id === presetIdOf(role, presetId))!
}

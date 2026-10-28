// THE JS8 PANES' COLOURS AND FONTS — JS8Call 3.0.3's UI settings for its RX pane, its Band and Call
// Activity tables and its compose box (Configuration ▸ UI, Configuration.ui:3250-4060), painted where
// JS8Call paints them: the RX pane's background and its received and transmitted text
// (mainwindow.cpp:568-569, :3247-3255), the two lists' background, text, selected row and row tints
// (displayBandActivity.cpp:293-357, displayCallActivity.cpp:423-456), the compose box's background and
// text (mainwindow.cpp:570-575), and a font for each.
//
// Nexus's theme tokens are the defaults: the cockpit sets a `--js8-*` custom property only for what the
// operator picked, and every rule that reads one falls back to the token it always had (styles.css).
// A colour is kept for the theme it was picked under, so a choice the operator made readable on Light
// never paints Dark; a font serves both, as in JS8Call. Everything read is checked: a colour is #rrggbb,
// a family one plain name, a size whole points in JS8Call's font dialog range; anything else is the
// theme's.
//
// Display only: nothing here keys.

export type Js8Theme = 'light' | 'dark'

/** Each pane's colours, in the order of JS8Call's tab for it (Messages, Band && Call Activity,
 *  Composition). */
export const JS8_PANE_COLORS = {
  rx: ['rxBg', 'rxFg', 'txFg'],
  lists: ['tableBg', 'tableFg', 'tableSelected', 'directed', 'cq', 'primary', 'secondary'],
  compose: ['composeBg', 'composeFg'],
} as const
/** Each pane's fonts: the RX pane has two, received and transmitted, as JS8Call's Messages tab. */
export const JS8_PANE_FONTS = { rx: ['rx', 'tx'], lists: ['table'], compose: ['compose'] } as const
export type Js8Pane = keyof typeof JS8_PANE_COLORS
export type Js8ColorKey = (typeof JS8_PANE_COLORS)[Js8Pane][number]
export type Js8FontKey = (typeof JS8_PANE_FONTS)[Js8Pane][number]
export const JS8_PANES = Object.keys(JS8_PANE_COLORS) as Js8Pane[]
const COLOR_KEYS = JS8_PANES.flatMap((p) => JS8_PANE_COLORS[p]) as Js8ColorKey[]
const FONT_KEYS = JS8_PANES.flatMap((p) => JS8_PANE_FONTS[p]) as Js8FontKey[]

export interface Js8PaneFont {
  /** One family name; empty is the theme's. */
  family: string
  /** Points, as JS8Call's; 0 is the theme's size. */
  sizePt: number
}
export interface Js8PaneStyle {
  light: Partial<Record<Js8ColorKey, string>>
  dark: Partial<Record<Js8ColorKey, string>>
  fonts: Partial<Record<Js8FontKey, Js8PaneFont>>
}

/** The custom property each colour sets on the cockpit (styles.css reads each with its token). */
export const JS8_COLOR_VAR: Record<Js8ColorKey, string> = {
  rxBg: '--js8-rx-bg',
  rxFg: '--js8-rx-fg',
  txFg: '--js8-tx-fg',
  tableBg: '--js8-table-bg',
  tableFg: '--js8-table-fg',
  tableSelected: '--js8-table-selected',
  directed: '--js8-table-directed',
  cq: '--js8-table-cq',
  primary: '--js8-table-primary',
  secondary: '--js8-table-secondary',
  composeBg: '--js8-compose-bg',
  composeFg: '--js8-compose-fg',
}
/** The two custom properties each font sets, named whole so a search of the code finds each
 *  (token-refs.test.ts). */
export const JS8_FONT_VAR: Record<Js8FontKey, { family: string; size: string }> = {
  rx: { family: '--js8-rx-font-family', size: '--js8-rx-font-size' },
  tx: { family: '--js8-tx-font-family', size: '--js8-tx-font-size' },
  table: { family: '--js8-table-font-family', size: '--js8-table-font-size' },
  compose: { family: '--js8-compose-font-family', size: '--js8-compose-font-size' },
}

/** Qt's font dialog, JS8Call's, offers 6 to 72 points. */
export const JS8_FONT_PT_MIN = 6
export const JS8_FONT_PT_MAX = 72
const HEX = /^#[0-9a-f]{6}$/i
/** One family name as a font dialog lists it: nothing CSS would read as a second family or a string. */
const FAMILY = /^[\p{L}\p{N}][\p{L}\p{N} ._-]{0,47}$/u
/** CSS's generic families, which are written bare. */
const GENERIC = new Set(['serif', 'sans-serif', 'monospace', 'cursive', 'fantasy', 'system-ui', 'ui-serif', 'ui-sans-serif', 'ui-monospace', 'ui-rounded'])

export const emptyJs8PaneStyle = (): Js8PaneStyle => ({ light: {}, dark: {}, fonts: {} })

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

/** A font as the settings hold it, or null when it changes nothing (no family and no size). */
export function js8PaneFont(v: unknown): Js8PaneFont | null {
  if (!isRecord(v)) return null
  const family = typeof v.family === 'string' && FAMILY.test(v.family.trim()) ? v.family.trim() : ''
  const n = v.sizePt
  const sizePt = typeof n === 'number' && Number.isInteger(n) && n >= JS8_FONT_PT_MIN && n <= JS8_FONT_PT_MAX ? n : 0
  return family || sizePt ? { family, sizePt } : null
}

/** The style the settings hold, every entry checked: anything unknown or malformed is the theme's. */
export function parseJs8PaneStyle(v: unknown): Js8PaneStyle {
  const out = emptyJs8PaneStyle()
  if (!isRecord(v)) return out
  for (const theme of ['light', 'dark'] as const) {
    const c = v[theme]
    if (!isRecord(c)) continue
    for (const k of COLOR_KEYS) {
      const hex = c[k]
      if (typeof hex === 'string' && HEX.test(hex)) out[theme][k] = hex.toLowerCase()
    }
  }
  if (isRecord(v.fonts)) {
    for (const k of FONT_KEYS) {
      const f = js8PaneFont(v.fonts[k])
      if (f) out.fonts[k] = f
    }
  }
  return out
}

/** The custom properties for the theme the page paints: only what the operator picked. A family the
 *  computer lacks falls back to the theme's monospace, the face every one of these panes has. */
export function js8PaneVars(style: unknown, theme: Js8Theme): Record<string, string> {
  const s = parseJs8PaneStyle(style)
  const out: Record<string, string> = {}
  for (const [k, hex] of Object.entries(s[theme]) as [Js8ColorKey, string][]) out[JS8_COLOR_VAR[k]] = hex
  for (const [k, f] of Object.entries(s.fonts) as [Js8FontKey, Js8PaneFont][]) {
    if (f.family) {
      const bare = f.family.toLowerCase()
      out[JS8_FONT_VAR[k].family] = `${GENERIC.has(bare) ? bare : `"${f.family}"`}, var(--font-mono)`
    }
    if (f.sizePt) out[JS8_FONT_VAR[k].size] = `calc(${f.sizePt}pt * var(--text-scale, 1))`
  }
  return out
}

/** One colour picked under `theme`, or given back to the theme (null). */
export function withJs8Color(s: Js8PaneStyle, theme: Js8Theme, key: Js8ColorKey, hex: string | null): Js8PaneStyle {
  const colors = { ...s[theme] }
  if (hex !== null && HEX.test(hex)) colors[key] = hex.toLowerCase()
  else delete colors[key]
  return { ...s, [theme]: colors }
}

/** One font set, or given back to the theme (a font that changes nothing). */
export function withJs8Font(s: Js8PaneStyle, key: Js8FontKey, font: Js8PaneFont): Js8PaneStyle {
  const fonts = { ...s.fonts }
  const f = js8PaneFont(font)
  if (f) fonts[key] = f
  else delete fonts[key]
  return { ...s, fonts }
}

/** "Reset": one pane back to the theme — its colours under `theme`, the one being edited, and its
 *  fonts. The other theme's colours for it stay as they are. */
export function resetJs8Pane(s: Js8PaneStyle, pane: Js8Pane, theme: Js8Theme): Js8PaneStyle {
  const colors = { ...s[theme] }
  for (const k of JS8_PANE_COLORS[pane]) delete colors[k]
  const fonts = { ...s.fonts }
  for (const k of JS8_PANE_FONTS[pane]) delete fonts[k]
  return { ...s, [theme]: colors, fonts }
}

/** A colour as #rrggbb or as the browser reports a computed one (`rgb(…)`, opaque `rgba(…)`), or
 *  null for anything else (a see-through one, or one not resolved). */
export function js8Rgb(c: string): [number, number, number] | null {
  const t = c.trim().toLowerCase()
  if (HEX.test(t)) return [1, 3, 5].map((i) => parseInt(t.slice(i, i + 2), 16)) as [number, number, number]
  const m = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/.exec(t)
  if (!m) return null
  if (m[4] !== undefined && (m[4].endsWith('%') ? parseFloat(m[4]) < 100 : parseFloat(m[4]) < 1)) return null
  return [Number(m[1]), Number(m[2]), Number(m[3])]
}

/** WCAG's contrast ratio of two colours, or null when either is not one js8Rgb reads. */
export function js8Contrast(fg: string, bg: string): number | null {
  const a = js8Rgb(fg)
  const b = js8Rgb(bg)
  if (!a || !b) return null
  const lum = ([r, g, bl]: [number, number, number]) => {
    const ch = (v: number) => {
      const s = v / 255
      return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
    }
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(bl)
  }
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

/** WCAG AA for body text: what reads well. */
export const JS8_READABLE = 4.5

/** The theme token each colour starts from: what its picker opens on, and what stands in for a side
 *  of a pair the operator has not picked. A background the pane leaves see-through shows the page's. */
export const JS8_COLOR_TOKEN: Record<Js8ColorKey, string> = {
  rxBg: '--bg',
  rxFg: '--text',
  txFg: '--accent',
  tableBg: '--bg',
  tableFg: '--text',
  tableSelected: '--bg-elev-2',
  directed: '--accent',
  cq: '--snr-strong',
  primary: '--snr-marginal',
  secondary: '--alert-info',
  composeBg: '--bg',
  composeFg: '--text',
}
/** The row colours the theme draws as a see-through wash: one not picked has no colour of its own to
 *  measure (its row reads as the list's background), so only a picked one is checked. */
const WASHES: readonly Js8ColorKey[] = ['directed', 'cq', 'primary', 'secondary']
/** Each text colour and the backgrounds it is drawn on (JS8Call draws a list row's text in the lists'
 *  text colour whatever its row colour). */
const PAIRS: readonly { pane: Js8Pane; fg: Js8ColorKey; bg: Js8ColorKey }[] = [
  { pane: 'rx', fg: 'rxFg', bg: 'rxBg' },
  { pane: 'rx', fg: 'txFg', bg: 'rxBg' },
  ...(['tableBg', 'tableSelected', 'directed', 'cq', 'primary', 'secondary'] as const).map((bg) => ({ pane: 'lists' as const, fg: 'tableFg' as const, bg })),
  { pane: 'compose', fg: 'composeFg', bg: 'composeBg' },
]

export interface Js8LowContrast {
  pane: Js8Pane
  fg: Js8ColorKey
  bg: Js8ColorKey
  ratio: number
}

/** The text that would be hard to read in what the operator picked under `theme`: each pair with a
 *  side picked, the theme's colour standing in for the other (`token` reads one, null when it cannot),
 *  under JS8_READABLE. Nothing picked, nothing to say: the theme's own pairs are the theme's. */
export function js8LowContrast(style: unknown, theme: Js8Theme, token: (name: string) => string | null): Js8LowContrast[] {
  const picked = parseJs8PaneStyle(style)[theme]
  const out: Js8LowContrast[] = []
  for (const { pane, fg, bg } of PAIRS) {
    if (picked[fg] === undefined && picked[bg] === undefined) continue
    if (picked[bg] === undefined && WASHES.includes(bg)) continue
    const ratio = js8Contrast(picked[fg] ?? token(JS8_COLOR_TOKEN[fg]) ?? '', picked[bg] ?? token(JS8_COLOR_TOKEN[bg]) ?? '')
    if (ratio !== null && ratio < JS8_READABLE) out.push({ pane, fg, bg, ratio })
  }
  return out
}

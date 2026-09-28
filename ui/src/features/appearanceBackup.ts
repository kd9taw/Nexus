// APPEARANCE IN THE BACKUP — Settings ▸ Config ▸ Backup & reset. Operator pick of 2026-09-26,
// "Per computer + Backup": how Nexus looks stays webview-local (per computer, preseeded before
// first paint, never a settings.json key), and the backup FILE carries it too, so a new computer
// or a rebuild gets the operator's look back with everything else.
//
// WHERE IT GOES: a top-level `appearance` section of the station's bundle, BESIDE `settings` and
// `uiState` — never inside `settings`, which would make it a settings.json key (and a Remote
// hazard: the hosted page's settings parser requires every key it knows). The station writes the
// bundle (src-tauri `export_settings_bundle`); the UI, which owns these values, adds its section
// to the text before saving it, and reads it back from the text on a restore after the station
// has accepted the rest. The station's import reads `kind`, `schema`, `settings` and `uiState`
// by name, so it passes over the section in either direction: an older Nexus restores the station
// from a newer file and ignores the look, and this one restores an older file that has none.
// The bundle's `schema` stays 1 for exactly that reason — a bump would make every older Nexus
// refuse the file outright.
//
// THE STATION'S BYTES ARE NOT RE-SERIALISED. The section is spliced in before the bundle's final
// brace, so every number the station wrote reaches the file as written — JSON.parse would round an
// integer past 2^53 and rewrite `1.0` as `1`. `withAppearance` checks the splice parses back to the
// same bundle plus the section, and refuses rather than save a file it did not mean to write.
//
// WHAT IS IN IT: the look — the theme choice and the built-in theme riding on it (`skin`, null for
// the standard one), High contrast, Night (Off / On / Auto), text size, density, motion, the five
// colour roles, the two waterfall palettes and the Logbook globe.
// WHAT IS NOT, deliberately: FIELD MODE, a situation rather than a preference (useFieldMode.ts:
// "a backup restoring 'outdoors' onto an indoor session would be wrong more often than right"),
// and UI SCALE, a fact about this computer's screen, per window and clamped to its monitor.
//
// ON A RESTORE every value is checked against what this build knows, and one it cannot read is
// SKIPPED — never replaced by a default, which would overwrite the operator's own value with a
// guess. A field never changes meaning; a new meaning takes a new name, so an older build skips it.

import { DENSITY_STEPS, type Density } from '../useDensity'
import type { Motion } from '../useMotion'
import type { NightChoice } from '../useNight'
import type { TextSize } from '../useTextSize'
import type { ThemeChoice } from '../useTheme'
import { WATERFALL_PALETTES } from '../waterfall'
import { PALETTE_ROLES, type PaletteRoleId } from './paletteRoles'
import { skinOf, type SkinId } from './skins'

export interface Appearance {
  theme: ThemeChoice
  /** The built-in theme on `theme` (features/skins.ts), or null for the standard one. A backup
   *  from before the themes has none, and restores without touching the theme on screen. */
  skin: SkinId | null
  highContrast: boolean
  night: NightChoice
  textSize: TextSize
  density: Density
  motion: Motion
  /** The preset each colour role is on (features/paletteRoles.ts). */
  colours: Partial<Record<PaletteRoleId, string>>
  /** The palette Phone, CW, RTTY and SSTV share, and the FT waterfall's own (waterfallPalette.ts). */
  waterfallPalette: string
  ftWaterfallPalette: string
  logbookGlobe: boolean
}

/** The section's name in the bundle. */
const SECTION = 'appearance'

/** The bundle text with `a` as its appearance section; every byte the station wrote kept. */
export function withAppearance(bundle: string, a: Partial<Appearance>): string {
  const end = bundle.lastIndexOf('}')
  if (end < 0) throw new Error('not a settings bundle')
  // Indented as the station indents a top-level section, so the file reads as one document.
  const section = JSON.stringify(a, null, 2).replace(/\n/g, '\n  ')
  const out = `${bundle.slice(0, end).trimEnd()},\n  "${SECTION}": ${section}\n}\n`
  // The splice must parse back to the station's bundle plus this section, and nothing else.
  const want = JSON.stringify({ ...JSON.parse(bundle), [SECTION]: a })
  if (JSON.stringify(JSON.parse(out)) !== want) throw new Error('the backup could not carry the appearance')
  return out
}

const THEMES: readonly ThemeChoice[] = ['light', 'dark', 'system']
const NIGHTS: readonly NightChoice[] = ['off', 'on', 'auto']
const TEXT_SIZES: readonly TextSize[] = ['normal', 'large', 'larger']
const MOTIONS: readonly Motion[] = ['system', 'reduce']
const PALETTES: readonly string[] = WATERFALL_PALETTES.map((p) => p.value)

// A function, not a generic arrow: i18n/placeholders.test.ts parses every file as TSX, where
// `<T>(…) =>` reads as a `<T>` element and would count as an unreadable call site.
function oneOf<V>(known: readonly V[], v: unknown): v is V {
  return known.includes(v as V)
}

/** The appearance a bundle carries, each value checked; `null` when it carries none. */
export function appearanceIn(bundle: string): Partial<Appearance> | null {
  let raw: unknown
  try {
    raw = (JSON.parse(bundle) as Record<string, unknown>)?.[SECTION]
  } catch {
    return null
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const r = raw as Record<string, unknown>
  const out: Partial<Appearance> = {}
  if (oneOf(THEMES, r.theme)) out.theme = r.theme
  if (r.skin === null || (typeof r.skin === 'string' && skinOf(r.skin))) out.skin = r.skin as SkinId | null
  if (typeof r.highContrast === 'boolean') out.highContrast = r.highContrast
  if (oneOf(NIGHTS, r.night)) out.night = r.night
  if (oneOf(TEXT_SIZES, r.textSize)) out.textSize = r.textSize
  if (oneOf(DENSITY_STEPS, r.density)) out.density = r.density
  if (oneOf(MOTIONS, r.motion)) out.motion = r.motion
  if (r.colours && typeof r.colours === 'object' && !Array.isArray(r.colours)) {
    const picks = r.colours as Record<string, unknown>
    const colours: Partial<Record<PaletteRoleId, string>> = {}
    for (const role of PALETTE_ROLES) {
      const v = picks[role.id]
      if (typeof v === 'string' && role.presets.some((p) => p.id === v)) colours[role.id] = v
    }
    if (Object.keys(colours).length) out.colours = colours
  }
  if (oneOf(PALETTES, r.waterfallPalette)) out.waterfallPalette = r.waterfallPalette
  if (oneOf(PALETTES, r.ftWaterfallPalette)) out.ftWaterfallPalette = r.ftWaterfallPalette
  if (typeof r.logbookGlobe === 'boolean') out.logbookGlobe = r.logbookGlobe
  return out
}

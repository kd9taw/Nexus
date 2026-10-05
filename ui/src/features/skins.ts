// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). A theme's name and line
// come from the catalog; the ids and the colour values are code.
//
// THE BUILT-IN THEMES — "skins" in code, "Theme" in the UI (Settings ▸ Appearance ▸ Theme). Operator
// picks of 2026-09-27 (tasks/themes-and-layout-spec-2026-09-27.md §4): "All ten", "Keep locked inks",
// "Per-theme Night", "HC wins surfaces", "Dark/Light switch", "Yes, on Auto", "Keep these names".
//
// A SKIN IS AN AXIS OVER A BASE, NOT A THIRD THEME. Each one rides on `base`, the `data-theme` it
// sets, so every `[data-theme]` component override in the sheet still applies (the light theme is
// its palette plus ~22 such rules; a third theme would have to copy them forever), and it retunes
// only the palette tokens in the table below. On <html> it is `data-skin='<id>'`, ABSENT for Dark,
// Light and System, which therefore stay byte-identical.
//
// THE MECHANISM, and why each half is where it is:
//   · the THEMES blocks in styles.css, between the light palette and FIELD MODE: by day
//     `[data-theme='<base>'][data-skin='<id>']` (0,2,0), at night the same plus `[data-night='1']`
//     (0,3,0), and a `[data-skin='<id>'] .well` twin of each. They sit BEFORE the contrast, Night
//     and colour-role blocks, so at equal specificity High contrast takes the surfaces and inks
//     and a colour-role pick takes the accent or the readout: a skin is the theme's default, and
//     an explicit pick or a readability mode is stronger;
//   · one webview-local key, `nexus-skin`, preseeded in index.html so the first paint is already
//     right (index-preseed.test.ts executes that copy against this table), and `appearance.skin`
//     in the backup;
//   · useSkin.ts, the one writer of the attribute per document. A skin applies only while the
//     page is its base: picking one sets the theme to its base, and picking Dark, Light or System
//     clears it.
//
// WHAT A SKIN MAY DECLARE: `SKIN_TOKENS` — the surfaces, the three inks, the accent family, the
// frequency readout and the display-well trio — and, on a dark skin, the map's basemap
// (`MAP_TOKENS`, in its day block; a light skin keeps the standard, dark one). NEVER a locked colour (the TX red and the ON AIR
// sign, the critical orange, the status and need sets) and never an OK, Amber or Cyan role token:
// the signal colours stay the theme's, so every colour-role preset holds on every skin.
//
// WELLS. A display well declares the dark palette on itself (styles.css DISPLAY WELLS), so a skin
// says what it paints in one. A DARK skin's well is the skin (its `.well` rule is its own block,
// the dark palette's pattern). A LIGHT skin's well stays the dark instrument palette and takes only
// the skin's accent and readout, in `well` (by day) and `nightWell`.
//
// NIGHT, per theme (D3): the standard warm night surfaces and inks, or the skin's own where they
// are already darker (Midnight keeps its black page), warmed; the accent and readout dimmed about
// 15% in OKLCH lightness at the skin's own hue (7% on a light skin, which the locked inks hold up).
//
// THE VALUES ARE THE SPEC, THE SHEET IS THE IMPLEMENTATION — the colour roles' rule.
// styles-skins.test.ts resolves the real cascade for every skin in each of its base's modes and
// every colour-role set, at <html> and inside a well, and fails when a token paints anything but
// this table, when a block declares a token it may not, or when anything reads below the floors
// every theme is held to (text 4.5:1, 7:1 under High contrast; status marks, TX and the ON AIR
// sign 3:1; the well ink 7:1; ΔE 0.15 from the reds, 0.06 to a colour-blind eye; Night warm and
// dimmer). The values were measured legal by the themes spec before they were written here.
//
// ONE DELIBERATE CHANGE FROM THE SPEC'S TABLE: Silver's page, #dfe3e8 → #e2e6eb, and with it its
// night page, which Night's derivation now makes the standard light night's #eae4da. The Needed
// board letters its mode cells in the page colour, and #dfe3e8 read 4.45:1 on them by day and
// 4.09:1 at night (styles-theme-cascade.test.ts); #e2e6eb reads 4.58:1, and 4.54:1 at night.

import type { ColormapName } from '../colormaps'
import type { MessageKey } from '../i18n'
import type { Theme } from '../useTheme'

export type SkinId =
  | 'amber-lcd'
  | 'green-lcd'
  | 'blue-vfd'
  | 'silver'
  | 'midnight'
  | 'slate'
  | 'lagoon'
  | 'ember'
  | 'nebula'
  | 'paper'

/** The gallery's two groups: looks that echo real radios, and soft editor-style palettes. */
export type SkinFamily = 'rig' | 'modern'

/** Token → value, plain hexes (the map and the canvases read these tokens as strings). */
export type SkinValues = Readonly<Record<string, string>>

export interface Skin {
  /** The invariant token: the attribute value and the stored value. Never translated. */
  id: SkinId
  family: SkinFamily
  /** Its name on the gallery card, and the one-line personality under it. */
  labelKey: MessageKey
  lineKey: MessageKey
  /** The theme this skin rides on: what picking it sets `tempo-theme` to. */
  base: Theme
  /** What a waterfall or scope on the Auto palette paints under it (resolveColormap). Night still
   *  turns Auto Amber CRT, and a palette picked by name is never changed (operator, "Yes, on Auto"). */
  waterfall: ColormapName
  /** By day, at <html> — and, on a dark skin, inside every display well. */
  day: SkinValues
  /** A light skin's display wells by day: the accent and readout only (the well stays dark). */
  well?: SkinValues
  /** At night: every token `day` retunes, dimmer and warmer. */
  night: SkinValues
  /** A light skin's display wells at night. */
  nightWell?: SkinValues
  /** A dark skin's map basemap (MAP_TOKENS), in its day block and so at night too. A light
   *  skin keeps the standard one. */
  map?: SkinValues
}

/** The tokens a skin may declare, and the only ones. */
export const SKIN_TOKENS: readonly string[] = [
  '--bg',
  '--panel',
  '--bg-elev',
  '--bg-elev-2',
  '--border',
  '--border-soft',
  '--text',
  '--text-dim',
  '--text-faint',
  '--accent',
  '--accent-ink',
  '--focus-ring',
  '--bubble-mine',
  '--bubble-mine-text',
  '--bubble-theirs',
  '--bubble-theirs-text',
  '--readout',
  '--well-bg',
  '--well-ink',
  '--well-grid',
]

/** The map's basemap: the tokens MapView bakes its land, sea, coasts, borders and globe from. */
export const MAP_TOKENS = [
  '--map-ocean',
  '--map-land',
  '--map-land-globe',
  '--map-coast',
  '--map-state',
  '--map-rim',
  '--map-ocean-lit',
  '--map-ocean-deep',
] as const
export type MapToken = (typeof MAP_TOKENS)[number]

/** The standard basemap, in both standard themes and on a light built-in theme: a map should read
 *  as a MAP (filled land and sea), not a wireframe. A light atlas palette under the shaded relief
 *  (operator pick 2026-10-04: "lighter look"), so the greyline's night shading reads strongly on
 *  the day side, the same in every theme. MapView also paints it where no sheet is loaded, and the
 *  3-D globes paint their texture from it. */
export const STANDARD_MAP: Readonly<Record<MapToken, string>> = {
  '--map-ocean': '#bcd3e3', // the sea and the lakes: a soft blue-grey
  '--map-land': '#eeebe2', // warm paper land under the relief (the flat World and AEQD maps)
  '--map-land-globe': '#e3dfd3', // a step darker on the 2-D globe, so its shading reads as a sphere
  '--map-coast': '#5f7f96', // coastlines and borders (and the grid and range rings): steel blue, crisp
  '--map-state': '#9aa3aa', // US state lines, quieter than the coast and still readable
  '--map-rim': '#82a3bc', // the globe's edge, and the rivers: a step deeper than the sea
  '--map-ocean-lit': '#d5e5f0', // the globe's lit sea, toward the light
  '--map-ocean-deep': '#8fb0c6', // the globe's limb, its shaded edge
}

/** The sun and the moon on the map (MapView, Globe3D; drawn by features/skyGlyphs). The same inks in
 *  every theme and mode: the sun is the sun, and both are drawn over the basemap with the markers'
 *  dark halo. So no theme may declare one (a theme block declares a theme's tokens only), and
 *  styles.css MAP SKY holds the one table. */
export const SKY_TOKENS = ['--map-sun', '--map-moon-lit', '--map-moon-dark'] as const
export type SkyToken = (typeof SKY_TOKENS)[number]
export const STANDARD_SKY: Readonly<Record<SkyToken, string>> = {
  '--map-sun': '#ffd166', // the disc, its rays and its glow: a warm gold
  '--map-moon-lit': '#ece8da', // the lit part of the moon, and the thin line of moonlight on its rim
  '--map-moon-dark': '#3a4150', // the unlit part: slate, a step above the sea so a new moon still shows
}

/** The gallery's order: the rig looks, then the modern ones. */
export const SKINS: readonly Skin[] = [
  {
    id: 'amber-lcd',
    family: 'rig',
    labelKey: 'theme.amberLcd.label',
    lineKey: 'theme.amberLcd.line',
    base: 'dark',
    waterfall: 'amber-crt',
    day: {
      '--bg': '#0c0a07', '--panel': '#14110c', '--bg-elev': '#1a160f', '--bg-elev-2': '#221c13',
      '--border': '#3a3020', '--border-soft': '#2a2418', '--text': '#f0e6d2', '--text-dim': '#bfae90',
      '--text-faint': '#9a8b70', '--accent': '#e9dcc3', '--accent-ink': '#1c1408', '--focus-ring': '#e9dcc3',
      '--bubble-mine': '#565a3a', '--bubble-mine-text': '#f7efe0', '--bubble-theirs': '#221c13',
      '--bubble-theirs-text': '#f0e6d2', '--readout': '#fdbe45', '--well-bg': '#0c0a07', '--well-ink': '#f0e6d2',
      '--well-grid': '#3d3424',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#c0b49b', '--accent-ink': '#1c1408', '--focus-ring': '#c0b49b',
      '--bubble-mine': '#444728', '--bubble-mine-text': '#c8c0b2', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#d9ad48', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#332c23',
    },
    map: {
      '--map-ocean': '#171310',
      '--map-land': '#3a3122',
      '--map-land-globe': '#241e14',
      '--map-coast': '#8a7a55',
      '--map-state': '#5f5440',
      '--map-rim': '#3a3020',
      '--map-ocean-lit': '#2a241a',
      '--map-ocean-deep': '#0a0806',
    },
  },
  {
    id: 'green-lcd',
    family: 'rig',
    labelKey: 'theme.greenLcd.label',
    lineKey: 'theme.greenLcd.line',
    base: 'dark',
    waterfall: 'sdr-green',
    day: {
      '--bg': '#070a08', '--panel': '#0e130f', '--bg-elev': '#131a14', '--bg-elev-2': '#1a231b',
      '--border': '#24382a', '--border-soft': '#1a2a1e', '--text': '#dfeadf', '--text-dim': '#a3b7a6',
      '--text-faint': '#86998a', '--accent': '#cfe3d1', '--accent-ink': '#0c1a10', '--focus-ring': '#cfe3d1',
      '--bubble-mine': '#2f5a3d', '--bubble-mine-text': '#eaf6ec', '--bubble-theirs': '#1a231b',
      '--bubble-theirs-text': '#dfeadf', '--readout': '#70ec90', '--well-bg': '#070a08', '--well-ink': '#dfeadf',
      '--well-grid': '#26382b',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#a7baa9', '--accent-ink': '#0c1a10', '--focus-ring': '#a7baa9',
      '--bubble-mine': '#1d482c', '--bubble-mine-text': '#bbc7bd', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#62c77b', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#312a21',
    },
    map: {
      '--map-ocean': '#0c1410',
      '--map-land': '#22382a',
      '--map-land-globe': '#152419',
      '--map-coast': '#6f9a7a',
      '--map-state': '#4a6b55',
      '--map-rim': '#24382a',
      '--map-ocean-lit': '#173226',
      '--map-ocean-deep': '#060a08',
    },
  },
  {
    id: 'blue-vfd',
    family: 'rig',
    labelKey: 'theme.blueVfd.label',
    lineKey: 'theme.blueVfd.line',
    base: 'dark',
    waterfall: 'blue',
    day: {
      '--bg': '#06090f', '--panel': '#0c1220', '--bg-elev': '#111a2d', '--bg-elev-2': '#182440',
      '--border': '#263a5e', '--border-soft': '#1b2b47', '--text': '#e6eefc', '--text-dim': '#93a1bb',
      '--text-faint': '#8393b3', '--accent': '#7fb6ff', '--accent-ink': '#061a3a', '--focus-ring': '#7fb6ff',
      '--bubble-mine': '#2a4d8a', '--bubble-mine-text': '#eaf2ff', '--bubble-theirs': '#182440',
      '--bubble-theirs-text': '#e6eefc', '--readout': '#8dc8ff', '--well-bg': '#06090f', '--well-ink': '#e6eefc',
      '--well-grid': '#2a3f66',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#5f94db', '--accent-ink': '#061a3a', '--focus-ring': '#5f94db',
      '--bubble-mine': '#193b76', '--bubble-mine-text': '#bbc3cf', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#6aa4d9', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#3c352c',
    },
    map: {
      '--map-ocean': '#0b1a33',
      '--map-land': '#2a3a55',
      '--map-land-globe': '#182640',
      '--map-coast': '#7f97bd',
      '--map-state': '#546b8f',
      '--map-rim': '#263a5e',
      '--map-ocean-lit': '#1a3660',
      '--map-ocean-deep': '#05091a',
    },
  },
  {
    id: 'silver',
    family: 'rig',
    labelKey: 'theme.silver.label',
    lineKey: 'theme.silver.line',
    base: 'light',
    waterfall: 'viridis',
    day: {
      '--bg': '#e2e6eb', '--panel': '#f9fafb', '--bg-elev': '#f9fafb', '--bg-elev-2': '#f3f5f7',
      '--border': '#9aa3ad', '--border-soft': '#b9c1ca', '--text': '#1b1f24', '--text-dim': '#4a525c',
      '--text-faint': '#5b636d', '--accent': '#3b5a7a', '--accent-ink': '#ffffff', '--focus-ring': '#3b5a7a',
      '--bubble-mine': '#3b5a7a', '--bubble-mine-text': '#ffffff', '--bubble-theirs': '#e6e9ed',
      '--bubble-theirs-text': '#1b1f24', '--readout': '#143d6b',
    },
    well: {
      '--accent': '#a9c0d8', '--accent-ink': '#0b1a2a', '--focus-ring': '#a9c0d8', '--readout': '#9ccbff',
    },
    night: {
      '--bg': '#eae4da', '--panel': '#fcf9f1', '--bg-elev': '#fcf9f1', '--bg-elev-2': '#f8f4ed',
      '--border': '#a39a90', '--border-soft': '#bfb7ae', '--text': '#201c17', '--text-dim': '#554f48',
      '--text-faint': '#5f5851', '--accent': '#325171', '--accent-ink': '#ffffff', '--focus-ring': '#325171',
      '--bubble-mine': '#325171', '--bubble-mine-text': '#ffffff', '--bubble-theirs': '#e2dfdb',
      '--bubble-theirs-text': '#201c17', '--readout': '#0c3664',
    },
    nightWell: {
      '--accent': '#879db4', '--accent-ink': '#0b1a2a', '--focus-ring': '#879db4', '--readout': '#78a6d8',
    },
  },
  {
    id: 'midnight',
    family: 'modern',
    labelKey: 'theme.midnight.label',
    lineKey: 'theme.midnight.line',
    base: 'dark',
    waterfall: 'inferno',
    day: {
      '--bg': '#000000', '--panel': '#0a0a0c', '--bg-elev': '#101014', '--bg-elev-2': '#17171c',
      '--border': '#2a2a33', '--border-soft': '#1c1c22', '--text': '#ececf1', '--text-dim': '#a9a9b6',
      '--text-faint': '#8a8a98', '--accent': '#4cc9f0', '--accent-ink': '#06222c', '--focus-ring': '#4cc9f0',
      '--bubble-mine': '#1f5d73', '--bubble-mine-text': '#eaf7fc', '--bubble-theirs': '#17171c',
      '--bubble-theirs-text': '#ececf1', '--readout': '#4cc9f0', '--well-bg': '#000000', '--well-ink': '#ececf1',
      '--well-grid': '#2a2a33',
    },
    night: {
      '--bg': '#000000', '--panel': '#080706', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#42a7c7', '--accent-ink': '#001620', '--focus-ring': '#42a7c7',
      '--bubble-mine': '#124658', '--bubble-mine-text': '#bdc9ce', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#42a7c7', '--well-bg': '#000000', '--well-ink': '#bfb4a5',
      '--well-grid': '#29231c',
    },
    map: {
      '--map-ocean': '#05070c',
      '--map-land': '#1a1f24',
      '--map-land-globe': '#111518',
      '--map-coast': '#5a6570',
      '--map-state': '#3d464f',
      '--map-rim': '#1e242b',
      '--map-ocean-lit': '#0f1a26',
      '--map-ocean-deep': '#000000',
    },
  },
  {
    id: 'slate',
    family: 'modern',
    labelKey: 'theme.slate.label',
    lineKey: 'theme.slate.line',
    base: 'dark',
    waterfall: 'viridis',
    day: {
      '--bg': '#14181f', '--panel': '#1b2029', '--bg-elev': '#222833', '--bg-elev-2': '#2a3140',
      '--border': '#3f4859', '--border-soft': '#323a49', '--text': '#e5ebf3', '--text-dim': '#a6b2c2',
      '--text-faint': '#9aa7b8', '--accent': '#88c4d6', '--accent-ink': '#0e2a33', '--focus-ring': '#88c4d6',
      '--bubble-mine': '#3e6f7f', '--bubble-mine-text': '#eef8fb', '--bubble-theirs': '#2a3140',
      '--bubble-theirs-text': '#e5ebf3', '--readout': '#66cff8', '--well-bg': '#14181f', '--well-ink': '#e5ebf3',
      '--well-grid': '#3f4859',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#66a1b3', '--accent-ink': '#0e2a33', '--focus-ring': '#66a1b3',
      '--bubble-mine': '#275968', '--bubble-mine-text': '#bfc8cb', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#3dabd2', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#443c33',
    },
    map: {
      '--map-ocean': '#1b2330',
      '--map-land': '#3a4656',
      '--map-land-globe': '#26303d',
      '--map-coast': '#8797aa',
      '--map-state': '#5c6d82',
      '--map-rim': '#3a4656',
      '--map-ocean-lit': '#283a4f',
      '--map-ocean-deep': '#0f141b',
    },
  },
  {
    id: 'lagoon',
    family: 'modern',
    labelKey: 'theme.lagoon.label',
    lineKey: 'theme.lagoon.line',
    base: 'dark',
    waterfall: 'viridis',
    day: {
      '--bg': '#001a20', '--panel': '#02252d', '--bg-elev': '#063039', '--bg-elev-2': '#0b3a44',
      '--border': '#1a5560', '--border-soft': '#0f434e', '--text': '#e3eeeb', '--text-dim': '#9fb9b3',
      '--text-faint': '#8aa5a1', '--accent': '#66d1d6', '--accent-ink': '#04252a', '--focus-ring': '#66d1d6',
      '--bubble-mine': '#135a63', '--bubble-mine-text': '#e6f7f7', '--bubble-theirs': '#0b3a44',
      '--bubble-theirs-text': '#e3eeeb', '--readout': '#66e2ea', '--well-bg': '#001a20', '--well-ink': '#e3eeeb',
      '--well-grid': '#1a5560',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#3eadb2', '--accent-ink': '#04252a', '--focus-ring': '#3eadb2',
      '--bubble-mine': '#004851', '--bubble-mine-text': '#b7c8c8', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#37bbc3', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#474037',
    },
    map: {
      '--map-ocean': '#04303a',
      '--map-land': '#1e4a3d',
      '--map-land-globe': '#123328',
      '--map-coast': '#6fa39a',
      '--map-state': '#4d7a72',
      '--map-rim': '#1a5560',
      '--map-ocean-lit': '#0f4a56',
      '--map-ocean-deep': '#001318',
    },
  },
  {
    id: 'ember',
    family: 'modern',
    labelKey: 'theme.ember.label',
    lineKey: 'theme.ember.line',
    base: 'dark',
    waterfall: 'inferno',
    day: {
      '--bg': '#1a1613', '--panel': '#221d19', '--bg-elev': '#2a2420', '--bg-elev-2': '#332c27',
      '--border': '#4a3f37', '--border-soft': '#3a322c', '--text': '#efe3d0', '--text-dim': '#c2b39c',
      '--text-faint': '#a3947e', '--accent': '#89c4c0', '--accent-ink': '#0d2624', '--focus-ring': '#89c4c0',
      '--bubble-mine': '#3f6360', '--bubble-mine-text': '#eaf5f4', '--bubble-theirs': '#332c27',
      '--bubble-theirs-text': '#efe3d0', '--readout': '#f5b83d', '--well-bg': '#1a1613', '--well-ink': '#efe3d0',
      '--well-grid': '#4a3f37',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#67a29e', '--accent-ink': '#0d2624', '--focus-ring': '#67a29e',
      '--bubble-mine': '#2c4f4c', '--bubble-mine-text': '#bbc6c5', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#d9ad48', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#3e362e',
    },
    map: {
      '--map-ocean': '#1f1b18',
      '--map-land': '#4a3d2f',
      '--map-land-globe': '#2e261e',
      '--map-coast': '#a08a6a',
      '--map-state': '#6f5f4c',
      '--map-rim': '#4a3f37',
      '--map-ocean-lit': '#33291f',
      '--map-ocean-deep': '#0f0c0a',
    },
  },
  {
    id: 'nebula',
    family: 'modern',
    labelKey: 'theme.nebula.label',
    lineKey: 'theme.nebula.line',
    base: 'dark',
    waterfall: 'inferno',
    day: {
      '--bg': '#16151e', '--panel': '#1e1c28', '--bg-elev': '#262333', '--bg-elev-2': '#2f2b3f',
      '--border': '#47415f', '--border-soft': '#38334d', '--text': '#eee9fb', '--text-dim': '#a69dbf',
      '--text-faint': '#9990b4', '--accent': '#b79bfa', '--accent-ink': '#22183a', '--focus-ring': '#b79bfa',
      '--bubble-mine': '#56477f', '--bubble-mine-text': '#f4f2fd', '--bubble-theirs': '#2f2b3f',
      '--bubble-theirs-text': '#eee9fb', '--readout': '#bf9bfc', '--well-bg': '#16151e', '--well-ink': '#eee9fb',
      '--well-grid': '#47415f',
    },
    night: {
      '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410',
      '--border': '#302822', '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284',
      '--text-faint': '#8a8073', '--accent': '#8c75ca', '--accent-ink': '#170c2b', '--focus-ring': '#8c75ca',
      '--bubble-mine': '#403461', '--bubble-mine-text': '#c6c4cf', '--bubble-theirs': '#191410',
      '--bubble-theirs-text': '#bfb4a5', '--readout': '#9e80d0', '--well-bg': '#090604', '--well-ink': '#bfb4a5',
      '--well-grid': '#423a32',
    },
    map: {
      '--map-ocean': '#1a1730',
      '--map-land': '#3a3255',
      '--map-land-globe': '#26213d',
      '--map-coast': '#8c7fc0',
      '--map-state': '#5f568a',
      '--map-rim': '#47415f',
      '--map-ocean-lit': '#2b2652',
      '--map-ocean-deep': '#0c0a18',
    },
  },
  {
    id: 'paper',
    family: 'modern',
    labelKey: 'theme.paper.label',
    lineKey: 'theme.paper.line',
    base: 'light',
    waterfall: 'cividis',
    day: {
      '--bg': '#ede7d8', '--panel': '#fcfaf5', '--bg-elev': '#fcfaf5', '--bg-elev-2': '#f7f5f0',
      '--border': '#c9c0ab', '--border-soft': '#ddd5c2', '--text': '#2b2618', '--text-dim': '#5b5445',
      '--text-faint': '#6a6353', '--accent': '#23629f', '--accent-ink': '#ffffff', '--focus-ring': '#23629f',
      '--bubble-mine': '#23629f', '--bubble-mine-text': '#ffffff', '--bubble-theirs': '#f1ece0',
      '--bubble-theirs-text': '#2b2618', '--readout': '#1f5a90',
    },
    well: {
      '--accent': '#7dc4ff', '--accent-ink': '#06222c', '--focus-ring': '#7dc4ff', '--readout': '#7dc4ff',
    },
    night: {
      '--bg': '#eae4da', '--panel': '#fcf9f1', '--bg-elev': '#fcf9f1', '--bg-elev-2': '#f8f4ed',
      '--border': '#c5bcb0', '--border-soft': '#d6cec3', '--text': '#26211a', '--text-dim': '#554f48',
      '--text-faint': '#5f5851', '--accent': '#175894', '--accent-ink': '#ffffff', '--focus-ring': '#175894',
      '--bubble-mine': '#175894', '--bubble-mine-text': '#ffffff', '--bubble-theirs': '#efe9df',
      '--bubble-theirs-text': '#26211a', '--readout': '#145186',
    },
    nightWell: {
      '--accent': '#5aa0d9', '--accent-ink': '#06222c', '--focus-ring': '#5aa0d9', '--readout': '#5aa0d9',
    },
  },
]

export const skinOf = (id: string | null | undefined): Skin | undefined => SKINS.find((s) => s.id === id)

/** The skin that paints under `theme` when `stored` is the operator's pick: that skin while the
 *  page is its base, else none. A value this build does not know is none, never a guess. */
export function activeSkin(stored: string | null | undefined, theme: Theme): SkinId | null {
  const s = skinOf(stored)
  return s && s.base === theme ? s.id : null
}

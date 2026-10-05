// THE STREET MAP'S STYLE: the Protomaps basemap's own MapLibre layers, repainted in Nexus's theme
// tokens, read off <html> the way MapView reads its own (so Night, High contrast, the built-in
// themes and the colour-role presets reach the street map as they reach everything else).
//
// What is repainted: the ground, water, roads and their casings, buildings, rail, boundaries, and
// every label with its halo, so the map reads as part of Nexus in either theme. What keeps the
// basemap flavor's own colour: the land-use tints (parks, woods, schools…) and the POI icons, which
// each flavor already tunes for its own light or dark ground.
//
// LANDCOVER IS NOT DRAWN. Its data is ESA WorldCover (CC BY 4.0), which would need a credit of its
// own; the map credits OpenStreetMap only. `layers()` emits the landcover layer only when the
// flavor carries a `landcover` table, so the repainted flavor has none.
//
// Every URL in the style is local: the pack through `pmtiles://`, glyphs and sprites through
// `street-asset://` (features/streetPack.ts). Nothing here can name the network.

import { language_script_pairs, layers, namedFlavor, type Flavor } from '@protomaps/basemaps'
import type { StyleSpecification } from 'maplibre-gl'
import { assetUrl, packUrl, type StreetPack } from './streetPack'

/** The colour keys of a basemap flavor (the rest are font names and nested tables). */
type FlavorColour = Exclude<keyof Flavor, 'regular' | 'bold' | 'italic' | 'pois' | 'landcover'>

/** Each Nexus token the street map is painted with, and the flavor colours it paints. Every
 *  token is declared in both themes (streetStyle.test.ts resolves each one in every mode). */
export const STREET_PAINT = {
  // The ground, and the halo every label wears so it reads off the ground.
  '--bg': [
    'background',
    'earth',
    'roads_label_minor_halo',
    'roads_label_major_halo',
    'subplace_label_halo',
    'city_label_halo',
    'state_label_halo',
    'address_label_halo',
  ],
  // Streets one step up from the ground; main roads and highways a step further.
  '--bg-elev': ['other', 'minor_service', 'minor_a', 'minor_b', 'link', 'bridges_other', 'bridges_minor', 'bridges_link'],
  '--bg-elev-2': ['major', 'highway', 'bridges_major', 'bridges_highway'],
  // The edge that keeps a road apart from the ground and from the road it crosses; rail.
  '--border': [
    'minor_service_casing',
    'minor_casing',
    'link_casing',
    'major_casing_late',
    'highway_casing_late',
    'major_casing_early',
    'highway_casing_early',
    'bridges_other_casing',
    'bridges_minor_casing',
    'bridges_link_casing',
    'bridges_major_casing',
    'bridges_highway_casing',
    'tunnel_other_casing',
    'tunnel_minor_casing',
    'tunnel_link_casing',
    'tunnel_major_casing',
    'tunnel_highway_casing',
    'railway',
  ],
  // Built things that are not roads, and roads underground.
  '--border-soft': [
    'buildings',
    'pier',
    'runway',
    'tunnel_other',
    'tunnel_minor',
    'tunnel_link',
    'tunnel_major',
    'tunnel_highway',
  ],
  // Labels, strongest to faintest: towns, then main roads and regions, then streets and numbers.
  '--text': ['city_label', 'country_label'],
  '--text-dim': ['roads_label_major', 'state_label', 'subplace_label', 'ocean_label'],
  '--text-faint': ['roads_label_minor', 'address_label', 'boundaries'],
} as const satisfies Record<string, readonly FlavorColour[]>

/** Water is the Cyan role's ink, mostly ground: blue in every preset, and as dark or light as the
 *  ground under it. A fifth of the ink keeps the water labels at 4.5:1 over it in every mode. */
const WATER_INK = '--alert-info'
const WATER_SHARE = 0.2

export type StreetToken = keyof typeof STREET_PAINT | typeof WATER_INK
export const STREET_TOKENS: readonly StreetToken[] = [...(Object.keys(STREET_PAINT) as StreetToken[]), WATER_INK]

/** What the page paints right now: its light or dark base, and each street token's value. */
export interface StreetLook {
  base: 'light' | 'dark'
  tokens: Record<StreetToken, string>
}

/** The source the layers draw from. */
export const STREET_SOURCE = 'protomaps'

const HEX = /^#[0-9a-f]{6}$/i

function rgbOf(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16)
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

/** `share` of `ink` over `ground`, as #rrggbb; null if either is not a #rrggbb colour. */
function mix(ink: string, ground: string, share: number): string | null {
  if (!HEX.test(ink) || !HEX.test(ground)) return null
  const a = rgbOf(ink)
  const b = rgbOf(ground)
  return `#${a.map((v, i) => Math.round(v * share + b[i] * (1 - share)).toString(16).padStart(2, '0')).join('')}`
}

/** The light or dark flavor with Nexus's tokens painted over it, and no landcover. A token that is
 *  not a #rrggbb colour leaves the flavor's own colour rather than an invalid one. */
export function streetFlavor({ base, tokens }: StreetLook): Flavor {
  const flavor: Flavor = { ...namedFlavor(base) }
  delete flavor.landcover
  for (const [token, keys] of Object.entries(STREET_PAINT)) {
    const value = tokens[token as StreetToken]
    if (!HEX.test(value)) continue
    for (const key of keys) flavor[key] = value
  }
  flavor.water = mix(tokens[WATER_INK], tokens['--bg'], WATER_SHARE) ?? flavor.water
  return flavor
}

const LABEL_LANGUAGES = new Set(language_script_pairs.map((p) => p.lang))

/** The label language for a UI locale: the locale itself, else its language, else English. Place
 *  names are names, not technical tokens, so they follow the operator's language. */
export function labelLanguage(locale: string): string {
  if (LABEL_LANGUAGES.has(locale)) return locale
  const language = locale.split('-')[0]
  return LABEL_LANGUAGES.has(language) ? language : 'en'
}

/** The complete offline style for one pack. */
export function streetStyle(pack: StreetPack, look: StreetLook, lang: string): StyleSpecification {
  return {
    version: 8,
    glyphs: assetUrl('fonts/{fontstack}/{range}.pbf'),
    sprite: assetUrl(`sprites/${look.base}`),
    sources: { [STREET_SOURCE]: { type: 'vector', url: packUrl(pack) } },
    layers: layers(STREET_SOURCE, streetFlavor(look), { lang }),
  }
}

/** The look `root` paints now. `data-theme` is always light or dark (useTheme resolves System). */
export function readStreetLook(root: HTMLElement = document.documentElement): StreetLook {
  const css = getComputedStyle(root)
  const tokens = Object.fromEntries(STREET_TOKENS.map((t) => [t, css.getPropertyValue(t).trim()]))
  return {
    base: root.getAttribute('data-theme') === 'light' ? 'light' : 'dark',
    tokens: tokens as Record<StreetToken, string>,
  }
}

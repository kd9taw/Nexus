// @vitest-environment jsdom
//
// The street map's style: Nexus's tokens on the basemap's own layers. Three questions, each
// computed rather than pattern-matched:
//   · does each token reach the layers it is meant to paint, with no basemap colour left there;
//   · is landcover gone (and would it be there without the repaint, so the check can fail);
//   · does every token resolve to a usable colour in EVERY theme mode the sheet can produce, with
//     the labels readable over their halos — styles.css resolved through the shared cascade.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { layers, namedFlavor } from '@protomaps/basemaps'
import type { LayerSpecification } from 'maplibre-gl'
import { MODES, contrast, expandWith, parseHex, parseRules, rootTokensFrom } from '../cssCascade'
import { assetPath, packUrl, type StreetPack } from './streetPack'
import {
  labelLanguage,
  readStreetLook,
  STREET_SOURCE,
  STREET_TOKENS,
  streetFlavor,
  streetStyle,
  type StreetLook,
  type StreetToken,
} from './streetStyle'

const pack: StreetPack = {
  id: 'home-200km',
  name: 'Home',
  bbox: [-98.2, 38.4, -97.0, 39.3],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 5_070_601,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
}

/** One colour per token, all different, so a colour on a layer names the token that put it there. */
const MARKED: Record<StreetToken, string> = {
  '--bg': '#101010',
  '--bg-elev': '#202020',
  '--bg-elev-2': '#303030',
  '--border': '#404040',
  '--border-soft': '#505050',
  '--text': '#606060',
  '--text-dim': '#707070',
  '--text-faint': '#808080',
  '--alert-info': '#0000ff',
}
/** 20% of the Cyan ink over the ground — the water. */
const MARKED_WATER = '#0d0d40'

const look = (base: 'light' | 'dark', tokens = MARKED): StreetLook => ({ base, tokens })
const layerById = (ls: LayerSpecification[], id: string) => {
  const l = ls.find((x) => x.id === id)
  if (!l) throw new Error(`no layer ${id}`)
  return l
}
/** Every #rrggbb colour a paint property can produce, expressions included. */
const coloursOf = (l: LayerSpecification, prop: string) =>
  [...new Set(JSON.stringify((l as { paint?: Record<string, unknown> }).paint?.[prop]).match(/#[0-9a-f]{6}/gi) ?? [])]

describe('the repaint', () => {
  // [layer, paint property, the token whose colour it must carry — and only that colour]
  const EXPECTED: Array<[string, string, StreetToken | 'water']> = [
    ['background', 'background-color', '--bg'],
    ['earth', 'fill-color', '--bg'],
    ['water', 'fill-color', 'water'],
    ['water_river', 'line-color', 'water'],
    ['roads_minor', 'line-color', '--bg-elev'],
    ['roads_other', 'line-color', '--bg-elev'],
    ['roads_bridges_minor', 'line-color', '--bg-elev'],
    ['roads_major', 'line-color', '--bg-elev-2'],
    ['roads_highway', 'line-color', '--bg-elev-2'],
    ['roads_minor_casing', 'line-color', '--border'],
    ['roads_highway_casing_early', 'line-color', '--border'],
    ['roads_bridges_major_casing', 'line-color', '--border'],
    ['roads_rail', 'line-color', '--border'],
    ['buildings', 'fill-color', '--border-soft'],
    ['roads_tunnels_major', 'line-color', '--border-soft'],
    ['places_locality', 'text-color', '--text'],
    ['places_locality', 'text-halo-color', '--bg'],
    ['places_country', 'text-color', '--text'],
    ['roads_labels_major', 'text-color', '--text-dim'],
    ['roads_labels_major', 'text-halo-color', '--bg'],
    ['places_region', 'text-color', '--text-dim'],
    ['water_label_lakes', 'text-color', '--text-dim'],
    ['water_label_lakes', 'text-halo-color', 'water'],
    ['roads_labels_minor', 'text-color', '--text-faint'],
    ['roads_labels_minor', 'text-halo-color', '--bg'],
    ['address_label', 'text-color', '--text-faint'],
    ['boundaries', 'line-color', '--text-faint'],
  ]

  for (const base of ['dark', 'light'] as const) {
    it(`paints each ${base} layer with its token, and nothing of the basemap's own is left there`, () => {
      const ls = streetStyle(pack, look(base), 'en').layers
      for (const [id, prop, token] of EXPECTED) {
        const want = token === 'water' ? MARKED_WATER : MARKED[token]
        expect({ id, prop, colours: coloursOf(layerById(ls, id), prop) }).toEqual({ id, prop, colours: [want] })
      }
    })
  }

  it('covers every token it reads', () => {
    // The water mix is the only place --alert-info lands.
    const used = new Set(EXPECTED.map(([, , token]) => (token === 'water' ? '--alert-info' : token)))
    expect([...used].sort()).toEqual([...STREET_TOKENS].sort())
  })

  it('keeps the flavor colour when a token is not a #rrggbb colour, never an invalid one', () => {
    const flavor = streetFlavor(look('dark', { ...MARKED, '--bg': '', '--alert-info': 'oklch(70% 0.1 230)' }))
    expect(flavor.earth).toBe(namedFlavor('dark').earth)
    expect(flavor.water).toBe(namedFlavor('dark').water)
    expect(flavor.major).toBe(MARKED['--bg-elev-2'])
  })

  it('draws no landcover, which the stock flavors would draw', () => {
    const isLandcover = (l: LayerSpecification) =>
      l.id === 'landcover' || (l as { 'source-layer'?: string })['source-layer'] === 'landcover'
    // Positive control: without the repaint the layer is there, so this check can fail.
    expect(layers(STREET_SOURCE, namedFlavor('light'), { lang: 'en' }).some(isLandcover)).toBe(true)
    for (const base of ['dark', 'light'] as const) {
      expect(streetStyle(pack, look(base), 'en').layers.some(isLandcover)).toBe(false)
    }
  })
})

describe('the style is offline', () => {
  it('takes the pack, glyphs and sprites from local schemes only', () => {
    const style = streetStyle(pack, look('light'), 'en')
    expect(style.sources).toEqual({ [STREET_SOURCE]: { type: 'vector', url: packUrl(pack) } })
    expect(style.glyphs).toBe('street-asset://fonts/{fontstack}/{range}.pbf')
    expect(style.sprite).toBe('street-asset://sprites/light')
    expect(streetStyle(pack, look('dark'), 'en').sprite).toBe('street-asset://sprites/dark')
    expect(JSON.stringify(style)).not.toMatch(/https?:/)
    for (const l of style.layers) {
      if (l.type !== 'background') expect((l as { source?: string }).source).toBe(STREET_SOURCE)
    }
  })

  it('names the asset files the Rust half serves', () => {
    const style = streetStyle(pack, look('light'), 'en')
    // What MapLibre derives from the style, then what street_map_asset is asked for.
    const glyph = style.glyphs!.replace('{fontstack}', 'Noto Sans Regular').replace('{range}', '0-255')
    expect(assetPath(glyph)).toBe('fonts/Noto Sans Regular/0-255.pbf')
    const sheet = new URL(style.sprite as string)
    sheet.pathname += '@2x.png'
    expect(assetPath(sheet.toString())).toBe('sprites/light@2x.png')
  })

  it('names only the three fonts the assets carry', () => {
    const fonts = new Set<string>()
    for (const l of streetStyle(pack, look('dark'), 'en').layers) {
      const font = (l as { layout?: Record<string, unknown> }).layout?.['text-font']
      for (const name of JSON.stringify(font ?? null).match(/Noto Sans [A-Za-z]+/g) ?? []) fonts.add(name)
    }
    expect([...fonts].sort()).toEqual(['Noto Sans Italic', 'Noto Sans Medium', 'Noto Sans Regular'])
  })
})

describe('the tokens exist in every theme mode', () => {
  // Under jsdom Vite rewrites `new URL(…, import.meta.url)` into an asset URL, so the sheet is read
  // from the working directory, as styles-on-air.test.ts does.
  const sheet = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(
    /\/\*[\s\S]*?\*\//g,
    (m) => m.replace(/[^\n]/g, ' '),
  )
  const rules = parseRules(sheet)

  it(`resolves every street token to #rrggbb, with readable labels, in all ${MODES.length} modes`, () => {
    const problems: string[] = []
    for (const mode of MODES) {
      const declared = rootTokensFrom(rules, mode)
      const tokens = Object.fromEntries(
        STREET_TOKENS.map((t) => [t, expandWith(declared, `var(${t})`).trim().toLowerCase()]),
      ) as Record<StreetToken, string>
      for (const t of STREET_TOKENS) if (!/^#[0-9a-f]{6}$/.test(tokens[t])) problems.push(`${mode}: ${t} = "${tokens[t]}"`)
      if (problems.length) continue
      // Every label sits on a halo of the ground, except the water labels, whose halo is the water.
      const flavor = streetFlavor(look(mode.startsWith('light') ? 'light' : 'dark', tokens))
      const rgb = (hex: string) => parseHex(hex)!
      const pairs: Array<[string, string, string]> = [
        ['--text on the ground', flavor.city_label, flavor.city_label_halo],
        ['--text-dim on the ground', flavor.roads_label_major, flavor.roads_label_major_halo],
        ['--text-faint on the ground', flavor.roads_label_minor, flavor.roads_label_minor_halo],
        ['--text-dim on the water', flavor.ocean_label, flavor.water],
      ]
      for (const [what, fg, bg] of pairs) {
        const c = contrast(rgb(fg), rgb(bg))
        if (c < 4.5) problems.push(`${mode}: ${what} ${fg} on ${bg} = ${c.toFixed(2)}:1`)
      }
    }
    expect(problems).toEqual([])
  })
})

describe('reading the look off the page', () => {
  it('reads the base from data-theme and each token from the computed style', () => {
    const root = document.documentElement
    root.setAttribute('data-theme', 'light')
    for (const t of STREET_TOKENS) root.style.setProperty(t, MARKED[t])
    expect(readStreetLook()).toEqual({ base: 'light', tokens: MARKED })
    root.setAttribute('data-theme', 'dark')
    expect(readStreetLook().base).toBe('dark')
    root.removeAttribute('style')
    root.removeAttribute('data-theme')
  })
})

describe('labelLanguage', () => {
  it("labels places in the operator's language when the basemap has it, else English", () => {
    expect(labelLanguage('en')).toBe('en')
    expect(labelLanguage('de')).toBe('de')
    expect(labelLanguage('ja')).toBe('ja')
    expect(labelLanguage('pt-BR')).toBe('pt')
    expect(labelLanguage('zh-Hant')).toBe('zh-Hant')
    expect(labelLanguage('tlh')).toBe('en')
  })
})

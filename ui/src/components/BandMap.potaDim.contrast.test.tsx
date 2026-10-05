// @vitest-environment jsdom
//
// THE DIM POTA COLOUR READS AS A MARK, AND AS DIMMER THAN THE NEW-PARK COLOUR, IN EVERY THEME
// (operator, 2026-10-03, "Dim POTA color for all"; the precedence is pinned in BandMap.potaDim.test.tsx).
//
// A tick is a mark, not text, so its floor is 3:1 against what it sits on: the track of the Band
// activity strip and of the pop-out band map. The new-park colour it must stay dimmer than is the
// POTA green (`--need-pota`), which a park still to be worked keeps. The two themes leave it very
// different room: on the dark tracks that green reads about 9.4 to 11.5:1, so the dim colour can
// give up most of its light and its colour; on the light tracks it reads about 4.6 to 4.9:1, so a
// dim colour has little light to give up before the floor and gives up more of its colour
// (chroma). "Dimmer" is held as all three: less contrast on the track than the new-park colour,
// at most 60 % of its chroma, and a visible step away from it (ΔE_OK ≥ 0.06, three just-noticeable
// differences).
//
// AND A STEP IN LIGHTNESS (operator, 2026-10-05, "Add a brightness difference too"). An operator who cannot tell
// two greens apart by their colour tells them apart by their light. The light theme's pair differed mostly in
// colour, 0.04 apart in OKLab L, when its new-park green read only 3.6 to 3.9:1 on the light tracks; that green
// is now darker, and the two are held at least 0.09 apart in every theme.
//
// The strip and the map are rendered and the cascade is resolved on their own chains with the app's
// own resolver (cssCascade.ts), in every base mode (day and night, standard and high contrast), bare
// and on every built-in theme of its base. The colour-role presets are left out, and the guard says
// why: none of them sets a token this measures.
import { describe, it, expect, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { BandMap } from './BandMap'
import { BandStrip } from './BandStrip'
import { PALETTE_ROLES } from '../features/paletteRoles'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  baseTheme,
  chainOf,
  contrast,
  deltaE,
  expandWith,
  oklch,
  parseHex,
  parseRules,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import type { NeedTag, SpotRow } from '../types'

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The sheet as it shipped before the dim colour: its class and both theme declarations gone. */
const SHIPPED = RULES.filter((r) => r.selector !== '.pota-dim').map((r) => ({ ...r, decls: r.decls.filter((d) => d.prop !== '--pota-dim') }))
/** The light theme's declaration lost, so the light themes fall through to the dark value — the
 *  bug the need palette's light block once had (styles.css, beside `--need-pota`). */
const DARK_ONLY = RULES.map((r) =>
  r.selector === "[data-theme='light']" ? { ...r, decls: r.decls.filter((d) => d.prop !== '--pota-dim') } : r,
)

/** The light theme's new-park green as it shipped until 2026-10-05, 0.04 darker than the dim colour. */
const LIGHT_AS_SHIPPED = RULES.map((r) =>
  r.selector === "[data-theme='light']" ? { ...r, decls: r.decls.map((d) => (d.prop === '--need-pota' ? { ...d, value: '#15803d' } : d)) } : r,
)

const MARK_MIN = 3
/** The least step in OKLab lightness between a dim mark and the new-park colour: four and a half just-noticeable
 *  differences (0.02 each), more than twice the light theme's old 0.04. */
const LIGHTNESS_STEP = 0.09
const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every base mode, bare and on every built-in theme of its base. */
const MODES: Mode[] = BASE_MODES.flatMap((b) => skinsOf(baseTheme(b)).map((skin) => (skin ? withRoles(b, { skin }) : b)))
const LIGHT = MODES.filter((m) => baseTheme(m) === 'light')
const DARK = MODES.filter((m) => baseTheme(m) === 'dark')

function spot(call: string, freqMhz: number): SpotRow {
  return { call, entity: '', zone: 0, band: '20m', freqMhz, mode: 'Phone', spotter: 'W3LPL', corroborators: [], ageSecs: 10, comment: '', licensed: true }
}
// K1WRK is activating a park already worked today (dim), K1NEW one still to be worked (the full
// colour), and K1PLN is not activating (plain).
const SPOTS = [spot('K1WRK', 14.205), spot('K1NEW', 14.22), spot('K1PLN', 14.24)]
const NEEDS = new Map<string, NeedTag>([['K1NEW', 'NewPark']])
const TYPES = new Map<string, 'Pota' | 'Sota' | 'Dxped'>([
  ['K1WRK', 'Pota'],
  ['K1NEW', 'Pota'],
])

const BODY: El = { tag: 'body', classes: [], attrs: {} }
const at = (node: Element | null | undefined, what: string): El[] => {
  if (!node) throw new Error(`not rendered: ${what}`)
  return [BODY, ...chainOf(node)]
}

/** The chains a mark is painted on: the track, the dim mark(s), the new-park tick, and a plain
 *  station's marks in the same places as the dim ones. */
interface Surface {
  name: string
  track: El[]
  dim: El[][]
  park: El[]
  plain: El[][]
}

function renderSurfaces(): Surface[] {
  const strip = render(
    <div className="app">
      <div className="pane-body">
        <BandStrip band="20m" dialMhz={14.2} txAllowed spots={SPOTS} needByCall={NEEDS} typeByCall={TYPES} onWorkSpot={() => {}} />
      </div>
    </div>,
  ).container
  const stripTick = (call: string) =>
    [...strip.querySelectorAll('.bandstrip-spot')].find((b) => b.textContent?.includes(call))?.querySelector('.bandstrip-tick')
  const out: Surface[] = [
    {
      name: 'Band activity',
      track: at(strip.querySelector('.bandstrip-track'), 'the strip track'),
      dim: [at(stripTick('K1WRK'), 'the dim strip tick')],
      park: at(stripTick('K1NEW'), 'the new-park strip tick'),
      plain: [at(stripTick('K1PLN'), 'the plain strip tick')],
    },
  ]
  cleanup()
  // The pop-out mounts the map straight under its shell (DetachedPanel's DetachedShell).
  const map = render(
    <div className="app detached">
      <BandMap band="20m" dialMhz={14.2} txAllowed spots={SPOTS} needByCall={NEEDS} typeByCall={TYPES} onWorkSpot={() => {}} />
    </div>,
  ).container
  const mapSpot = (call: string) => [...map.querySelectorAll('.bandmap-spot')].find((b) => b.textContent?.includes(call))
  out.push({
    name: 'the band map',
    track: at(map.querySelector('.bandmap-track'), 'the map track'),
    // The tick at the true frequency, and the label's left edge beside it.
    dim: [at(mapSpot('K1WRK')?.previousElementSibling, 'the dim map tick'), at(mapSpot('K1WRK'), 'the dim map label')],
    park: at(mapSpot('K1NEW')?.previousElementSibling, 'the new-park map tick'),
    plain: [at(mapSpot('K1PLN')?.previousElementSibling, 'the plain map tick'), at(mapSpot('K1PLN'), 'the plain map label')],
  })
  cleanup()
  return out
}

const chainKey = (chain: El[]) => JSON.stringify(chain.map((e) => [e.tag, e.classes]))
const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const tokensFor = (rules: Rule[], mode: Mode, chain: El[]) => once(rules, `t|${mode}|${chainKey(chain)}`, () => tokensAt(rules, mode, chain))
const colourOf = (rules: Rule[], mode: Mode, chain: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(tokensFor(rules, mode, chain), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
/** The colour the winning declaration paints: a background, or a border shorthand's colour. */
function paint(rules: Rule[], mode: Mode, chain: El[], under: Rgb): Rgb {
  const w = winnerAt(rules, mode, chain, 'background', 'background-color', 'border-left', 'border-left-color')
  if (!w) throw new Error(`${chainKey(chain)} ${mode}: paints nothing`)
  return colourOf(rules, mode, chain, w.value.replace(/^\S+\s+solid\s+/, ''), under)
}

interface Measured {
  name: string
  track: Rgb
  dim: Rgb[]
  park: Rgb
  plain: Rgb[]
  token: Rgb | null
}

function measure(rules: Rule[], mode: Mode, surfaces: Surface[]): Measured[] {
  return surfaces.map((s) => {
    const panel = colourOf(rules, mode, s.track, 'var(--panel)', [0, 0, 0])
    const track = paint(rules, mode, s.track, panel)
    const declared = rootTokensFrom(rules, mode).get('--pota-dim')
    return {
      name: s.name,
      track,
      dim: s.dim.map((c) => paint(rules, mode, c, track)),
      park: paint(rules, mode, s.park, track),
      plain: s.plain.map((c) => paint(rules, mode, c, track)),
      token: declared ? parseHex(declared) : null,
    }
  })
}

const SURFACES = renderSurfaces()

/** Every dim mark that does not paint its theme's own `--pota-dim`. */
function notTheToken(rules: Rule[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const mode of modes)
    for (const m of measure(rules, mode, SURFACES))
      for (const d of m.dim)
        if (!m.token || hex(d) !== hex(m.token)) out.push(`${m.name} ${mode}: ${hex(d)}, token ${m.token ? hex(m.token) : 'undeclared'}`)
  return out
}

/** Every dim mark under 3:1 on its track. */
function unreadable(rules: Rule[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const mode of modes)
    for (const m of measure(rules, mode, SURFACES))
      for (const d of m.dim) {
        const r = contrast(d, m.track)
        if (r < MARK_MIN) out.push(`${m.name} ${mode}: ${hex(d)} on ${hex(m.track)} = ${r.toFixed(2)}:1`)
      }
  return out
}

/** Every dim mark that is not clearly dimmer than the new-park tick beside it. */
function notDimmer(rules: Rule[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const mode of modes)
    for (const m of measure(rules, mode, SURFACES))
      for (const d of m.dim) {
        const [cd, cp] = [contrast(d, m.track), contrast(m.park, m.track)]
        const chroma = oklch(d).C / oklch(m.park).C
        const step = deltaE(d, m.park)
        if (!(cd < cp) || chroma > 0.6 || step < 0.06)
          out.push(
            `${m.name} ${mode}: ${hex(d)} ${cd.toFixed(2)}:1 vs new-park ${hex(m.park)} ${cp.toFixed(2)}:1, ` +
              `chroma ${(chroma * 100).toFixed(0)} %, ΔE ${step.toFixed(3)}`,
          )
      }
  return out
}

/** Every dim mark less than a clear step in lightness from the new-park tick beside it. */
function tooAlike(rules: Rule[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const mode of modes)
    for (const m of measure(rules, mode, SURFACES))
      for (const d of m.dim) {
        const [ld, lp] = [oklch(d).L, oklch(m.park).L]
        if (Math.abs(ld - lp) < LIGHTNESS_STEP)
          out.push(`${m.name} ${mode}: ${hex(d)} L ${ld.toFixed(3)} vs new-park ${hex(m.park)} L ${lp.toFixed(3)}`)
      }
  return out
}

describe('the dim POTA colour on Band activity and its band map, in every theme', () => {
  it('renders the marks it measures (the census cannot silently empty out)', () => {
    expect(SURFACES.map((s) => [s.name, s.dim.length])).toEqual([
      ['Band activity', 1],
      ['the band map', 2],
    ])
    expect(MODES).toHaveLength(BASE_MODES.length + SKINS.length * 4)
  })

  it('is a theme token, declared in both themes', () => {
    const dark = rootTokensFrom(RULES, 'dark').get('--pota-dim')
    const light = rootTokensFrom(RULES, 'light').get('--pota-dim')
    expect(dark).toMatch(/^#[0-9a-f]{6}$/i)
    expect(light).toMatch(/^#[0-9a-f]{6}$/i)
    expect(light, 'the light theme overrides the dark value').not.toBe(dark)
  })

  it('every dim mark paints its theme’s own token', () => {
    expect(notTheToken(RULES, MODES)).toEqual([])
  }, 60_000)

  it('every dim mark clears 3:1 on its track', () => {
    expect(unreadable(RULES, MODES)).toEqual([])
  }, 60_000)

  it('every dim mark is clearly dimmer than the new-park colour', () => {
    expect(notDimmer(RULES, MODES)).toEqual([])
  }, 60_000)

  it(`every dim mark is a step in lightness from the new-park colour: ${LIGHTNESS_STEP} or more in OKLab L`, () => {
    expect(tooAlike(RULES, MODES)).toEqual([])
  }, 60_000)

  it('no colour-role preset sets a token this measures, so leaving them out hides nothing', () => {
    const measured = ['--bg', '--text', '--panel', '--need-pota', '--pota-dim']
    const set = PALETTE_ROLES.flatMap((r) => [...r.tokens, ...r.aliases])
    expect(set.filter((t) => measured.includes(t))).toEqual([])
  })

  it('FIRES: without the dim colour the worked-park tick is plain, in every theme', () => {
    for (const mode of MODES)
      for (const m of measure(SHIPPED, mode, SURFACES))
        m.dim.forEach((d, i) => expect(hex(d), `${m.name} ${mode}`).toBe(hex(m.plain[i])))
    expect(notTheToken(SHIPPED, MODES)).toHaveLength(MODES.length * 3)
  }, 60_000)

  it('FIRES: a light theme that falls through to the dark value is caught as not dimmer, and only there', () => {
    const found = notDimmer(DARK_ONLY, MODES)
    expect(found.length).toBeGreaterThan(0)
    expect(found.every((f) => LIGHT.some((m) => f.includes(` ${m}: `)))).toBe(true)
    expect(notDimmer(DARK_ONLY, DARK)).toEqual([])
  }, 60_000)

  it('FIRES: the light theme’s greens as they shipped, 0.04 apart in lightness, are caught on every light mark, and only there', () => {
    expect(tooAlike(LIGHT_AS_SHIPPED, MODES)).toHaveLength(LIGHT.length * 3)
    expect(tooAlike(LIGHT_AS_SHIPPED, DARK)).toEqual([])
  }, 60_000)
})

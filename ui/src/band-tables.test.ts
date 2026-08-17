import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { bandLabelForMhz, bandRangeForLabel } from './band'

// BAND TABLES — the UI's hand-written band data against the Rust band plan, which is
// the arbiter.
//
// Three tables describe the same bands and nothing compared them: `bandplan.rs`
// (`band_for_dial` + the per-mode plans), `band.ts`'s BAND_RANGES, and SettingsPanel's
// STOCK_WORKING_FREQUENCIES / FREQ_BANDS. They drifted, and 4 m is where it showed:
// the Rust FT8 plan was generated from WSJT-X's own table but dropped the 70.154 row,
// FREQ_BANDS never listed the band at all, and band.ts stopped 4 m at 70.5 while the
// backend, ADIF and WSJT-X all run it to 71.0. An operator saw a band the app half
// knew about.
//
// So nothing below hardcodes an expected frequency. Each guard parses BOTH sides and
// compares them; the Rust source is the fixture only in the sense that it is the side
// with the citation. When this goes red, check the Rust table's source note first —
// a wrong frequency here is an operator on the wrong frequency.

const repo = (rel: string) => fileURLToPath(new URL(`../../${rel}`, import.meta.url))
const read = (rel: string) => readFileSync(repo(rel), 'utf8')

const BANDPLAN_RS = read('crates/tempo-app/src/bandplan.rs')
const SETTINGS_TSX = readFileSync(
  fileURLToPath(new URL('./components/SettingsPanel.tsx', import.meta.url)),
  'utf8',
)

/** The body of a `pub fn <name>() -> ... {` up to its closing brace in column 0. */
function fnBody(src: string, name: string): string {
  const at = src.indexOf(`pub fn ${name}(`)
  expect(at, `bandplan.rs declares ${name}`).toBeGreaterThan(-1)
  const end = src.indexOf('\n}\n', at)
  return src.slice(at, end)
}

/** Every `ch("band", "group", dial, ...)` row in a Rust band-plan function, in order.
 *
 * ⚠️ `\s*` AFTER THE OPEN PAREN IS LOAD-BEARING. rustfmt splits a `ch(...)` call across lines
 * as soon as its note argument makes the line long, and without this the row silently stops
 * being seen — the Rust side then looks like it has no such band and the guard fails claiming
 * the TS table has an EXTRA row. That is a confusing way to learn you reformatted a file: it
 * happened to 60 m the moment its note grew (2026-08-05). The guard exists to compare DATA, so
 * it must not be sensitive to formatting. */
function rustPlan(name: string): { band: string; mhz: number }[] {
  return [
    ...fnBody(BANDPLAN_RS, name).matchAll(/\bch\(\s*"([^"]+)",\s*"[^"]+",\s*([\d.]+)/g),
  ].map((m) => ({ band: m[1], mhz: Number(m[2]) }))
}

/** A `const NAME = [ ... ]` / `const NAME: T = [ ... ]` array literal's text. The open
 * bracket is taken from the END of the declaration match, never the first `[` after the
 * name — an annotated decl (`: WorkingFrequency[] =`) carries one of its own. */
function declArray(src: string, name: string): string {
  const decl = new RegExp(`const ${name}\\b[^=]*=\\s*\\[`).exec(src)
  expect(decl, `SettingsPanel.tsx declares ${name}`).not.toBeNull()
  const open = decl!.index + decl![0].length - 1
  let depth = 0
  for (let i = open; i < src.length; i++) {
    if (src[i] === '[') depth++
    else if (src[i] === ']' && --depth === 0) return src.slice(open, i + 1)
  }
  throw new Error(`unbalanced [ for ${name}`)
}

const STOCK = [
  ...declArray(SETTINGS_TSX, 'STOCK_WORKING_FREQUENCIES').matchAll(
    /band:\s*'([^']+)',\s*mode:\s*'([^']+)',\s*mhz:\s*([\d.]+)/g,
  ),
].map((m) => ({ band: m[1], mode: m[2], mhz: Number(m[3]) }))

const FREQ_BANDS = [...declArray(SETTINGS_TSX, 'FREQ_BANDS').matchAll(/'([^']+)'/g)].map(
  (m) => m[1],
)

describe('the Settings stock table mirrors the Rust band plan', () => {
  it('parsed both sides', () => {
    expect(rustPlan('ft8_band_plan').length).toBeGreaterThan(10)
    expect(STOCK.length).toBeGreaterThan(10)
    expect(FREQ_BANDS.length).toBeGreaterThan(10)
  })

  // The panel calls this table "the stock WSJT-X working-frequency table" and shows it
  // read-only for reference. If it disagrees with the plan the engine actually tunes,
  // the reference lies about where the radio goes.
  for (const [fn, mode] of [
    ['ft8_band_plan', 'FT8'],
    ['ft4_band_plan', 'FT4'],
  ] as const) {
    it(`publishes exactly the ${mode} rows the engine ships, in order`, () => {
      expect(STOCK.filter((r) => r.mode === mode).map((r) => `${r.band} ${r.mhz}`)).toEqual(
        rustPlan(fn).map((r) => `${r.band} ${r.mhz}`),
      )
    })
  }

  // The override editor's band select is how an operator reaches a stock row to change
  // it. A row whose band is missing from FREQ_BANDS is a row nobody can override —
  // exactly what 4 m was.
  it('offers every band the stock table has a row on', () => {
    for (const r of STOCK) expect(FREQ_BANDS, `${r.band} ${r.mode}`).toContain(r.band)
  })
})

describe('band.ts agrees with bandplan.rs on 4 m', () => {
  // 4 m is IARU Region 1 only and its national edges vary by tens of kHz, so the app
  // must not narrow it on a guess. 70.0–71.0 is the ADIF Band Enumeration, WSJT-X's
  // `models/Bands.cpp` ({"4m", 70000000u, 71000000u}) and `band_for_dial`'s own arm —
  // three sources, one number. UK Full licensees reach 70.5+ by NoV, so the half-band
  // band.ts used to stop at was not empty air.
  it('spans the same range the backend does', () => {
    const arm = BANDPLAN_RS.match(/\(([\d.]+)\.\.([\d.]+)\)\.contains\(&f\) => "4m"/)
    expect(arm, 'band_for_dial has a 4m arm').not.toBeNull()
    expect(bandRangeForLabel('4m')).toEqual({ lo: Number(arm![1]), hi: Number(arm![2]) })
  })

  it('labels every 4 m calling frequency the plans ship', () => {
    const fourM = [
      'ft8_band_plan',
      'ft2_band_plan',
      'jt65_band_plan',
      'msk144_band_plan',
      'wspr_band_plan',
    ]
      .flatMap(rustPlan)
      .filter((r) => r.band === '4m')
    // 70.157 is FT2's — Decodium's only IARU-R1-restricted FT2 row.
    expect(fourM.map((r) => r.mhz).sort()).toEqual([70.091, 70.102, 70.154, 70.157, 70.23])
    for (const r of fourM) expect(bandLabelForMhz(r.mhz), `${r.mhz} MHz`).toBe('4m')
  })
})

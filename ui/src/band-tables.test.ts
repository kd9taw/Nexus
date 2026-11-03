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

describe('band.ts names every band band_for_dial does, with the same edges', () => {
  // The engine's `band_for_dial` reaches the ADIF band edges: 2200 m, 630 m, 60 m from 5.06
  // (the UK starts at 5.2585), 8 m and 5 m. The UI table labelled none of them and stopped 60 m
  // at 5.41, so a UK operator on their own 60 m saw a band in the engine and a blank on screen.
  // Parsed from the Rust source, never restated.
  const ARMS = [
    ...fnBody(BANDPLAN_RS, 'band_for_dial').matchAll(
      /\(([\d._]+)\.\.([\d._]+)\)\.contains\(&f\) => "([^"]+)"/g,
    ),
  ].map((m) => ({
    label: m[3],
    lo: Number(m[1].replace(/_/g, '')),
    hi: Number(m[2].replace(/_/g, '')),
  }))
  // 17 m: the engine kept its old 18.06–18.17 so no over that runs today is cut; the UI draws
  // ADIF's 18.068–18.168, which the band strip shades as the band. Inside the engine's, so every
  // dial the UI labels 17 m the engine does too.
  const NARROWER: Record<string, { lo: number; hi: number }> = {
    '17m': { lo: 18.068, hi: 18.168 },
  }

  it('parsed the Rust table', () => {
    expect(ARMS.map((a) => a.label)).toEqual(expect.arrayContaining(['2200m', '630m', '60m', '8m', '5m']))
    expect(ARMS.length).toBeGreaterThan(20)
  })

  it.each(['2200m', '630m', '60m', '8m', '5m'])('%s has the engine edges', (label) => {
    const arm = ARMS.find((a) => a.label === label)!
    expect(bandRangeForLabel(label)).toEqual({ lo: arm.lo, hi: arm.hi })
  })

  it('every arm', () => {
    for (const a of ARMS) {
      expect(bandRangeForLabel(a.label), a.label).toEqual(NARROWER[a.label] ?? { lo: a.lo, hi: a.hi })
    }
  })

  it('labels a dial inside each arm as the engine does', () => {
    for (const a of ARMS) {
      const r = NARROWER[a.label] ?? a
      for (const f of [r.lo + 1e-6, (r.lo + r.hi) / 2, r.hi - 1e-6]) {
        expect(bandLabelForMhz(f), `${f} MHz`).toBe(a.label)
      }
    }
    // The UK 60 m channel, and the old 5.41 top that used to blank 5.41–5.5.
    expect(bandLabelForMhz(5.2585)).toBe('60m')
    expect(bandLabelForMhz(5.45)).toBe('60m')
    // Controls: still no band below 60 m, between 8 m and 6 m, or between 5 m and 4 m.
    for (const f of [5.0599, 39.99, 45.5, 69.95, 0.4719, 0.1356]) {
      expect(bandLabelForMhz(f), `${f} MHz`).toBe('')
    }
  })
})

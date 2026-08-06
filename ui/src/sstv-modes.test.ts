// THE SSTV MODE TABLE — `SstvView.tsx`'s `SSTV_TX_MODES` against `modespec.rs`, which is
// the arbiter.
//
// There are two tables describing the same 15 modes and nothing compared them. The Rust
// one calls itself the "single source of truth" and is the one the transmit path actually
// obeys: `sstv_send` does `for_mode(mode)` and REFUSES anything that is not exactly
// `line_pixels × image_lines`, and `encode_image` refuses again. The TS one is a hand
// copy whose header says "Dimensions mirror modespec.rs" — with no test saying so.
//
// The rasters were all still correct when this guard was written; the drift was latent,
// not live. The `seconds` column had already gone, though: every entry was about a second
// low against the encoder's own `tx_duration_secs`. That was cosmetic while it only fed a
// picker label — it stopped being cosmetic when the composer began telling the operator
// how long the rig will be keyed down.
//
// So, `band-tables.test.ts` discipline: nothing below hardcodes a dimension or a duration.
// Both sides are parsed and compared, and the Rust source is the fixture only in the sense
// that it is the side with the citation (slowrx `modespec.c`, N7CXI 2000). A resizer keys
// off the raster, so when this goes red the crop is about to be wrong.

import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const repo = (rel: string) => readFileSync(fileURLToPath(new URL(`../../${rel}`, import.meta.url)), 'utf8')
const MODESPEC_RS = repo('crates/tempo-sstv/src/modespec.rs')
const ENCODE_RS = repo('crates/tempo-sstv/src/encode.rs')
const SSTV_VIEW_TSX = readFileSync(
  fileURLToPath(new URL('./components/SstvView.tsx', import.meta.url)),
  'utf8',
)

/** Rust numeric literals carry `_` separators (`0.000_137_5`); JS does not care but
 *  `Number()` does. */
const num = (s: string) => Number(s.replace(/_/g, ''))

interface RustSpec {
  slug: string
  w: number
  h: number
  line: number
  sync: number
  porch: number
  pixel: number
  septr: number
  layout: string
  mode: string
}

/** Every `const NAME: ModeSpec = ModeSpec { … };` block in modespec.rs. */
function rustSpecs(): RustSpec[] {
  const out: RustSpec[] = []
  for (const m of MODESPEC_RS.matchAll(/const\s+\w+\s*:\s*ModeSpec\s*=\s*ModeSpec\s*\{([\s\S]*?)\n\};/g)) {
    const body = m[1]
    const field = (name: string, re = '[0-9._]+') => {
      const f = new RegExp(`\\b${name}\\s*:\\s*(${re})`).exec(body)
      expect(f, `ModeSpec block is missing ${name}`).not.toBeNull()
      return f![1]
    }
    out.push({
      slug: /short_name\s*:\s*"([^"]+)"/.exec(body)![1],
      mode: /\bmode\s*:\s*SstvMode::(\w+)/.exec(body)![1],
      w: num(field('line_pixels')),
      h: num(field('image_lines')),
      line: num(field('line_seconds')),
      sync: num(field('sync_seconds')),
      porch: num(field('porch_seconds')),
      pixel: num(field('pixel_seconds')),
      septr: num(field('septr_seconds')),
      layout: /channel_layout\s*:\s*ChannelLayout::(\w+)/.exec(body)![1],
    })
  }
  return out
}

/** A `const NAME: f64 = value;` from encode.rs — the header timing constants. */
function encodeConst(name: string): number {
  const m = new RegExp(`const ${name}\\s*:\\s*f64\\s*=\\s*([0-9._]+)\\s*;`).exec(ENCODE_RS)
  expect(m, `encode.rs declares ${name}`).not.toBeNull()
  return num(m![1])
}

/**
 * `tx_duration_secs` for one mode.
 *
 * The DATA is parsed from the Rust source above — this restates only the STRUCTURE of
 * `encode.rs::scanline_secs`, which is the part a regex cannot carry: which channels each
 * family emits, and that PD packs two image rows into one radio line. Kept beside a
 * citation of the function it mirrors so the two are checked together.
 */
function txSeconds(s: RustSpec): number {
  const header = 2 * encodeConst('LEADER_SECS') + encodeConst('CAL_BREAK_SECS') + 10 * encodeConst('VIS_BIT_SECS')
  const w = s.w
  let content: number
  let frames: number
  if (s.layout === 'PdYcbcr') {
    // sync + porch + 4 channels (Y_odd, Cr, Cb, Y_even); two image rows per radio line.
    content = s.sync + s.porch + 4 * w * s.pixel
    frames = s.h / 2
  } else if (s.layout === 'RgbSequential') {
    // Scottie / Martin: 2 septr + sync + porch + 3 channels (G, B, R).
    content = 2 * s.septr + s.sync + s.porch + 3 * w * s.pixel
    frames = s.h
  } else if (s.mode === 'Robot72') {
    // R72: sync + porch + 2 septr + 3 channels (Y, U, V).
    content = s.sync + s.porch + 2 * s.septr + 3 * w * s.pixel
    frames = s.h
  } else {
    // R36 / R24: sync + porch + 1 septr + Y(2×) + 1 chroma = 3 pixel widths.
    content = s.sync + s.porch + s.septr + 3 * w * s.pixel
    frames = s.h
  }
  return header + frames * Math.max(content, s.line)
}

/** Every row of `SSTV_TX_MODES` in SstvView.tsx. */
function tsModes() {
  const decl = /const SSTV_TX_MODES: TxMode\[\] = \[/.exec(SSTV_VIEW_TSX)
  expect(decl, 'SstvView.tsx declares SSTV_TX_MODES').not.toBeNull()
  const start = decl!.index
  const end = SSTV_VIEW_TSX.indexOf('\n]', start)
  const body = SSTV_VIEW_TSX.slice(start, end)
  return [
    ...body.matchAll(
      /slug:\s*'([^']+)'[^}]*?width:\s*(\d+),\s*height:\s*(\d+),\s*seconds:\s*(\d+)/g,
    ),
  ].map((m) => ({ slug: m[1], w: Number(m[2]), h: Number(m[3]), seconds: Number(m[4]) }))
}

describe('the SSTV transmit-mode table mirrors modespec.rs', () => {
  const rust = rustSpecs()
  const ts = tsModes()

  it('parsed both sides', () => {
    expect(rust.length, 'modespec.rs ModeSpec blocks').toBe(15)
    expect(ts.length, 'SSTV_TX_MODES rows').toBe(15)
  })

  it('offers exactly the modes the crate implements — no more, no fewer', () => {
    expect(ts.map((m) => m.slug).sort()).toEqual(rust.map((s) => s.slug).sort())
  })

  it('⭐ every raster matches — the backend REFUSES anything else, so a drift here is a refused send', () => {
    for (const spec of rust) {
      const row = ts.find((m) => m.slug === spec.slug)
      expect(row, `no UI row for ${spec.slug}`).toBeDefined()
      expect([row!.w, row!.h], `${spec.slug} raster`).toEqual([spec.w, spec.h])
    }
  })

  it('⭐ every airtime matches the encoder — this is what the composer tells the operator to expect key-down', () => {
    for (const spec of rust) {
      const row = ts.find((m) => m.slug === spec.slug)!
      expect(row.seconds, `${spec.slug} airtime`).toBe(Math.round(txSeconds(spec)))
    }
  })

  it('the longest over is under the engine cap (engine.rs refuses past 330 s)', () => {
    for (const spec of rust) {
      expect(txSeconds(spec), `${spec.slug}`).toBeLessThan(330)
    }
  })

  it('the rasters span the five aspect ratios a crop box has to re-derive for', () => {
    // A crop rectangle correct for Scottie is wrong for Robot 36. The drag box's aspect is
    // re-derived on every mode change, not just its pixel size — this pins that there is
    // really more than one aspect to re-derive.
    const aspects = new Set(rust.map((s) => (s.w / s.h).toFixed(3)))
    expect(aspects.size).toBeGreaterThan(1)
    expect(aspects).toContain('1.250') // Scottie / Martin / PD-50 / PD-90
    expect(aspects).toContain('1.333') // Robot
  })
})

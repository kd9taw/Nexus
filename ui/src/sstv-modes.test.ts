// THE SSTV MODE TABLE — `sstvModes.ts`'s `SSTV_TX_MODES` against `modespec.rs`, which is
// the arbiter. (The table lived in `SstvView.tsx` until Settings grew a default-mode picker
// that needs the same 15 rows; it moved to a pure module rather than being copied.)
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
const ENGINE_RS = repo('crates/tempo-app/src/engine.rs')
const FSK_RS = repo('crates/tempo-sstv/src/fsk.rs')

/** The engine's hard ceiling on one SSTV over, read from `engine.rs` rather than
 *  restated. `sstv_send` refuses anything longer, so a mode past it must never reach
 *  the picker — the operator would choose it, wait for the encode, and be told no. */
const MAX_TX_SECS = (() => {
  const m = /const SSTV_MAX_TX_SECS\s*:\s*f64\s*=\s*([0-9._]+)\s*;/.exec(ENGINE_RS)
  expect(m, 'engine.rs declares SSTV_MAX_TX_SECS').not.toBeNull()
  return Number(m![1].replace(/_/g, ''))
})()
const SSTV_MODES_TS = readFileSync(
  fileURLToPath(new URL('./sstvModes.ts', import.meta.url)),
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
  } else if (s.layout === 'RgbSequential' || s.layout === 'SequentialRgb') {
    // Scottie / Martin (G,B,R) and Wraase SC-2 / Pasokon (R,G,B):
    // 2 septr + sync + porch + 3 channels.
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

/** Every row of a `TxMode[]` table in sstvModes.ts, by declaration name. */
function tsRows(name: string) {
  const decl = new RegExp(`const ${name}: TxMode\\[\\] = \\[`).exec(SSTV_MODES_TS)
  expect(decl, `sstvModes.ts declares ${name}`).not.toBeNull()
  const start = decl!.index
  const end = SSTV_MODES_TS.indexOf('\n]', start)
  const body = SSTV_MODES_TS.slice(start, end)
  return [
    ...body.matchAll(
      /slug:\s*'([^']+)'[^}]*?width:\s*(\d+),\s*height:\s*(\d+),\s*seconds:\s*(\d+)/g,
    ),
  ].map((m) => ({ slug: m[1], w: Number(m[2]), h: Number(m[3]), seconds: Number(m[4]) }))
}

/** Every row of `SSTV_TX_MODES` — what the composer offers to SEND. */
const tsModes = () => tsRows('SSTV_TX_MODES')

describe('the SSTV transmit-mode table mirrors modespec.rs', () => {
  const rust = rustSpecs()
  const ts = tsModes()

  it('parsed both sides', () => {
    expect(rust.length, 'modespec.rs ModeSpec blocks').toBe(18)
    expect(ts.length, 'SSTV_TX_MODES rows').toBe(17)
  })

  it('⭐ offers exactly the modes the crate implements AND the engine will key', () => {
    // Was "no more, no fewer" against the whole crate table, until #264 added a mode
    // the crate DECODES and the engine refuses to TRANSMIT (Pasokon P7, 407 s against
    // engine.rs's 330 s cap). The rule is now derived from that cap on both sides, so
    // neither a new mode nor a changed cap can leave the picker quietly wrong: a mode
    // under the cap that is missing here is a mode the operator cannot send, and a
    // mode over it that is present is a send the backend will refuse after the wait.
    const sendable = rust.filter((s) => txSeconds(s) < MAX_TX_SECS).map((s) => s.slug)
    expect(ts.map((m) => m.slug).sort()).toEqual(sendable.sort())
    expect(sendable, 'the cap must actually exclude something, or this test is vacuous')
      .not.toEqual(rust.map((s) => s.slug))
  })

  it('⭐ the RECEIVE list is exactly the crate\'s mode table — a manual start (#202) can name any of them', () => {
    // The transmit list is allowed to be smaller (the cap, above). The receive list is
    // not allowed to be smaller by ANY amount: everything the decoder understands can be
    // started by hand, and a mode missing from both lists would be one Nexus decodes and
    // never lets the operator ask for.
    const rx = [...tsRows('SSTV_TX_MODES'), ...tsRows('SSTV_RX_ONLY_MODES')]
    expect(rx.map((m) => m.slug).sort()).toEqual(rust.map((s) => s.slug).sort())
    for (const row of rx) {
      const spec = rust.find((s) => s.slug === row.slug)!
      expect([row.w, row.h], `${row.slug} raster`).toEqual([spec.w, spec.h])
      expect(row.seconds, `${row.slug} airtime`).toBe(Math.round(txSeconds(spec)))
    }
  })

  it('every mode the crate decodes is either offered for transmit or over the cap', () => {
    // The other direction, stated plainly: nothing may fall out of both lists by
    // accident. A mode absent from the picker has to be absent BECAUSE of its length.
    for (const spec of rust) {
      const offered = ts.some((m) => m.slug === spec.slug)
      if (!offered) {
        expect(txSeconds(spec), `${spec.slug} is not offered, so it must be over the cap`)
          .toBeGreaterThanOrEqual(MAX_TX_SECS)
      }
    }
  })

  // Both sweeps walk the OFFERED rows, not the whole crate table: a mode the picker
  // deliberately omits (over the engine cap — see above) has no row to compare, and
  // the pair of membership tests above is what proves the omission is deliberate.
  it('⭐ every raster matches — the backend REFUSES anything else, so a drift here is a refused send', () => {
    for (const row of ts) {
      const spec = rust.find((s) => s.slug === row.slug)
      expect(spec, `${row.slug} is offered but modespec.rs has no such mode`).toBeDefined()
      expect([row.w, row.h], `${row.slug} raster`).toEqual([spec!.w, spec!.h])
    }
  })

  it('⭐ every airtime matches the encoder — this is what the composer tells the operator to expect key-down', () => {
    for (const row of ts) {
      const spec = rust.find((s) => s.slug === row.slug)!
      expect(row.seconds, `${row.slug} airtime`).toBe(Math.round(txSeconds(spec)))
    }
  })

  it('the longest OFFERED over is under the engine cap (engine.rs refuses past it)', () => {
    for (const row of ts) {
      const spec = rust.find((s) => s.slug === row.slug)!
      expect(txSeconds(spec), `${spec.slug}`).toBeLessThan(MAX_TX_SECS)
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


// The FSK callsign burst's airtime is quoted to the operator in Settings and added to
// the composer's key-down clock, so its two constants are a mirror like the mode table
// — parsed from `fsk.rs`, never restated.
describe('the FSK callsign burst mirrors fsk.rs', () => {
  const rustConst = (name: string, re = '[0-9._]+') => {
    const m = new RegExp(`const ${name}\\s*:\\s*\\w+\\s*=\\s*(${re})\\s*;`).exec(FSK_RS)
    expect(m, `fsk.rs declares ${name}`).not.toBeNull()
    return Number(m![1].replace(/_/g, ''))
  }
  const tsConst = (name: string) => {
    const m = new RegExp(`export const ${name} = ([0-9.]+)`).exec(SSTV_MODES_TS)
    expect(m, `sstvModes.ts exports ${name}`).not.toBeNull()
    return Number(m![1])
  }

  it('baud and bits per character are the ones on the wire', () => {
    expect(tsConst('FSK_ID_BAUD')).toBe(rustConst('BAUD'))
    expect(tsConst('FSK_ID_BITS_PER_CHAR')).toBe(rustConst('BITS_PER_CHAR'))
  })

  it('a six-character callsign costs about a second and a quarter', () => {
    // 2 leader bytes + 6 characters + 1 end marker, six bits each at 45.45 baud.
    const secs = (9 * rustConst('BITS_PER_CHAR')) / rustConst('BAUD')
    expect(secs).toBeGreaterThan(1)
    expect(secs).toBeLessThan(1.5)
  })
})

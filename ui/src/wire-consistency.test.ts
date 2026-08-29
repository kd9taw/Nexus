// Cross-language wire-format guard: the TypeScript string unions that the UI COMPARES
// AGAINST must match the strings Rust actually SERIALIZES.
//
// This exists because of a real bug on 2026-07-21. Renaming FT1 -> TempoFast changed
// `#[serde(rename = "FT1")]` to `"TempoFast"` in dto.rs, but ui/src/types.ts still declared
// `type Tier = 'FT1' | 'DX1' | 'FT8' | 'FT4'`. Both sides compiled clean — they were just
// string literals that happened to disagree — and every comparison silently evaluated false:
//
//   App.tsx  `if (tier === 'FT1')`        -> Work routing sent Tempo contacts to FT8
//   App.tsx  `s.tier === 'FT1'`           -> the Tempo chat roster rendered empty
//   decodeHistory.ts                      -> per-tier history depths fell back to defaults
//
// Nothing failed loudly. TypeScript cannot catch it, because neither side is wrong on its
// own — only the pair is. So the pair gets a test.
//
// Reads dto.rs the same way cockpit-floors.test.ts reads styles.css.
//
// The same reasoning covers a CONSTANT declared once in each language: neither side is wrong
// on its own, only the pair is, so the pair gets a test. See the zero-beat block at the
// bottom — that mirror is a plain number, which is if anything easier to drift than a string,
// because a number carries no clue about what it is supposed to equal.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const dto = readFileSync(
  fileURLToPath(new URL('../../crates/tempo-app/src/dto.rs', import.meta.url)),
  'utf8',
)
const types = readFileSync(fileURLToPath(new URL('./types.ts', import.meta.url)), 'utf8')

/** The `#[serde(rename = "...")]` values of one Rust enum, in declaration order. */
function rustWireValues(enumName: string): string[] {
  const m = dto.match(new RegExp(`pub enum ${enumName}\\s*\\{([\\s\\S]*?)\\n\\}`))
  if (!m) throw new Error(`enum ${enumName} not found in dto.rs`)
  return [...m[1].matchAll(/#\[serde\(rename\s*=\s*"([^"]+)"\)\]/g)].map((x) => x[1])
}

/** The members of a TS string-literal union `export type X = 'a' | 'b'`. */
function tsUnion(typeName: string): string[] {
  const m = types.match(new RegExp(`export type ${typeName} =([^\\n]+)`))
  if (!m) throw new Error(`type ${typeName} not found in types.ts`)
  return [...m[1].matchAll(/'([^']+)'/g)].map((x) => x[1])
}

describe('Rust <-> TypeScript wire consistency', () => {
  it('Tier: every value Rust serializes is a value the UI can compare against', () => {
    const rust = rustWireValues('Tier')
    const ts = tsUnion('Tier')
    // Sorted: declaration order is not part of the contract, membership is.
    expect([...ts].sort()).toEqual([...rust].sort())
  })

  it('Tier still carries the renamed Tempo protocols, not the retired FT1/DX1 names', () => {
    const rust = rustWireValues('Tier')
    expect(rust).toContain('TempoFast')
    expect(rust).toContain('TempoDeep')
    // The specific regression: if either old name comes back on the wire without the UI
    // following, Work routing breaks silently again.
    expect(rust).not.toContain('FT1')
    expect(rust).not.toContain('DX1')
    expect(tsUnion('Tier')).not.toContain('FT1')
  })

  it('no UI source compares a tier against a retired protocol name', () => {
    // Belt and braces: the type check above only guards types.ts. A stray literal in a
    // comparison elsewhere is exactly what broke Work routing.
    const appSrc = readFileSync(fileURLToPath(new URL('./App.tsx', import.meta.url)), 'utf8')
    expect(appSrc).not.toMatch(/tier\s*===\s*'(FT1|DX1)'/)
    expect(appSrc).not.toMatch(/\.tier\s*===\s*'(FT1|DX1)'/)
  })
})

// ---------------------------------------------------------------------------------------
// MIRRORED CONSTANTS — one number, declared once per language.
// ---------------------------------------------------------------------------------------
//
// A cross-language mirror in this tree is not a hypothetical risk. `RadioProfilePatch` exists
// in both Rust and TypeScript and has drifted FIVE separate times (pttSerialPort, the three
// Flex fields, omnirigSlot, icomDataMode, ampModel/ampPort), and every one of those shipped
// silently, because a comment naming the other side is documentation, not a mechanism.
//
// So each mirror below is READ OUT OF BOTH SOURCES and compared. The failure message names
// both values and both files, because "these disagree" without the numbers sends the reader
// to grep for what was already known at the moment of failure.

const rxdsp = readFileSync(
  fileURLToPath(new URL('../../crates/tempo-audio/src/rxdsp.rs', import.meta.url)),
  'utf8',
)
const waterfall = readFileSync(fileURLToPath(new URL('./waterfall.ts', import.meta.url)), 'utf8')

/** The value of a bare Rust `const NAME: <ty> = <number>;`. Throws if it is gone or renamed —
 *  a mirror guard that quietly stops finding one half is worse than no guard. */
function rustConst(src: string, where: string, name: string): number {
  const m = src.match(new RegExp(`const ${name}\\s*:\\s*[A-Za-z0-9_]+\\s*=\\s*([0-9_.]+)`))
  if (!m) throw new Error(`const ${name} not found in ${where} — renamed, or moved out of it`)
  return Number(m[1].replace(/_/g, ''))
}

/** The value of a TS `export const NAME = <number>`. Throws for the same reason. */
function tsConst(src: string, where: string, name: string): number {
  const m = src.match(new RegExp(`export const ${name}\\s*=\\s*([0-9_.]+)`))
  if (!m) throw new Error(`export const ${name} not found in ${where} — renamed, or moved out of it`)
  return Number(m[1].replace(/_/g, ''))
}

describe('Rust <-> TypeScript mirrored constants', () => {
  it('the CW zero-beat search range and the needle range are the same number', () => {
    // Rust searches +/- this far from the operator's pitch for a tone; the UI draws the needle
    // over +/- this far. They are the SAME quantity seen from two sides, so:
    //   TS wider than Rust  -> needle travel the backend can never reach (dead ends of scale)
    //   TS narrower         -> readings the backend genuinely produces pin at the edge, and
    //                          being 250 Hz off looks identical to being 400 Hz off, which is
    //                          the exact complaint the indicator was built to answer.
    const rust = rustConst(rxdsp, 'crates/tempo-audio/src/rxdsp.rs', 'ZERO_BEAT_SEARCH_HZ')
    const ts = tsConst(waterfall, 'ui/src/waterfall.ts', 'ZERO_BEAT_RANGE_HZ')
    expect(
      ts,
      `zero-beat range drifted: rxdsp.rs ZERO_BEAT_SEARCH_HZ = ${rust}, ` +
        `waterfall.ts ZERO_BEAT_RANGE_HZ = ${ts}. They are one quantity; change both.`,
    ).toBe(rust)
    // Sanity on the parse itself: a regex that silently matched nothing would make the
    // comparison above pass on two NaNs. Both must be real, positive numbers.
    expect(Number.isFinite(rust) && rust > 0).toBe(true)
    expect(Number.isFinite(ts) && ts > 0).toBe(true)
  })
})

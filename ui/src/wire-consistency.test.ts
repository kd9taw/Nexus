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

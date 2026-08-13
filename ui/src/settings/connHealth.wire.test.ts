// The connector `id` slugs cross the wire, and NOTHING else guards them.
//
// `wire-consistency.test.ts` reads `crates/tempo-app/src/dto.rs`, but `CredStatus` is
// declared in `src-tauri/src/lib.rs` — outside everything that test can see. So the pair
// gets its own guard, in the same source-reading style.
//
// Why it matters: the UI branches on `c.id === 'qrz-logbook'` to render the only credential
// test button in the Connections grid, and connHealth.ts keys nothing else off the label.
// A slug renamed on the Rust side compiles clean on both, and the button silently vanishes
// — the exact failure the old `connector === 'QRZ Logbook'` prose sentinel had, which this
// id was introduced to remove. Neither side is wrong alone; only the pair is.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const lib = readFileSync(
  fileURLToPath(new URL('../../../src-tauri/src/lib.rs', import.meta.url)),
  'utf8',
)

/** Every `id: "…".into()` literal inside `get_credentials_status`, in row order. */
function rustIds(): string[] {
  const m = lib.match(/fn get_credentials_status[\s\S]*?\n\}/)
  if (!m) throw new Error('get_credentials_status not found in lib.rs')
  return [...m[0].matchAll(/\bid:\s*"([^"]+)"\.into\(\)/g)].map((x) => x[1])
}

/** The slugs the TS side documents as the closed set (the `id` jsdoc in types.ts). */
function tsIds(): string[] {
  const types = readFileSync(fileURLToPath(new URL('../types.ts', import.meta.url)), 'utf8')
  const m = types.match(/Stable slug:([\s\S]*?)\*\//)
  if (!m) throw new Error("the CredStatus `id` jsdoc's slug list not found in types.ts")
  return [...m[1].matchAll(/'([^']+)'/g)].map((x) => x[1])
}

describe('the connector id slugs agree across the wire', () => {
  it('finds the real rows — the positive control', () => {
    // Without this, a regex that matched nothing would make every assertion below vacuous:
    // two empty arrays are equal, and the guard would pass forever while drifting freely.
    const ids = rustIds()
    expect(ids.length).toBe(8)
    expect(ids).toContain('qrz-logbook')
  })

  it('declares exactly the same set on both sides', () => {
    expect([...rustIds()].sort()).toEqual([...tsIds()].sort())
  })

  it('still ships the id the QRZ Logbook test button is keyed on', () => {
    // The one id the UI actually branches on. Renaming it must fail here, not in the field.
    const panel = readFileSync(
      fileURLToPath(new URL('../components/SettingsPanel.tsx', import.meta.url)),
      'utf8',
    )
    expect(panel).toMatch(/c\.id === 'qrz-logbook'/)
    expect(rustIds()).toContain('qrz-logbook')
  })
})

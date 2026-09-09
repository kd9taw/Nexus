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

/** The ids `CONN_HEALTH_IDS` in lib.rs allows a persisted health row to carry. */
function rustHealthIds(): string[] {
  const m = lib.match(/const CONN_HEALTH_IDS: &\[&str\] = &\[([\s\S]*?)\];/)
  if (!m) throw new Error('CONN_HEALTH_IDS not found in lib.rs')
  return [...m[1].matchAll(/"([^"]+)"/g)].map((x) => x[1])
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
    expect(ids.length).toBe(10)
    expect(ids).toContain('qrz-logbook')
  })

  it('declares exactly the same set on both sides', () => {
    expect([...rustIds()].sort()).toEqual([...tsIds()].sort())
  })

  it('lets every row it emits survive a restart', () => {
    // ⚠️ A THIRD list, and the only guard on it. `conn_health_from_json` drops any row whose
    // id is not in `CONN_HEALTH_IDS`, so a connector that stamps health under an id missing
    // from that list writes the file, reloads, and finds its row gone — silently, visible
    // only as the panel going amber again after a restart, which is #245's exact symptom.
    // `winlink` was in that state, ahead of its own health stamps.
    //
    // Asserted as a superset rather than as equality: the list may legitimately hold an id
    // for a row not yet emitted (that is what makes a future stamp safe), and may not
    // legitimately be missing one that is.
    const health = rustHealthIds()
    // The control first: without it, a regex that matched nothing would make the assertion
    // below vacuous — every id is trivially contained in a set nobody read. Named rather
    // than counted, so adding a connector does not have to touch a number here and the
    // failure below is the one that names what is missing.
    expect(health).toContain('cloudlog')
    for (const id of rustIds()) {
      expect(health, `${id} stamps health that no restart would keep`).toContain(id)
    }
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

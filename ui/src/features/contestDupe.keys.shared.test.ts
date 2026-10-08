// The dupe key the log strip builds for the contact being typed is the key the ENGINE builds
// for the row it is about to write (`DupeRule::key_of`, crates/tempo-core/src/contest/dupe.rs).
// `__fixtures__/contest-dupe-keys.json` is ONE table of contacts and their keys: the engine's
// key is checked against it in Rust (crates/tempo-app/src/fdevent.rs) and `dupeKey` here, so a
// change to either side's key is red until both agree. A while-typing warning built from a key
// the engine does not build is a warning about a different contact.
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { dupeKey } from './contestDupe'
import type { DupeRule } from '../types'

interface Row {
  ruleset: string
  call: string
  band: string
  mode: string
  rx: Record<string, string>
  tx: Record<string, string>
  key: string[]
  note?: string
}
const TABLE = JSON.parse(
  readFileSync(new URL('./__fixtures__/contest-dupe-keys.json', import.meta.url), 'utf8'),
) as { rules: Record<string, DupeRule>; rows: Row[] }

describe('the strip builds the key the engine builds', () => {
  it('reads a table that is there — a parser that found nothing would pass the loop below', () => {
    expect(TABLE.rows.length).toBeGreaterThanOrEqual(9)
    expect(Object.keys(TABLE.rules).sort()).toEqual(['arrlfd', 'arrlvhf_jun', 'ilqp', 'nyqp', 'ohqp'])
  })

  for (const row of TABLE.rows) {
    it(`${row.ruleset} ${row.call.trim()} ${row.band} ${row.mode}${row.note ? ` (${row.note})` : ''}`, () => {
      const rule = TABLE.rules[row.ruleset]
      expect(rule, `no rule for ${row.ruleset}`).toBeDefined()
      const key = dupeKey(rule, row.call, row.band, row.mode, {
        rx: (slot) => row.rx[slot] ?? '',
        tx: (slot) => row.tx[slot] ?? '',
      })
      expect(key).toEqual(row.key)
    })
  }
})

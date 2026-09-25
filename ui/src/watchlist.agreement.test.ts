// ONE MATCHER, TWO LANGUAGES (operator, 2026-09-24: "watched counts as needed").
//
// The desktop marks a watched station with the WATCH tile (`watchedEntry`, here) on the Call
// Roster, the Stations list and Spots; the STATION puts the same station on the Needed board
// (`watched`, crates/tempo-app/src/watchlist.rs) — for this window and for every Remote browser
// it serves. An entry that names a station on the roster must name it on the board, so both are
// held to one table of cases that lives with the Rust crate
// (crates/tempo-app/tests/fixtures/watch-matches.json) and is read from there — not copied —
// by this test and by `the_watch_matcher_agrees_with_the_desktop_on_every_case_in_the_shared_table`.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { watchedEntry, type WatchKind } from './watchlist'

interface Case {
  entry: { kind: WatchKind; value: string }
  subject: { call: string; entity?: string; grid?: string }
  names: boolean
}

const table = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../../crates/tempo-app/tests/fixtures/watch-matches.json'),
    'utf8',
  ),
) as { cases: Case[] }

describe('the WATCH tile and the Needed board answer every case alike', () => {
  it('the shared table is there', () => {
    expect(table.cases.length).toBeGreaterThanOrEqual(20)
    // Both answers occur, for every kind — a table of one verdict could not tell a matcher apart
    // from a constant.
    for (const kind of ['call', 'dxcc', 'grid'] as const)
      for (const names of [true, false])
        expect(table.cases.some((c) => c.entry.kind === kind && c.names === names), `${kind} ${names}`).toBe(true)
  })

  it('the desktop agrees with the table on every case', () => {
    const wrong = table.cases
      .filter((c) => (watchedEntry(c.subject, [{ id: 'case', ...c.entry }]) !== null) !== c.names)
      .map((c) => `${c.entry.kind} ${JSON.stringify(c.entry.value)} on ${JSON.stringify(c.subject)}`)
    expect(wrong).toEqual([])
  })
})

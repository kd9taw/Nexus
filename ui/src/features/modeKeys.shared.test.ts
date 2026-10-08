// "NEW MODE" COUNTS EACH MODE SEPARATELY, AND EVERY WINDOW SAYS SO (operator ruling 2026-10-07):
// FT8, FT4, RTTY, PSK31 and the rest are their own modes on the callsign card, the Call Roster,
// Band Activity, the Needed board and the alerts. The DXCC award totals still count CW / Phone /
// Digital.
//
// The report behind it: C5R (The Gambia) heard calling on 20 m FT8, against a log holding The
// Gambia on 20 m SSB and 40 m FT4. The card said "New mode-slot", because it compares exact modes;
// the roster said nothing, because the engine and the roster's gate compared mode CLASSES and FT4
// had already "worked Digital". Two answers to one question, on one screen.
//
// `__fixtures__/mode-keys.json` is ONE table of what a contact's mode is for that need. This file
// holds the UI to it — `modeKey`, the card's verdict, the roster's gate and Band Activity's — and
// two Rust suites hold the engine to the same rows: the needs engine
// (crates/propagation/tests/needs_each_mode.rs) and the ADIF reader with the card's engine answer
// (crates/tempo-core, logbook::query::tests). Change a row and all of them must agree again.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { bandKey, entitySlots, modeKey } from './callHistory'
import { resolveDecodeNeeds } from './decodeNeeds'
import { tagsForSurface } from './needs'
import type { DecodeRow, NeedAlert, NeedTag } from '../types'

interface Fold {
  mode: string
  submode: string | null
  stored: string
  key: string
}
interface Scenario {
  name: string
  entity: string
  log: { call: string; band: string; mode: string; confirmed: boolean }[]
  heard: { call: string; band: string; mode: string }
  /** The needs engine's row for the heard station, field by field, or null for no row. */
  alert: { tags: NeedTag[]; mode: string; exactMode: string; headline: string } | null
  cardNewMode: boolean
}
const TABLE = JSON.parse(readFileSync(new URL('./__fixtures__/mode-keys.json', import.meta.url), 'utf8')) as {
  folds: Fold[]
  spellings: { stored: string; key: string }[]
  scenarios: Scenario[]
}

/** The callsign card's "New mode-slot", computed the way all three cards compute it (Operate's
 * card, the log strip's, Remote's): the entity worked, this band not a new slot, and the mode in
 * front of the operator never worked. */
function cardSaysNewMode(s: Scenario): boolean {
  const slots = entitySlots(
    s.log.map((q) => ({ entity: s.entity, band: q.band, mode: q.mode })),
    s.entity,
  )
  const liveBand = bandKey({ band: s.heard.band })
  const newBandSlot =
    slots.workedEver && !slots.bandUnknown && liveBand !== null && !slots.bandsWorked.includes(liveBand)
  return slots.workedEver && !newBandSlot && !slots.modesWorked.includes(modeKey(s.heard.mode))
}

/** The engine's row as the window receives it: the table's fields on the heard station. */
function engineRow(s: Scenario): NeedAlert | null {
  if (!s.alert) return null
  return {
    call: s.heard.call,
    entity: s.entity,
    band: s.heard.band,
    zone: 35,
    priority: 30,
    freqMhz: null,
    ...s.alert,
  }
}

/** The heard station's decode, as Band Activity lists it. */
const decodeOf = (s: Scenario): DecodeRow =>
  ({
    from: s.heard.call,
    snr: -12,
    dtSec: 0.1,
    freqHz: 1200,
    message: `CQ ${s.heard.call} IK13`,
    isCq: true,
    directedToMe: false,
    worked: false,
    tier: s.heard.mode,
    rv: 0,
  }) as DecodeRow

describe('a contact keys as the shared table says', () => {
  it.each(TABLE.folds)('$mode + $submode, stored as $stored, is $key', (f) => {
    expect(modeKey(f.stored)).toBe(f.key)
    expect(modeKey(f.key), 'a key folds to itself').toBe(f.key)
  })

  it.each(TABLE.spellings)('$stored is $key', (sp) => {
    expect(modeKey(sp.stored)).toBe(sp.key)
  })
})

describe('the card, the roster and Band Activity give the engine’s answer', () => {
  it.each(TABLE.scenarios)('$name', (s) => {
    const row = engineRow(s)
    expect(cardSaysNewMode(s), 'the callsign card').toBe(s.cardNewMode)
    // The roster's chips and Band Activity's icons, on a surface showing the heard band and mode.
    const roster = row ? tagsForSurface(row, s.heard.band, s.heard.mode) : []
    expect(roster.includes('NewMode'), 'the roster').toBe(s.cardNewMode)
    const activity = resolveDecodeNeeds(decodeOf(s), s.heard.band, row ? [row] : [], s.heard.mode)
    expect(activity.cats.includes('mode'), 'Band Activity').toBe(s.cardNewMode)
  })
})

describe('a "new mode" chip shows only where the surface is that mode', () => {
  const rows = TABLE.scenarios.map(engineRow).filter((r): r is NeedAlert => r?.tags.includes('NewMode') ?? false)

  it('the table has mode needs to check (positive control)', () => {
    expect(rows.length).toBeGreaterThan(0)
  })

  it.each(rows)('$headline', (row) => {
    // Every mode the table names, as a surface: the chip is there exactly where the key is the need's.
    for (const surface of TABLE.folds.map((f) => f.stored)) {
      const shown = tagsForSurface(row, row.band, surface).includes('NewMode')
      expect(shown, `on a ${surface} surface`).toBe(modeKey(surface) === modeKey(row.exactMode))
    }
  })
})

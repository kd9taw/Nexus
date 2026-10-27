// @vitest-environment jsdom
//
// THE NEW MODE CHIP LISTS ONLY MODES THE LOG DOES NOT HOLD, IN EVERY WINDOW THAT SHOWS A NEED. Report on
// the 1.17.1 test builds: "seeing a ton of ft8 neededs when "mode" is turned on, but I have already
// gotten those countries on the ft8 mode. Countries like usa and canada where I have hundreds of
// contacts already".
//
// The rows are the engine's own. features/__fixtures__/needed-new-mode-board.json is the Needed board the
// needs engine ranks for that log (hundreds of FT8 contacts with the United States and Canada, Germany on
// CW alone, Japan in FT8) against the stations PSK Reporter's receivers near EN52 heard on 20 m, in every
// mode one capture named; crates/propagation/tests/needs_each_mode.rs holds the file to that board. The
// engine judged every name PSK Reporter passes on as a mode of its own, so each WSPR beacon, VarAC
// and FreeDV station of the two countries was a "new mode" no contact could clear, and the radio's own
// WSPR decodes were too. JTTY is a mode contacts are logged in (MODE=MFSK SUBMODE=JTTY), so a station
// heard in it is a real new mode until one is worked, as a JS8 station is.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { NeededPanel } from './components/NeededPanel'
import { OperateDecodes } from './components/OperateDecodes'
import { OperateRoster } from './components/OperateRoster'
import { alertsByCall, boardNeeds, topNeedByCall, visibleNeeds } from './features/needs'
import type { DecodeRow, NeedAlert, Station, Tier } from './types'

vi.mock('./api', () => ({ getDeclination: vi.fn(() => Promise.resolve(0)), openQrzPage: vi.fn() }))

const BOARD = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), 'features/__fixtures__/needed-new-mode-board.json'),
    'utf8',
  ),
) as NeedAlert[]

afterEach(() => {
  cleanup()
  localStorage.clear()
})

/** The call-keyed alerts as App and the Needed window derive them for the roster, Band Activity, the
 *  band strip, the band map and the map (CW and Phone on). */
const byCall = () => alertsByCall(visibleNeeds(BOARD, { cw: true, phone: true }))

/** The Needed board with its New mode chip on — what the docked view, the Needed window and the
 *  dashboard's Needed box show (each hands this panel `boardNeeds` of the engine's rows), as each row's
 *  label and the reason it gives. */
function newModeBoard(): string[] {
  render(
    <NeededPanel alerts={boardNeeds(BOARD)} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />,
  )
  fireEvent.click(document.querySelector('.np-filter-toggle') as HTMLElement)
  const group = document.querySelector('.np-filters .np-filter-group') as HTMLElement
  const chip = [...group.querySelectorAll('button')].find((b) => b.textContent === 'New mode')
  expect(chip, 'no New mode chip').toBeTruthy()
  fireEvent.click(chip!)
  return ([...document.querySelectorAll('.np-row:not(.np-header)')] as HTMLElement[]).map(
    (r) => `${r.getAttribute('aria-label')} | ${r.querySelector('.np-why')?.firstChild?.textContent}`,
  )
}

/** A station decoded on `tier`, as the roster and Band Activity hold it. */
const station = (call: string): Station => ({
  call,
  grid: null,
  snr: -10,
  lastHeardSlot: 100,
  heardCount: 1,
  presence: 'heard' as Station['presence'],
  worked: false,
}) as Station

const decode = (call: string, tier: Tier): DecodeRow => ({
  from: call,
  snr: -10,
  dtSec: 0.1,
  freqHz: 1200,
  message: `CQ ${call}`,
  isCq: true,
  directedToMe: false,
  worked: false,
  tier,
  rv: 0,
})

/** The calls the 20 m roster on `tier` puts a MODE chip on. */
function rosterModeChips(calls: string[], tier: Tier): string[] {
  const { container } = render(
    <OperateRoster
      stations={calls.map(station)}
      myGrid="EN52"
      currentSlot={100}
      needByCall={topNeedByCall(byCall())}
      needAlertsByCall={byCall()}
      band="20m"
      feedMode={tier}
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
  const rows = [...container.querySelectorAll('.or-row:not(.or-header)')] as HTMLElement[]
  expect(rows.length, 'every station is listed').toBe(calls.length)
  return calls.filter((c) => {
    const row = rows.find((r) => r.textContent?.includes(c))
    return [...(row?.querySelectorAll('*') ?? [])].some((el) => el.childElementCount === 0 && el.textContent === 'MODE')
  })
}

/** The calls 20 m Band Activity on `tier` puts a MODE chip on. */
function bandActivityModeChips(calls: string[], tier: Tier): string[] {
  const { container } = render(
    <OperateDecodes
      decodes={calls.map((c) => decode(c, tier))}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier={tier}
      harqRescues={0}
      onCall={() => {}}
      needAlertsByCall={byCall()}
    />,
  )
  const rows = [...container.querySelectorAll('.decode-row')] as HTMLElement[]
  expect(rows.length, 'every decode is listed').toBe(calls.length)
  return calls.filter((c) =>
    rows.some((r) => r.textContent?.includes(c) && r.querySelector('.need-chip.need-mode') != null),
  )
}

describe('the New mode need, on the report’s log and the stations heard near EN52', () => {
  it('the Needed board with New mode on lists Germany in FT8 and the United States in JS8 and JTTY, nothing the log holds', () => {
    expect(newModeBoard()).toEqual([
      'DL1ABC, Fed. Rep. of Germany, 20m FT8, needed NewMode | New mode — FT8 Fed. Rep. of Germany (any band)',
      'K1JSE, United States, 20m Digital, needed NewMode | New mode — JS8 United States (any band)',
      'K4JTY, United States, 20m JTTY, needed NewMode | New mode — JTTY United States (any band)',
    ])
  })

  it('the 20 m FT8 roster and Band Activity mark Germany, worked on CW alone, and no station worked in FT8', () => {
    // K1JSE is the control: a JS8 need is not one an FT8 surface can close.
    const heard = ['K1ABC', 'VE3ABC', 'JA1ABC', 'DL1ABC', 'K1JSE']
    expect(rosterModeChips(heard, 'FT8')).toEqual(['DL1ABC'])
    cleanup()
    expect(bandActivityModeChips(heard, 'FT8')).toEqual(['DL1ABC'])
  })

  it('the radio’s own WSPR decodes carry no MODE chip, on the roster or in Band Activity', () => {
    const beacons = ['W9WSP', 'VE3WSP', 'W8WSP']
    expect(rosterModeChips(beacons, 'WSPR')).toEqual([])
    cleanup()
    expect(bandActivityModeChips(beacons, 'WSPR')).toEqual([])
  })

  it('the band strip, the band map and the map colour no station of the United States or Canada as a new mode it has', () => {
    // Each colours a station by its strongest need, keyed by call alone (App's `needByCall`).
    expect([...topNeedByCall(byCall())].sort()).toEqual([
      ['DL1ABC', 'NewMode'],
      ['K1JSE', 'NewMode'],
      ['K4JTY', 'NewMode'],
    ])
  })
})

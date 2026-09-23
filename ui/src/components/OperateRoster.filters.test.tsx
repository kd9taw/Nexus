// @vitest-environment jsdom
//
// The Call Roster's filter checkboxes come back from storage on mount, and ticking one
// writes it. The operator's complaint was that "Needed only" was unticked again after every
// restart, so the load-bearing assertion is the FIRST render of a fresh component: both the
// checkbox and the rows it governs must reflect what was stored.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateRoster } from './OperateRoster'
import { ROSTER_FILTER_KEY, loadRosterFilters } from '../operateFilters'
import type { NeedAlert, NeedTag, Station } from '../types'
import { boardNeeds, isActivityTag, NEED_TIER } from '../features/needs'
import { DEFAULT_FILTERS, filterAlerts } from '../neededFilters'

vi.mock('../api', () => ({
  getDeclination: vi.fn(() => Promise.resolve(0)),
  openQrzPage: vi.fn(),
}))

const SLOT = 100

function station(call: string, over: Partial<Station> = {}): Station {
  return {
    call,
    grid: 'EN52',
    snr: -10,
    lastHeardSlot: SLOT,
    heardCount: 1,
    presence: 'heard' as Station['presence'],
    worked: false,
    ...over,
  }
}

// PLAIN is neither needed nor worked, NEEDED carries a need tag, WORKED is worked with no
// need — one station per filter outcome, so each checkbox has something to remove.
const STATIONS = [station('PLAIN1'), station('NEEDED1'), station('WORKED1', { worked: true })]
const NEEDS = new Map<string, NeedTag>([['NEEDED1', 'NewEntity']])

function mount() {
  return render(
    <OperateRoster
      stations={STATIONS}
      myGrid="EN52"
      currentSlot={SLOT}
      needByCall={NEEDS}
      selectedCall={null}
      onSelect={() => {}}
      onCall={() => {}}
    />,
  )
}

const neededBox = () => screen.getByLabelText('Needed only') as HTMLInputElement
const workedBox = () => screen.getByLabelText('Hide worked') as HTMLInputElement

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('Call Roster filters initialize from storage', () => {
  it('starts with both off and every station shown when nothing is stored', () => {
    mount()
    expect(neededBox().checked).toBe(false)
    expect(workedBox().checked).toBe(false)
    expect(screen.queryByText('PLAIN1')).not.toBeNull()
    expect(screen.queryByText('NEEDED1')).not.toBeNull()
    expect(screen.queryByText('WORKED1')).not.toBeNull()
  })

  it('comes up with Needed-only ticked AND applied', () => {
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ neededOnly: true, hideWorked: false, hideBlocked: false }))
    mount()
    expect(neededBox().checked).toBe(true)
    // The checkbox and the rows must agree — a ticked box over an unfiltered list would be
    // the same bug wearing the fix.
    expect(screen.queryByText('NEEDED1')).not.toBeNull()
    expect(screen.queryByText('PLAIN1')).toBeNull()
    expect(screen.queryByText('WORKED1')).toBeNull()
  })

  it('comes up with Hide-worked ticked AND applied', () => {
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ neededOnly: false, hideWorked: true, hideBlocked: false }))
    mount()
    expect(workedBox().checked).toBe(true)
    expect(screen.queryByText('WORKED1')).toBeNull()
    expect(screen.queryByText('PLAIN1')).not.toBeNull()
  })

  it('ignores a corrupt stored value and shows everything', () => {
    localStorage.setItem(ROSTER_FILTER_KEY, '{"neededOnly":tr')
    mount()
    expect(neededBox().checked).toBe(false)
    expect(workedBox().checked).toBe(false)
    expect(screen.queryByText('PLAIN1')).not.toBeNull()
  })
})

describe('Call Roster filters are written when ticked', () => {
  it('persists Needed-only, and does so without disturbing Hide-worked', () => {
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ neededOnly: false, hideWorked: true, hideBlocked: false }))
    mount()
    fireEvent.click(neededBox())
    expect(loadRosterFilters()).toEqual({ neededOnly: true, hideWorked: true, hideBlocked: false })
  })

  it('persists Hide-worked, and unticking persists the OFF state too', () => {
    mount()
    fireEvent.click(workedBox())
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: true, hideBlocked: false })
    fireEvent.click(workedBox())
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: false, hideBlocked: false })
  })

  it('survives a remount — the whole point of the change', () => {
    mount()
    fireEvent.click(neededBox())
    cleanup()
    mount() // a fresh component, as after a restart
    expect(neededBox().checked).toBe(true)
    expect(screen.queryByText('PLAIN1')).toBeNull()
  })
})

// ── Hide worked vs. a live park/summit activation ────────────────────────────────────
//
// The defect (operator, 2026-09-15): "Hide worked" hid a station he had worked before even
// while that station was RIGHT NOW activating a different park. `worked` means "this callsign
// is in the logbook" — ever, not per band, not per park — while the Needed board was listing
// the activation, so the two panes disagreed about one station at one moment.
//
// The fix is not a rule of this pane's own. The backend now decides park-level worked state
// once (`HuntedActivations`, needalert.rs) and hands both surfaces the SAME alerts, so a park
// still to be worked arrives here as a real need tag — `NewPark` — and survives for exactly
// the reason it appears on the board. These tests therefore assert on the tag, which is the
// thing that is shared; if they were written against a roster-only predicate they would go on
// passing after the two surfaces had drifted apart again.
//
// What the 2026-08-23 ruling protected still holds: a DXPEDITION chip does not rescue a worked
// station, because being a DXpedition is not something you can need.
describe('Hide worked and the Needed board agree about a park activator', () => {
  const alert = (call: string, tags: NeedTag[], reference = 'US-0002'): NeedAlert => ({
    call,
    entity: 'United States',
    band: '20m',
    zone: 4,
    tags,
    priority: NEED_TIER[tags[0]],
    headline: `POTA ${reference}`,
    mode: 'Digital',
    freqMhz: 14.074,
  })

  const ACTIVITY_STATIONS = [
    station('PLAIN1'),
    // Worked before, nothing to gain, no activity — the CONTROL that proves the filter still
    // filters. If this row ever survives, the fix has simply disabled Hide worked.
    station('WORKED1', { worked: true }),
    // Worked yesterday at park A; on the air from park B now. The operator's own case.
    station('NEWPARK1', { worked: true }),
    // Worked before, on a summit not yet worked in this activation.
    station('NEWSUMMIT1', { worked: true }),
    // Worked before, activating a park ALREADY worked today — the backend withheld NewPark,
    // so this one has nothing left to offer and must go quiet.
    station('SAMEPARK1', { worked: true }),
    // Worked before and part of an announced DXpedition — the 2026-08-23 ruling's case.
    station('DXPED1', { worked: true }),
  ]
  const ACTIVITY_ALERTS = new Map<string, NeedAlert[]>([
    ['NEWPARK1', [alert('NEWPARK1', ['NewPark', 'Pota'])]],
    ['NEWSUMMIT1', [alert('NEWSUMMIT1', ['NewPark', 'Sota'], 'W7A/MN-001')]],
    ['SAMEPARK1', [alert('SAMEPARK1', ['Pota'])]],
    ['DXPED1', [alert('DXPED1', ['Dxped'])]],
  ])

  function mountActivity() {
    return render(
      <OperateRoster
        stations={ACTIVITY_STATIONS}
        myGrid="EN52"
        currentSlot={SLOT}
        needByCall={new Map()}
        needAlertsByCall={ACTIVITY_ALERTS}
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
  }

  beforeEach(() => {
    localStorage.setItem(
      ROSTER_FILTER_KEY,
      JSON.stringify({ neededOnly: false, hideWorked: true, hideBlocked: false }),
    )
  })

  it('keeps a worked call that is at a park it has not worked in this activation', () => {
    mountActivity()
    expect(screen.queryByText('NEWPARK1')).not.toBeNull()
  })

  it('keeps a worked call that is on a summit it has not worked in this activation', () => {
    mountActivity()
    expect(screen.queryByText('NEWSUMMIT1')).not.toBeNull()
  })

  it('hides a call already worked at that park in this activation', () => {
    mountActivity()
    expect(screen.queryByText('SAMEPARK1')).toBeNull()
  })

  it('still hides a worked call with no activity and no need (the filter still filters)', () => {
    mountActivity()
    expect(screen.queryByText('WORKED1')).toBeNull()
    expect(screen.queryByText('PLAIN1')).not.toBeNull()
  })

  it('still hides a worked DXpedition — a DXped chip is a label, not a need', () => {
    mountActivity()
    expect(screen.queryByText('DXPED1')).toBeNull()
  })

  // BY CONSTRUCTION, not by coincidence. Feed the identical alerts to the board's own pipeline
  // and assert the two surfaces reach the same verdict about each station. The defect was
  // precisely that these two answers were computed from different things.
  it('reaches the same verdict as the Needed board on every one of them', () => {
    mountActivity()
    const board = filterAlerts(boardNeeds([...ACTIVITY_ALERTS.values()].flat()), DEFAULT_FILTERS)
    const boardSaysNeeded = (call: string) =>
      board.some((a) => a.call === call && a.tags.some((t) => !isActivityTag(t)))
    for (const call of ['NEWPARK1', 'NEWSUMMIT1']) {
      expect(boardSaysNeeded(call), `${call} on the board`).toBe(true)
      expect(screen.queryByText(call), `${call} on the roster`).not.toBeNull()
    }
    for (const call of ['SAMEPARK1', 'DXPED1']) {
      expect(boardSaysNeeded(call), `${call} on the board`).toBe(false)
      expect(screen.queryByText(call), `${call} on the roster`).toBeNull()
    }
  })
})

// ── #350: a confirmation already on its way is not a reason to keep a row ────────────────
//
// Operator report: "Needed only" and "Hide worked" still showed the station he had just worked.
// The row survived on its LoTW chip. A fresh contact is unconfirmed, so the backend's Confirm
// tier tags that slot at once — deliberately (needalert.rs pins it: a worked-but-unconfirmed
// row "must NOT be 'fixed'"), and the chip stays. What changes is only what the two FILTERS
// count: with the station already worked on this band, another contact with them cannot
// confirm anything the first one will not, so a need that is ONLY that confirmation stops
// holding the row on the list.
//
// The controls matter as much as the case: a Confirm on a station NOT worked on this band is
// still a slot a contact here can confirm, and any real chase need (a park, a state) riding
// beside the Confirm keeps the row exactly as before.
describe('the filters and a need that is only a confirmation (#350)', () => {
  const confirmAlert = (call: string, tags: NeedTag[]): NeedAlert => ({
    call,
    entity: 'United States',
    band: '20m',
    zone: 4,
    tags,
    priority: NEED_TIER[tags[0]],
    headline: '',
    mode: 'FT8',
    freqMhz: 14.074,
  })

  const ROWS = [
    // Worked on THIS band a moment ago; all the backend still says is "confirm it".
    station('JUSTWORKED1', { worked: true, workedBand: true }),
    // Worked, but on another band: here the same Confirm is a 20 m slot a contact can confirm.
    station('OTHERBAND1', { worked: true, workedBand: false }),
    // Never worked: the Confirm is a slot somebody else filled, which this station can confirm.
    station('NEVERWORKED1'),
    // Worked on this band, but at a park not yet worked in the activation running now.
    station('PARKTOO1', { worked: true, workedBand: true }),
    // Worked on this band, and still a new state here.
    station('STATETOO1', { worked: true, workedBand: true }),
  ]
  const ALERTS = new Map<string, NeedAlert[]>([
    ['JUSTWORKED1', [confirmAlert('JUSTWORKED1', ['Confirm'])]],
    ['OTHERBAND1', [confirmAlert('OTHERBAND1', ['Confirm'])]],
    ['NEVERWORKED1', [confirmAlert('NEVERWORKED1', ['Confirm'])]],
    ['PARKTOO1', [confirmAlert('PARKTOO1', ['NewPark', 'Confirm', 'Pota'])]],
    ['STATETOO1', [confirmAlert('STATETOO1', ['NewState', 'Confirm'])]],
  ])

  function mountWith(filters: { neededOnly: boolean; hideWorked: boolean }) {
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ ...filters, hideBlocked: false }))
    return render(
      <OperateRoster
        stations={ROWS}
        myGrid="EN52"
        currentSlot={SLOT}
        needByCall={new Map()}
        needAlertsByCall={ALERTS}
        band="20m"
        feedMode="FT8"
        selectedCall={null}
        onSelect={() => {}}
        onCall={() => {}}
      />,
    )
  }

  const shown = (call: string) => screen.queryByText(call) != null

  it('Hide worked hides a station worked on this band whose only need is the confirmation', () => {
    mountWith({ neededOnly: false, hideWorked: true })
    expect(shown('JUSTWORKED1'), 'the station just worked is still on the list').toBe(false)
  })

  it('Needed only hides it too', () => {
    mountWith({ neededOnly: true, hideWorked: false })
    expect(shown('JUSTWORKED1'), 'the station just worked is still on the list').toBe(false)
  })

  it('keeps the same confirmation when the station was worked on another band', () => {
    mountWith({ neededOnly: true, hideWorked: true })
    expect(shown('OTHERBAND1')).toBe(true)
  })

  it('keeps a confirmation on a station never worked', () => {
    mountWith({ neededOnly: true, hideWorked: true })
    expect(shown('NEVERWORKED1')).toBe(true)
  })

  it('keeps a station worked on this band that still has a real need beside the confirmation', () => {
    mountWith({ neededOnly: true, hideWorked: true })
    expect(shown('PARKTOO1'), 'a park not yet worked in this activation').toBe(true)
    expect(shown('STATETOO1'), 'a new state').toBe(true)
  })

  it('with the filters off, the station just worked still wears its LoTW chip', () => {
    mountWith({ neededOnly: false, hideWorked: false })
    const row = screen.getByText('JUSTWORKED1').closest('[role="row"]') as HTMLElement
    expect(row, 'the row is on the list').not.toBeNull()
    expect(row.querySelector('.need-chip.need-confirm')?.textContent).toBe('LoTW')
  })
})

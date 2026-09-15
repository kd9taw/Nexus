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

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
// while that station was RIGHT NOW activating a different park. The Needed board and the
// Roster gave opposite answers about the same station in the same state — `activation_alert`
// (needalert.rs) always yields a board row for a live activator, while this filter kept a
// worked station only when `need` was non-null, and `need` deliberately excludes the
// Dxped/Pota/Sota activity tags (the 2026-08-23 ruling).
//
// What the ruling protected must survive: a DXPEDITION chip still does not rescue a worked
// station, because a DXpedition is not something you can *need* — it is a label. The
// difference is not "activity tags are needs after all"; it is that a live park/summit
// activation is a board ROW in its own right and a DXpedition decoration never is.
describe('Hide worked keeps a station that is on the air from a park or summit', () => {
  const alerts = (call: string, tags: NeedTag[]): NeedAlert => ({
    call,
    entity: 'United States',
    band: '20m',
    zone: 4,
    tags,
    priority: 20,
    headline: 'POTA US-0002',
    mode: 'Digital',
    freqMhz: 14.074,
  })

  const ACTIVITY_STATIONS = [
    station('PLAIN1'),
    // Worked before, nothing to gain, no activity — the CONTROL that proves the filter
    // still filters. If this row ever survives, the fix has simply disabled Hide worked.
    station('WORKED1', { worked: true }),
    // Worked before, and on the air from a park right now.
    station('POTA1', { worked: true }),
    // Worked before, and on a summit right now.
    station('SOTA1', { worked: true }),
    // Worked before and part of an announced DXpedition — the 2026-08-23 ruling's case.
    station('DXPED1', { worked: true }),
  ]
  const ACTIVITY_ALERTS = new Map<string, NeedAlert[]>([
    ['POTA1', [alerts('POTA1', ['Pota'])]],
    ['SOTA1', [alerts('SOTA1', ['Sota'])]],
    ['DXPED1', [alerts('DXPED1', ['Dxped'])]],
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

  it('keeps a worked call that is activating a park', () => {
    mountActivity()
    expect(screen.queryByText('POTA1')).not.toBeNull()
  })

  it('keeps a worked call that is activating a summit', () => {
    mountActivity()
    expect(screen.queryByText('SOTA1')).not.toBeNull()
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
})

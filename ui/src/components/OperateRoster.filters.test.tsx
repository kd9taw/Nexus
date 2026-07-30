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
import type { NeedTag, Station } from '../types'

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
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ neededOnly: true, hideWorked: false }))
    mount()
    expect(neededBox().checked).toBe(true)
    // The checkbox and the rows must agree — a ticked box over an unfiltered list would be
    // the same bug wearing the fix.
    expect(screen.queryByText('NEEDED1')).not.toBeNull()
    expect(screen.queryByText('PLAIN1')).toBeNull()
    expect(screen.queryByText('WORKED1')).toBeNull()
  })

  it('comes up with Hide-worked ticked AND applied', () => {
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ neededOnly: false, hideWorked: true }))
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
    localStorage.setItem(ROSTER_FILTER_KEY, JSON.stringify({ neededOnly: false, hideWorked: true }))
    mount()
    fireEvent.click(neededBox())
    expect(loadRosterFilters()).toEqual({ neededOnly: true, hideWorked: true })
  })

  it('persists Hide-worked, and unticking persists the OFF state too', () => {
    mount()
    fireEvent.click(workedBox())
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: true })
    fireEvent.click(workedBox())
    expect(loadRosterFilters()).toEqual({ neededOnly: false, hideWorked: false })
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

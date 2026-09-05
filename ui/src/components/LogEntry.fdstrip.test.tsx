// @vitest-environment jsdom
//
// Two properties of the SHARED log strip when Field Day is on — the strip the Phone, CW, RTTY
// and PSK cockpits all render during an event, since that is where Field Day contacts are made.
//
//  1. THE STRIP NEVER GRABS FOCUS ON MOUNT. A focused text field disarms the window Space
//     handler these cockpits key the rig with — Phone's Space keyup is a member of its
//     stop-line census — so a mount-time `focus()` kills push-to-talk from the moment the
//     cockpit appears, for the whole of the event. That shipped once, from an autofocus gated
//     on `fdActive` alone, and this is the pin that stops it coming back.
//     (`reset()`'s refocus after a LOGGED contact is a different thing and is not covered here:
//     it follows an action the operator took in this strip.)
//  2. The FD callsign field drops spaces, for the same push-to-talk reason one level down.
//
// The standard-log variant is rendered alongside both, because the guards are the reason the
// strip's other consumers (Phone and CW off Field Day, Satellites) are unaffected.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

const fieldDay = {
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: '',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
} as unknown as FieldDayStatus

const call = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
// The standard variant's callsign box — a different layout, so a different placeholder.
const stdCall = () => screen.getByPlaceholderText('Call') as HTMLInputElement

/** The FD variant, as the Phone, CW, RTTY and PSK cockpits host it during an event. */
function renderFd() {
  render(
    <LogEntry
      snap={snap}
      mode="PH"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={fieldDay}
      fdMode="PH"
    />,
  )
}

/** The standard-log variant — Phone/CW off Field Day, and Satellites. */
function renderStd() {
  render(
    <LogEntry snap={snap} mode="PH" defaultRst="59" exchange="terrestrial" fieldDay={null} />,
  )
}

afterEach(() => cleanup())

describe('LogEntry — the FD strip takes no focus when it mounts', () => {
  it('leaves focus where the operator put it — the cockpits key off the space bar', () => {
    // ⚠️ THE REGRESSION THIS PINS. An autofocus gated on `fdActive` alone fired inside
    // PhoneCockpit and CwCockpit: their window Space handler is guarded on "not while typing
    // in a field", so from the moment either cockpit appeared during Field Day the space bar
    // typed a space instead of keying the rig — and Phone's Space keyup is a member of that
    // cockpit's stop-line census.
    renderFd()
    expect(
      document.activeElement,
      'the FD strip grabbed focus on mount — Space PTT is dead in every cockpit hosting it',
    ).toBe(document.body)
  })

  it('takes no focus on the standard-log path either', () => {
    renderStd()
    expect(document.activeElement).not.toBe(stdCall())
    expect(document.activeElement).toBe(document.body)
  })
})

describe('LogEntry — the FD callsign field drops spaces', () => {
  it('a space typed mid-callsign never reaches the value, or the log', () => {
    // The space bar is the phone position's push-to-talk; a reflexive press while the caret is
    // in Call would otherwise log "K1 ABC" verbatim (`logIt` only trims the ends).
    renderFd()
    fireEvent.change(call(), { target: { value: 'k1 abc' } })
    expect((call() as HTMLInputElement).value).toBe('K1ABC')
  })
})

describe('LogEntry — space walks the contest exchange, like N1MM and N3FJP', () => {
  // N1MM's keyboard reference puts it in capitals — "SPACE IS THE PREFERRED TAB CHARACTER" —
  // and N3FJP's own help tells operators to "press the space bar to tab". Contest operators
  // arrive with that in their fingers, and a logger that ignores it feels broken at speed.
  // Tab is unchanged (plain DOM order); this adds a second way, it does not replace one.
  const cls = () => screen.getByPlaceholderText('1D')
  const sect = () => screen.getByPlaceholderText('WI')

  it('walks Call → Class → Section → Call', () => {
    renderFd()
    call().focus()
    fireEvent.keyDown(call(), { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(cls())
    fireEvent.keyDown(cls(), { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(sect())
    // Wraps, so a corrected exchange gets back to the call without reaching for the mouse.
    fireEvent.keyDown(sect(), { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(call())
  })

  it('the space never reaches the value — Class and Section have no stripping of their own', () => {
    // Call drops spaces on change; these two do not, so without preventDefault a reflexive
    // press would log a class of "3A " or a section of " WI".
    renderFd()
    fireEvent.change(cls(), { target: { value: '3A' } })
    const ev = fireEvent.keyDown(cls(), { key: ' ', code: 'Space' })
    expect(ev).toBe(false) // preventDefault called
    expect((cls() as HTMLInputElement).value).toBe('3A')
  })

  it('POSITIVE CONTROL: an ordinary character still types', () => {
    // Otherwise this suite would pass against a handler that swallowed every key.
    renderFd()
    fireEvent.change(cls(), { target: { value: '3A' } })
    fireEvent.keyDown(cls(), { key: 'A', code: 'KeyA' })
    expect(document.activeElement).not.toBe(sect())
  })
})

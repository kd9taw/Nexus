// @vitest-environment jsdom
//
// The two FD-cockpit seams on the shared log strip.
//
//  1. `onFdDraftChange` — the in-progress exchange mirrored out as it is typed, so a host can
//     paint boards from the DRAFT (the band × mode grid lights the cells where this call is
//     already worked; the sections checklist glows the section) before anything is logged. The
//     values are normalized the way the strip's own dupe verdict normalizes them — trimmed,
//     uppercased — because a board and the dupe check must never disagree about the key.
//  2. FD autofocus on mount — a run starts in the callsign field, which is where `reset()`
//     already puts focus after every logged contact. OPT-IN (`autoFocusCall`), and the opt-in
//     is safety: this strip is ALSO hosted by the Phone and CW cockpits during Field Day, and
//     a focused text field disarms their window Space handler — Phone's Space keyup is on its
//     stop-line census. Focusing on `fdActive` alone killed push-to-talk in a shipped cockpit
//     for the whole of the event, from the moment it appeared.
//
// Both are FD-ONLY. The strip's other consumers (Phone and CW off Field Day, Satellites) pass
// no callback and get no focus grab: they are the reason each half carries an `fdActive` guard,
// and the standard-variant tests below are what hold those guards in place.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
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
const klass = () => screen.getByPlaceholderText('1D') as HTMLInputElement
const section = () => screen.getByPlaceholderText('WI') as HTMLInputElement
const logBtn = () => screen.getByRole('button', { name: /log fd/i }) as HTMLButtonElement
// The standard variant's callsign box — a different layout, so a different placeholder.
const stdCall = () => screen.getByPlaceholderText('Call') as HTMLInputElement

/** The FD variant, as the Field Day cockpit hosts it. */
function renderFd(onFdDraftChange?: (d: { call: string; cls: string; section: string }) => void) {
  render(
    <LogEntry
      snap={snap}
      mode="PH"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={fieldDay}
      fdMode="PH"
      onFdDraftChange={onFdDraftChange}
      autoFocusCall
    />,
  )
}

/** The FD variant as the PHONE and CW cockpits host it during Field Day — same strip, same
 *  `fieldDay`, and no autofocus, because those cockpits key the rig off the space bar. */
function renderFdHosted() {
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
function renderStd(onFdDraftChange?: (d: { call: string; cls: string; section: string }) => void) {
  render(
    <LogEntry
      snap={snap}
      mode="PH"
      defaultRst="59"
      exchange="terrestrial"
      fieldDay={null}
      onFdDraftChange={onFdDraftChange}
    />,
  )
}

const lastDraft = (spy: ReturnType<typeof vi.fn>) => spy.mock.calls[spy.mock.calls.length - 1][0]

afterEach(() => cleanup())

describe('LogEntry — the FD draft mirror', () => {
  it('reports the exchange as it is typed, one field at a time', () => {
    const spy = vi.fn()
    renderFd(spy)
    // The mount draft already carries the class prefilled from the FD status, so a board
    // painting the current cell has the whole key before the operator touches anything.
    expect(lastDraft(spy)).toEqual({ call: '', cls: '3A', section: '' })

    fireEvent.change(call(), { target: { value: 'k1abc' } })
    expect(lastDraft(spy)).toEqual({ call: 'K1ABC', cls: '3A', section: '' })

    fireEvent.change(section(), { target: { value: 'wi' } })
    expect(lastDraft(spy)).toEqual({ call: 'K1ABC', cls: '3A', section: 'WI' })

    fireEvent.change(klass(), { target: { value: '2a' } })
    expect(lastDraft(spy)).toEqual({ call: 'K1ABC', cls: '2A', section: 'WI' })
  })

  it('normalizes every field the way the dupe key is normalized — trimmed and uppercased', () => {
    const spy = vi.fn()
    renderFd(spy)
    fireEvent.change(call(), { target: { value: '  k1abc ' } })
    fireEvent.change(klass(), { target: { value: ' 3a ' } })
    fireEvent.change(section(), { target: { value: ' wi ' } })
    // Not ' K1ABC ' / ' 3A ' / ' WI ': a board that took these raw would look up a cell that
    // the log path — which trims — never writes.
    expect(lastDraft(spy)).toEqual({ call: 'K1ABC', cls: '3A', section: 'WI' })
  })

  it('clears the call when a logged contact resets the strip, keeping the exchange', async () => {
    const spy = vi.fn()
    renderFd(spy)
    fireEvent.change(call(), { target: { value: 'k1abc' } })
    fireEvent.change(section(), { target: { value: 'wi' } })
    expect(logBtn().disabled).toBe(false)
    fireEvent.click(logBtn())
    // The boards must stop painting the contact that is now logged — and must keep painting
    // the class/section a run re-uses, which `reset()` deliberately preserves.
    await waitFor(() => expect(lastDraft(spy)).toEqual({ call: '', cls: '3A', section: 'WI' }))
  })

  it('is never called on the standard-log path, however much is typed', () => {
    const spy = vi.fn()
    renderStd(spy)
    fireEvent.change(stdCall(), { target: { value: 'k1abc' } })
    expect(spy).not.toHaveBeenCalled()
  })
})

describe('LogEntry — FD autofocus on mount', () => {
  it('lands focus in the callsign field when the host asked for it', () => {
    renderFd()
    expect(document.activeElement).toBe(call())
  })

  it('takes NO focus in a Field Day host that did not ask — Phone and CW key off the space bar', () => {
    // ⚠️ THE REGRESSION THIS PINS. The autofocus first shipped gated on `fdActive` alone, so it
    // fired inside PhoneCockpit and CwCockpit too: their window Space handler is guarded on
    // "not while typing in a field", so from the moment either cockpit appeared during Field
    // Day the space bar typed a space instead of keying the rig — and Phone's Space keyup is a
    // member of that cockpit's stop-line census. Same strip, same `fieldDay`, no `autoFocusCall`.
    renderFdHosted()
    expect(
      document.activeElement,
      'the FD strip grabbed focus in a host that never asked — Space PTT is dead there',
    ).toBe(document.body)
  })

  it('takes no focus on the standard-log path', () => {
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

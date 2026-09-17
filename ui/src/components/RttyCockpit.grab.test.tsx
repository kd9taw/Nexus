// @vitest-environment jsdom
//
// DOUBLE-CLICK A CALL IN DECODED TEXT AND IT LANDS IN THE CALL BOX AND THE LOG STRIP.
//
// The token rules are `features/rttyGrab.test.ts`. This file is the wiring, with the log strip
// rendered for real: the two call fields are one callsign, linked both ways, and the grab must
// never move the caret — while continuous TX is latched the compose bar is the only field that
// feeds the air, and a grab that took the caret out of it would silently stop the transmission
// taking what the operator types.
//
// ⚠️ jsdom DOES NOT LAY OUT, so it has no caret hit-test: `document.caretRangeFromPoint` is
// absent. Each test installs one that answers the character offset a real browser would, into
// the REAL text nodes the cockpit rendered — so the cockpit's own node-offset → string-offset
// mapping is what is exercised, not a shortcut around it. Which character is under a pixel is
// the Windows tester build's to check.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import { StationControlContext } from '../stationAccess'
import * as toast from '../toast'
import type { AppSnapshot, LoggedQso, RttyState } from '../types'

// Solid copy with a faint run through the middle of the compound call, so the transcript
// renders as SEVERAL spans and the call straddles two of them.
const TEXT = 'CQ TEST DE VE3/K1ABC VE3/K1ABC CQ\r\n'
const CONF = Array.from(TEXT, (_, i) => (i >= 14 && i < 18 ? 20 : 95))

const state: { current: RttyState } = {
  current: {
    armed: true,
    afcHz: 0,
    afcLocked: false,
    text: TEXT,
    charConf: CONF,
    baud: 45.45,
    shiftHz: 170,
    markHz: 2125,
    spaceHz: 2295,
    sending: false,
    latched: false,
    backend: 'afsk',
    keyerError: null,
    auto: false,
    seqState: 'idle',
    peer: null,
    peerExchange: [],
    heardCq: null,
  } as unknown as RttyState,
}

const logQso = vi.fn(async (rec: LoggedQso) => rec)
const contestLog = vi.fn(async (..._a: unknown[]) => ({}))

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(async () => state.current),
  rttyAutoArm: vi.fn(async () => state.current),
  rttySend: vi.fn(async () => state.current),
  rttyStop: vi.fn(async () => state.current),
  rttyClear: vi.fn(async () => state.current),
  rttyAfcReset: vi.fn(async () => state.current),
  rttyNet: vi.fn(async () => state.current),
  rttySetAuto: vi.fn(async () => state.current),
  rttySetLatched: vi.fn(async () => state.current),
  rttyType: vi.fn(async () => state.current),
  rttyAutoCq: vi.fn(async () => state.current),
  rttyAutoAnswer: vi.fn(async () => state.current),
  rttyAutoAbort: vi.fn(async () => state.current),
  setRfPower: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  atuTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  logQso: (rec: LoggedQso) => logQso(rec),
  contestLogManual: (...a: unknown[]) => contestLog(...a),
  contestZoneHint: vi.fn(async () => null),
  contestIMoved: vi.fn(async () => ({})),
  getLog: vi.fn(async () => [] as LoggedQso[]),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  lookupPark: vi.fn(async () => null),
  lookupParkLive: vi.fn(async () => null),
  searchParks: vi.fn(async () => []),
  setCwPeerInfo: vi.fn(async () => {}),
  openQrzPage: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const pushToast = toast.pushToast as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'W9XYZ',
  mygrid: 'EN61',
  hunt: null,
  b4MatchMode: false,
  radio: {
    dialMhz: 14.08,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    txAllowed: true,
  },
} as unknown as AppSnapshot

/** A browser's caret hit-test, answered from the REAL rendered text nodes: the insertion point
 *  `offset` characters into the transcript. */
function caretAt(offset: number) {
  const hit = () => {
    const box = document.querySelector('.cw-decode-text') as HTMLElement
    const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT)
    let left = offset
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const len = (n.nodeValue ?? '').length
      if (left <= len) {
        const r = document.createRange()
        r.setStart(n, left)
        r.collapse(true)
        return r
      }
      left -= len
    }
    return null
  }
  Object.defineProperty(document, 'caretRangeFromPoint', { value: hit, configurable: true })
}

async function renderCockpit(control = true) {
  const r = render(
    <StationControlContext.Provider value={control}>
      <RttyCockpit snap={snap} />
    </StationControlContext.Provider>,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  await waitFor(() => expect(document.querySelector('.cw-decode-text')!.textContent).toBe(TEXT))
  return r
}

const stream = () => document.querySelector('.cw-decode-text') as HTMLElement
const callBox = () => document.querySelector('.rtty-hiscall') as HTMLInputElement
const logCall = () => document.querySelector('.le-call') as HTMLInputElement
const compose = () => document.querySelector('.cw-type') as HTMLInputElement

/** The browser's own sequence for a double-click: two presses (the first one counting 1),
 *  then `dblclick`. */
async function doubleClick() {
  await act(async () => {
    fireEvent.mouseDown(stream(), { detail: 1 })
    fireEvent.mouseUp(stream(), { detail: 1 })
    fireEvent.mouseDown(stream(), { detail: 2 })
    fireEvent.mouseUp(stream(), { detail: 2 })
    fireEvent.doubleClick(stream(), { detail: 2 })
  })
}

const BASE_STATE = state.current

beforeEach(() => {
  logQso.mockClear()
  contestLog.mockClear()
  pushToast.mockClear()
})
afterEach(() => {
  Reflect.deleteProperty(document, 'caretRangeFromPoint')
  window.getSelection()?.removeAllRanges()
  state.current = BASE_STATE
  cleanup()
})

describe('grab a callsign from Decoded text', () => {
  it('fills the Call box AND the log strip — the whole compound call, across the faint run', async () => {
    await renderCockpit()
    expect(stream().querySelectorAll('span').length, 'fixture must straddle spans').toBeGreaterThan(1)
    caretAt(TEXT.indexOf('K1ABC') + 2)
    await doubleClick()
    expect(callBox().value).toBe('VE3/K1ABC')
    await waitFor(() => expect(logCall().value).toBe('VE3/K1ABC'))
  })

  it('does nothing, and says nothing, on a word that is not a call', async () => {
    await renderCockpit()
    caretAt(TEXT.indexOf('TEST') + 1)
    await doubleClick()
    await new Promise((r) => setTimeout(r, 600)) // past the Call box's settle debounce
    expect(callBox().value).toBe('')
    expect(logCall().value).toBe('')
    expect(pushToast).not.toHaveBeenCalled()
  })

  it('falls back to the native selection when the browser has no caret hit-test', async () => {
    await renderCockpit()
    // What a double-click leaves selected: native word selection stops at the slash, so it
    // selected K1ABC alone — the grab still fills the WHOLE compound call.
    // The anchor sits in the SECOND span (the faint run), at the K.
    const [solid, faint] = Array.from(stream().querySelectorAll('span'))
    const sel = window.getSelection()!
    const r = document.createRange()
    r.setStart(faint.firstChild as Text, TEXT.indexOf('K1ABC') - (solid.textContent ?? '').length)
    r.collapse(true)
    sel.removeAllRanges()
    sel.addRange(r)
    await doubleClick()
    expect(callBox().value).toBe('VE3/K1ABC')
  })

  it('hands the caret back to the compose bar it was taken from', async () => {
    await renderCockpit()
    compose().focus()
    expect(document.activeElement, 'fixture: caret in the compose bar').toBe(compose())
    caretAt(TEXT.indexOf('K1ABC'))
    await act(async () => {
      fireEvent.mouseDown(stream(), { detail: 1 })
      // The browser's default action on a press over plain text: focus leaves the input.
      compose().blur()
      fireEvent.mouseUp(stream(), { detail: 1 })
      fireEvent.mouseDown(stream(), { detail: 2 })
      fireEvent.mouseUp(stream(), { detail: 2 })
      fireEvent.doubleClick(stream(), { detail: 2 })
    })
    expect(callBox().value).toBe('VE3/K1ABC')
    expect(document.activeElement, 'the grab left the caret out of the compose bar').toBe(compose())
    // …and not in either call field, which is where a fill that "helpfully" focused would put it.
    await waitFor(() => expect(logCall().value).toBe('VE3/K1ABC'))
    expect(document.activeElement).toBe(compose())
  })

  it('refills the same call after it was logged — both fields', async () => {
    await renderCockpit()
    caretAt(TEXT.indexOf('K1ABC'))
    await doubleClick()
    await waitFor(() => expect(logCall().value).toBe('VE3/K1ABC'))
    await act(async () => {
      fireEvent.click(document.querySelector('.le-log-btn') as HTMLElement)
    })
    await waitFor(() => expect(logQso).toHaveBeenCalledTimes(1))
    expect(logQso.mock.calls[0][0].call).toBe('VE3/K1ABC')
    // Logging clears BOTH: the strip resets, and the Call box follows it.
    await waitFor(() => expect(logCall().value).toBe(''))
    await waitFor(() => expect(callBox().value).toBe(''))
    // The same station calls again (a dupe, a second band) — the grab must land again.
    await doubleClick()
    expect(callBox().value).toBe('VE3/K1ABC')
    await waitFor(() => expect(logCall().value).toBe('VE3/K1ABC'))
  })

  it('is inert for a Remote observer, whose Call box is disabled', async () => {
    await renderCockpit(false)
    caretAt(TEXT.indexOf('K1ABC'))
    await doubleClick()
    expect(callBox().value).toBe('')
  })
})

describe('the Call box and the log callsign are one field', () => {
  it('Call box → log: fills once typing settles, without moving the caret', async () => {
    await renderCockpit()
    callBox().focus()
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: 'w1aw' } })
    })
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    expect(document.activeElement).toBe(callBox())
  })

  it('log → Call box: typing the callsign into the strip fills the dock', async () => {
    await renderCockpit()
    logCall().focus()
    await act(async () => {
      fireEvent.change(logCall(), { target: { value: 'k1abc' } })
    })
    await waitFor(() => expect(callBox().value).toBe('K1ABC'))
    // Past the settle debounce: the value must not be filled straight back over the strip.
    await new Promise((r) => setTimeout(r, 600))
    expect(logCall().value).toBe('K1ABC')
    expect(document.activeElement).toBe(logCall())
  })

  it('keeps every keystroke typed into the strip while the dock follows', async () => {
    // Keystroke by keystroke, then past the dock's settle debounce: whatever the dock hands back
    // must be what the strip already holds. (The in-flight race itself — a keystroke landing
    // between a report and its echo — needs a real event loop; `linkedCall` in LogEntry is
    // what closes it, and its comment says how.)
    await renderCockpit()
    for (const v of ['W', 'W9', 'W9X', 'W9XY', 'W9XYZ']) {
      await act(async () => {
        fireEvent.change(logCall(), { target: { value: v } })
      })
    }
    await new Promise((r) => setTimeout(r, 600))
    expect(logCall().value).toBe('W9XYZ')
    expect(callBox().value).toBe('W9XYZ')
  })

  it('clearing the Call box clears the log callsign', async () => {
    await renderCockpit()
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: 'W1AW' } })
    })
    await waitFor(() => expect(logCall().value).toBe('W1AW'))
    await act(async () => {
      fireEvent.change(callBox(), { target: { value: '' } })
    })
    await waitFor(() => expect(logCall().value).toBe(''))
  })
})

// ── INSIDE A CONTEST ──────────────────────────────────────────────────────────────────────
//
// The same double-click also fills the EXCHANGE, and nothing about the contest is written
// into the cockpit: the session publishes the slots it receives (`snap.fieldDay.receives`),
// and the zone's bounds and the QTH's domain come off those specs. CQ WW RTTY's shape is
// RST + CQ zone (1–40) + an optional QTH that only W/VE stations send.
const CQ_WW = [
  { key: 'RST', kind: 'rst', required: true },
  { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
  { key: 'QTH', kind: 'enum', required: false, domain: 'fd_sections' },
]
const CONTEST_TEXT = 'W1AW 599 14 WI\r\n'

async function renderContest(receives: unknown[] = CQ_WW, text = CONTEST_TEXT) {
  state.current = { ...state.current, text, charConf: Array.from(text, () => 95) } as RttyState
  const contestSnap = {
    ...snap,
    fieldDay: {
      running: true,
      state: 'run',
      event: 'cqwwrtty',
      qsoCount: 0,
      sections: 0,
      points: 0,
      log: [],
      receives,
      sentExchange: '04 WI',
    },
  } as unknown as AppSnapshot
  const r = render(
    <StationControlContext.Provider value={true}>
      <RttyCockpit snap={contestSnap} />
    </StationControlContext.Provider>,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  await waitFor(() => expect(document.querySelector('.cw-decode-text')!.textContent).toBe(text))
  return r
}

const fdCall = () => document.querySelector('.le-fd-input-call') as HTMLInputElement
const box = (caption: string) => screen.getByLabelText(caption) as HTMLInputElement

describe('grab an exchange value from Decoded text', () => {
  it('fills the Zone from a number the slot’s own bounds accept, and the QTH from a code its domain knows', async () => {
    await renderContest()
    // ⚠️ THE FILL STAMP IS NOT THE WALL CLOCK, and this is the case that says why: the strip
    // refills a box on every NEW `ts`, so two fills sharing a millisecond would be one fill.
    // The clock is frozen to put both grabs inside one — no sleep, no flake, and the cockpit's
    // own counter is what has to carry them apart.
    const clock = vi.spyOn(Date, 'now').mockReturnValue(1_758_000_000_000)
    caretAt(CONTEST_TEXT.indexOf('14'))
    await doubleClick()
    await waitFor(() => expect(box('Zone').value).toBe('14'))
    caretAt(CONTEST_TEXT.indexOf('WI'))
    await doubleClick()
    await waitFor(() => expect(box('QTH').value).toBe('WI'))
    clock.mockRestore()
    // An exchange value is not a callsign, and must not have touched either call field.
    expect(callBox().value).toBe('')
    expect(fdCall().value).toBe('')
    expect(pushToast).not.toHaveBeenCalled()
  })

  it('still grabs the callsign while the contest runs', async () => {
    await renderContest()
    caretAt(CONTEST_TEXT.indexOf('W1AW') + 1)
    await doubleClick()
    expect(callBox().value).toBe('W1AW')
    await waitFor(() => expect(fdCall().value).toBe('W1AW'))
    expect(box('Zone').value, 'a call is not a zone').toBe('')
  })

  it('fills nothing from a domain this build carries no values for — and the call is still a call', async () => {
    // ⚠️ THE POLARITY TRAP. The strip's own `inDomain` answers TRUE for a domain with no
    // value set (the honest while-typing verdict: an absent list is not evidence the operator
    // is wrong). Read that way here it would make the first word double-clicked — a CALLSIGN —
    // this contest's county.
    await renderContest([
      { key: 'RST', kind: 'rst', required: true },
      { key: 'CO', kind: 'enum', required: false, domain: 'wi_counties' },
    ])
    caretAt(CONTEST_TEXT.indexOf('W1AW') + 1)
    await doubleClick()
    expect(callBox().value).toBe('W1AW')
    expect(box('CO').value, 'a call was filed as a county').toBe('')
  })

  it('Enter in an exchange box logs the contact the grab filled', async () => {
    await renderContest()
    for (const word of ['W1AW', '14', 'WI']) {
      caretAt(CONTEST_TEXT.indexOf(word) + 1)
      await doubleClick()
    }
    await waitFor(() => expect(box('QTH').value).toBe('WI'))
    await act(async () => {
      fireEvent.keyDown(box('Zone'), { key: 'Enter' })
    })
    await waitFor(() => expect(contestLog).toHaveBeenCalledTimes(1))
    expect(contestLog.mock.calls[0][0]).toBe('W1AW')
    // The field vector, in the session's own receive order — the report defaulted, the other
    // two grabbed off the screen.
    expect(contestLog.mock.calls[0][1]).toEqual([
      ['RST', '599'],
      ['ZN', '14'],
      ['QTH', 'WI'],
    ])
    expect(contestLog.mock.calls[0][2], 'the scoring class RTTY logs under').toBe('DIG')
  })
})

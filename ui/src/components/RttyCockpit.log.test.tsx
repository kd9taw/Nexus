// @vitest-environment jsdom
//
// RTTY CAN LOG A CONTACT BY HAND — AND ON FIELD DAY IT LOGS TO THE CONTEST LOG.
//
// Field Day is documented and designed as ALL-MODE, and Winter Field Day's own rules data
// keeps RTTY legal as Digital. RTTY's auto-sequencer already WORKED a Field Day contact
// correctly (the engine builds the class/section exchange the instant the master switch is
// on) — but this cockpit rendered no `LogEntry` at all, its only pane being the decode
// stream. So a station worked by hand with the F-key macros, which is how a great deal of
// RTTY is actually operated, had nowhere to be written down: not to the general logbook, and
// not to the Field Day log that scores it, claims its section and writes its Cabrillo line.
//
// PSK IS THE MODEL, not a new shape: a second bare `CockpitPaneFrame` below the stream and
// above the pinned TX dock, out of the ⊞ vocabulary (RTTY's is {stream}), hosting no stop
// control. `RttyCockpit.structure.test.tsx` owns the shell census; this file renders the
// strip for real and asserts where a contact GOES.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, LoggedQso, RttyState } from '../types'

const state: { current: RttyState } = {
  current: {
    armed: true,
    afcHz: 0,
    afcLocked: false,
    text: 'CQ CQ DE KD9TAW',
    charConf: [],
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
const fdLogManual = vi.fn(async (..._args: unknown[]) => ({}))

vi.mock('../api', () => ({
  // The cockpit's own surface.
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
  // …and the log strip's, which is the point of this file.
  logQso: (rec: LoggedQso) => logQso(rec),
  fdLogManual: (...args: unknown[]) => fdLogManual(...args),
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

const snap = {
  mycall: 'KD9TAW',
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

/** The same rig with the Field Day master switch on. `snap.fieldDay` non-null is the ONE
 *  condition that makes a cockpit FD-aware (Engine::snapshot gates it on `fd_active`), and it
 *  does not depend on which section is on screen — Field Day is a single global engine mode. */
const fdSnap = { ...snap, fieldDay: { log: [], club: null } } as unknown as AppSnapshot

async function renderCockpit(s: AppSnapshot = snap) {
  const r = render(<RttyCockpit snap={s} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

beforeEach(() => {
  state.current = { ...state.current, auto: false, seqState: 'idle', sending: false }
  logQso.mockClear()
  fdLogManual.mockClear()
})
afterEach(cleanup)

describe('the RTTY cockpit can log a contact by hand', () => {
  it('renders the shared log strip in a framed pane, above the pinned TX dock', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]')
    expect(pane, 'RTTY still has nowhere to write a hand-worked contact').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    expect(pane!.querySelector('.log-entry'), 'the frame is empty').not.toBeNull()

    // PSK/CW/Phone's placement: the strip scrolls inside the pane body, and the TX dock —
    // which carries this cockpit's Esc/Stop macro and the sequencer's Abort — stays pinned
    // BELOW it and outside every pane. A log strip that pushed the stop controls off the
    // bottom would be a stop-line defect.
    const log = pane!.querySelector('.le-log-btn') as HTMLElement
    expect(log, 'no Log button').not.toBeNull()
    expect(log.closest('.pane-body'), 'the strip is not inside the pane scroller').not.toBeNull()
    const dock = document.querySelector('.cockpit-txdock')!
    expect(dock.contains(log), 'the log strip is inside the TX dock').toBe(false)
    expect(pane!.compareDocumentPosition(dock) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()

    // No ✕: the pane has no id in RTTY's ⊞ vocabulary ({stream}), so it stays put — and it
    // hosts no stop control, so nothing about the stop line moves.
    expect(pane!.querySelector('.pane-head .cockpit-popout')).toBeNull()
    expect(pane!.querySelector('.rtty-arm')).toBeNull()
  })

  it('keeps the MEASURED fill role — a fill pane at weight 1.5, never a content strip', async () => {
    // ⚠️ A NUMBER FROM A REAL LAYOUT ENGINE, NOT A PREFERENCE — PSK's measurement, and this
    // is PSK's shell rule (`.layout.single.rtty-cockpit, .layout.single.psk-cockpit` share
    // one declaration, floor knob included). The strip needs 296 px of pane height to put its
    // Log button on screen; an even 1:1 split yields 292 px at 1920×1080 and misses by four
    // pixels. jsdom lays nothing out and cannot re-derive that — what it CAN check is that
    // the role and the share survive an edit. See PskCockpit.tsx for the full table.
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    expect(pane.getAttribute('data-fit'), 'the log strip became a content strip').toBe('fill')
    expect(pane.style.flex).toBe('var(--cockpit-pane-flex, 1.5 1 0)')
    // …and the stream beside it declares none, so the share is stated in ONE place.
    const stream = document.querySelector('[data-pane="stream"]') as HTMLElement
    expect(stream.style.flex).toBe('var(--cockpit-pane-flex, 1 1 0)')
    // Both consult the region-less shells' floor knob — the frame's inline min-height is what
    // makes `--cockpit-fill-min` reachable at all.
    expect(pane.style.minHeight).toBe('var(--cockpit-fill-min, 0)')
  })

  it('writes RTTY — the mode that was on the air', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    await act(async () => {
      fireEvent.change(pane.querySelector('.le-call') as HTMLInputElement, {
        target: { value: 'w1abc' },
      })
    })
    await act(async () => {
      fireEvent.click(pane.querySelector('.le-log-btn') as HTMLElement)
    })
    await waitFor(() => expect(logQso).toHaveBeenCalled())
    const rec = logQso.mock.calls[0][0]
    expect(rec.call).toBe('W1ABC')
    expect(rec.mode, 'an RTTY contact must not be logged as anything else').toBe('RTTY')
    expect(rec.band).toBe('20m')
    expect(rec.rstSent).toBe('599') // the digital report, like PSK and CW — not phone's 59
  })
})

describe('the RTTY cockpit works FIELD DAY (all-mode FD, digital class)', () => {
  /** Fill the FD strip's call/class/section and press Log. */
  async function logFd(call: string, cls: string, section: string) {
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    const codes = pane.querySelectorAll('.le-fd-input-code')
    await act(async () => {
      fireEvent.change(pane.querySelector('.le-fd-input-call') as HTMLInputElement, {
        target: { value: call },
      })
      fireEvent.change(codes[0] as HTMLInputElement, { target: { value: cls } })
      fireEvent.change(codes[1] as HTMLInputElement, { target: { value: section } })
    })
    await act(async () => {
      fireEvent.click(pane.querySelector('.le-fd-log-btn') as HTMLElement)
    })
  }

  it('logs to the CONTEST log — as DIG, stamped RTTY — and never twice', async () => {
    // Three things, each of which was a separate way to get this wrong:
    //   · the strip is in its FD layout, so the class/section exchange can be entered at all;
    //   · the scoring class is DIG (2 points, and dupes against the other digital modes);
    //   · the SUBMODE is RTTY. Without it the engine fills it from `current_submode`, which
    //     tracks the FT tier alone, and stamps an RTTY contact "FT8" — Cabrillo "DG" instead
    //     of "RY", the wrong ADIF mode, and a mode Winter Field Day bans outright on a QSO
    //     that was perfectly legal RTTY.
    await renderCockpit(fdSnap)
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    expect(pane.querySelector('.log-entry-fd'), 'the strip is not in its FD layout').not.toBeNull()
    expect((pane.querySelector('.le-fd-mode') as HTMLElement).textContent).toBe('DIG')

    await logFd('w1aw', '2a', 'ema')
    await waitFor(() => expect(fdLogManual).toHaveBeenCalled())
    expect(fdLogManual.mock.calls[0]).toEqual(['W1AW', '2A', 'EMA', 'DIG', 'RTTY'])
    // ONE LOG PER CONTACT. A Field Day contact belongs to the contest log alone — a copy in
    // the general logbook would also re-broadcast it on the WSJT-X sink and the upload queue.
    expect(logQso, 'the FD contact was double-logged as a casual QSO').not.toHaveBeenCalled()
  })

  it('master switch OFF is untouched: the casual strip and the general logbook', async () => {
    // The positive control for the gate. `snap.fieldDay` null (the master is off, or the
    // engine left FD mode) must leave this cockpit ordinary.
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    expect(pane.querySelector('.log-entry-fd'), 'FD layout with the master off').toBeNull()
    expect(pane.querySelector('.le-call'), 'no casual callsign field').not.toBeNull()
    expect(fdLogManual).not.toHaveBeenCalled()
  })
})

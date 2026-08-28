// @vitest-environment jsdom
//
// PSK HAS A LOG BUTTON (#159), AND IT WRITES THE RIGHT MODE.
//
// Three defects were reported as one, and this file is the rendered proof of all three:
//   (a) the cockpit imported no LogEntry, so there was no in-cockpit log path at all —
//       a PSK31 ragchew had to be written on paper and re-typed in the Logbook;
//   (b) `LOG_MODES` (the manual override's Mode picker) had no PSK entry, so even the
//       hand-entry escape hatch could not name what the operator was running;
//   (c) App's Logbook seed fell through to the FT tier for any rig mode that is not
//       phone/cw/rtty, and PSK's is `keyboard` — so a PSK contact hand-logged after the
//       cockpit visit came up pre-filled FT8. That third one is App-level and is pinned in
//       App.rigmode.test.ts's neighbourhood rather than here; (a) and (b) are rendered.
//
// The strip renders REAL here — that is the whole point, since the bug was a component that
// existed everywhere except this surface. PskCockpit.structure.test.tsx stubs it and owns
// the shell census instead.
//
// ⚠️ NOT MODELLED ON RTTY. RTTY looks like PSK's sibling and has no log strip either; it
// logs through its auto-sequencer, which is a different mechanism entirely. CW and Phone are
// the model, and this asserts the CW/Phone shape: the strip inside a framed pane, above the
// pinned TX dock, never inside it.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor, screen } from '@testing-library/react'
import { PskCockpit } from './PskCockpit'
import type { AppSnapshot, LoggedQso, PskState } from '../types'

const state: { current: PskState } = {
  current: {
    armed: true,
    afcHz: 0,
    signal: false,
    centerHz: 1000,
    text: 'CQ CQ de KD9TAW',
    charConf: [],
    sending: false,
    latched: false,
    keyerError: null,
  } as unknown as PskState,
}

const logQso = vi.fn(async (rec: LoggedQso) => rec)

vi.mock('../api', () => ({
  // PskCockpit's own surface
  getPskState: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  pskArm: vi.fn(async () => state.current),
  pskAutoArm: vi.fn(async () => state.current),
  pskClear: vi.fn(async () => state.current),
  pskAfcReset: vi.fn(async () => state.current),
  pskNet: vi.fn(async () => state.current),
  pskSend: vi.fn(async () => state.current),
  pskSetLatched: vi.fn(async () => state.current),
  pskSetMode: vi.fn(async (slug: string, reverse: boolean) => {
    state.current = { ...state.current, mode: slug, reverse } as PskState
    return state.current
  }),
  pskType: vi.fn(async () => state.current),
  pskStop: vi.fn(async () => state.current),
  setRfPower: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  atuTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  // …and the log strip's, which is the point of this file.
  logQso: (rec: LoggedQso) => logQso(rec),
  fdLogManual: vi.fn(async () => ({})),
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
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { modeIndicator?: ReactNode }) => (
    <header className="cockpit-header">{p.modeIndicator}</header>
  ),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  hunt: null,
  b4MatchMode: false,
  radio: {
    dialMhz: 14.07,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    txAllowed: true,
  },
} as unknown as AppSnapshot

async function renderCockpit() {
  const r = render(<PskCockpit snap={snap} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

/** Fill the call field and press Log. Returns the record handed to `logQso`. */
async function logACall(call: string): Promise<LoggedQso> {
  const pane = document.querySelector('[data-pane="log"]') as HTMLElement
  expect(pane, 'no log pane in the PSK cockpit').not.toBeNull()
  const callInput = pane.querySelector('.le-call') as HTMLInputElement
  expect(callInput, 'no callsign field in the log strip').not.toBeNull()
  await act(async () => {
    fireEvent.change(callInput, { target: { value: call } })
  })
  await act(async () => {
    fireEvent.click(pane.querySelector('.le-log-btn') as HTMLElement)
  })
  await waitFor(() => expect(logQso).toHaveBeenCalled())
  return logQso.mock.calls[logQso.mock.calls.length - 1][0]
}

beforeEach(() => {
  state.current = { ...state.current, mode: 'psk31', reverse: false } as PskState
  logQso.mockClear()
})
afterEach(cleanup)

describe('the PSK cockpit can log a contact (#159)', () => {
  it('renders the shared log strip in a framed pane, above the pinned TX dock', async () => {
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]')
    expect(pane, 'the log strip never reached this cockpit — the whole of #159(a)').not.toBeNull()
    expect(pane!.classList.contains('pane-frame')).toBe(true)
    // The strip itself, not just its frame.
    expect(pane!.querySelector('.log-entry'), 'the frame is empty').not.toBeNull()
    // The Log button, by the name the operator reads.
    const log = pane!.querySelector('.le-log-btn') as HTMLElement
    expect(log, 'no Log button').not.toBeNull()
    expect(log.textContent).toMatch(/log/i)

    // CW/Phone's placement: the strip scrolls inside the pane body, and the TX dock — which
    // carries this cockpit's Esc/Stop — stays pinned BELOW it and outside every pane. A log
    // strip that pushed the stop controls off the bottom would be a stop-line defect.
    expect(log.closest('.pane-body'), 'the strip is not inside the pane scroller').not.toBeNull()
    const dock = document.querySelector('.cockpit-txdock')!
    expect(dock.contains(log), 'the log strip is inside the TX dock').toBe(false)
    expect(pane!.compareDocumentPosition(dock) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()

    // The pane frame carries no ✕: it has no id in PSK's ⊞ vocabulary, so it stays put.
    expect(pane!.querySelector('.pane-head .cockpit-popout')).toBeNull()
  })

  it('keeps the MEASURED fill role — a fill pane at weight 1.5, never a content strip', async () => {
    // ⚠️ THIS IS A NUMBER FROM A REAL LAYOUT ENGINE, NOT A PREFERENCE, and jsdom cannot
    // re-derive it — it lays nothing out. Measured in headless Chrome against the shipped
    // sheet: the strip needs 296 px of pane height to put its Log button on screen; an even
    // 1:1 split yields 292 px at 1920×1080 and misses by four pixels, and `fit="content"`
    // (488 px with a recall card open) drives the SHELL onto its deficit valve in the
    // ordinary case. What jsdom CAN check is that the role and the share survive an edit,
    // which is the half that would otherwise be silently undone. See PskCockpit.tsx for the
    // full table of sizes.
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    expect(pane.getAttribute('data-fit'), 'the log strip became a content strip').toBe('fill')
    expect(pane.style.flex).toBe('var(--cockpit-pane-flex, 1.5 1 0)')
    // …and the stream beside it declares none, so the share is stated in ONE place.
    const stream = document.querySelector('[data-pane="stream"]') as HTMLElement
    expect(stream.style.flex).toBe('var(--cockpit-pane-flex, 1 1 0)')
    // Both consult the region-less shells' floor knob — the frame's inline min-height is
    // what makes `--cockpit-fill-min` reachable at all (a sheet rule on .pane-frame is
    // outranked by it, which is how the original spelling shipped dead).
    expect(pane.style.minHeight).toBe('var(--cockpit-fill-min, 0)')
  })

  it('writes PSK31 — not the FT tier, and not the rig sideband', async () => {
    await renderCockpit()
    const rec = await logACall('w1abc')
    expect(rec.call).toBe('W1ABC')
    expect(rec.mode, 'a PSK contact must not be logged as anything else').toBe('PSK31')
    expect(rec.band).toBe('20m')
    // The digital signal report, like RTTY and CW — not phone's 59.
    expect(rec.rstSent).toBe('599')
  })

  it('follows the SUB-MODE: a QPSK31 contact is logged as QPSK31', async () => {
    // The two are different waveforms on the air (QPSK31 carries the convolutional code),
    // and both are ADIF Mode values — folding one into the other would put a mode the
    // operator did not run into a permanent record.
    await renderCockpit()
    const select = screen.getByLabelText('PSK sub-mode') as HTMLSelectElement
    await act(async () => {
      fireEvent.change(select, { target: { value: 'qpsk31' } })
    })
    await waitFor(() =>
      expect((document.querySelector('.psk-mode-select') as HTMLSelectElement).value).toBe('qpsk31'),
    )
    const rec = await logACall('w1abc')
    expect(rec.mode).toBe('QPSK31')
  })

  it('fills the log callsign from the dock’s “their call” — WITHOUT moving the caret', async () => {
    // THE CONSTRAINT, TESTED, because it is the one that decides whether this ships at all:
    // the fill happens while the operator is typing, and the strip's OTHER machine-fill path
    // (`pendingWork`) focuses and selects the RST box on arrival. Doing that here would yank
    // the caret out of the compose bar mid-over. The fixture puts the caret exactly where it
    // must not move from — the compose bar — and asserts it is still there afterwards.
    await renderCockpit()
    const compose = document.querySelector('.cw-type') as HTMLInputElement
    const hisCall = document.querySelector('.rtty-hiscall') as HTMLInputElement
    const callBox = document.querySelector('.le-call') as HTMLInputElement

    compose.focus()
    expect(document.activeElement, 'fixture did not put the caret in the compose bar').toBe(compose)
    await act(async () => {
      fireEvent.change(hisCall, { target: { value: 'w1abc' } })
    })
    await waitFor(() => expect(callBox.value).toBe('W1ABC'))

    // The whole point.
    expect(document.activeElement, 'the fill stole focus from the compose bar').toBe(compose)
    // …and specifically not to RST, which is where `pendingWork` would have sent it.
    expect(document.activeElement).not.toBe(document.querySelector('.le-rst'))
  })

  it('does not clobber a call the operator typed straight into the log strip', async () => {
    // The dock field is empty and stays empty, so nothing settles and nothing fills. A fill
    // path that fired on mount would wipe a hand-typed call on every render.
    await renderCockpit()
    const callBox = document.querySelector('.le-call') as HTMLInputElement
    await act(async () => {
      fireEvent.change(callBox, { target: { value: 'k9xyz' } })
    })
    await new Promise((r) => setTimeout(r, 700)) // past the settle debounce
    expect(callBox.value).toBe('K9XYZ')
  })

  it('offers PSK31 and QPSK31 in the other-radio override’s Mode picker', async () => {
    // #159(b). The override is how a contact made on a rig Nexus does not control gets
    // logged, and its Mode list is a CLOSED ADIF enumeration — a mode missing from it
    // cannot be entered at all.
    await renderCockpit()
    const pane = document.querySelector('[data-pane="log"]') as HTMLElement
    await act(async () => {
      fireEvent.click(pane.querySelector('.le-override-toggle') as HTMLElement)
    })
    const modes = Array.from(
      (pane.querySelector('.le-ov-mode') as HTMLSelectElement).options,
      (o) => o.value,
    )
    expect(modes).toContain('PSK31')
    expect(modes).toContain('QPSK31')
    // BPSK31 is the same waveform under a logger's spelling and folds to PSK31 on import;
    // offering both would let one mode be logged under two names.
    expect(modes).not.toContain('BPSK31')
  })
})

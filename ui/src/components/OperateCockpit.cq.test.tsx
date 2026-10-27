// @vitest-environment jsdom
//
// #254 — "CQ KD9WES POTA" SILENTLY SENT A PLAIN CQ.
//
// The reporter typed his directed CQ with the words the other way round. `cqDirFromText`
// accepts only WSJT-X's grammar (`CQ [TOKEN] MYCALL [GRID4]`), so it returned `undefined`
// — and `doTx(6)` read that as "fall back to a plain CQ" and called `startCq(null)`. The
// Tx6 box still showed what he typed. The radio sent something else.
//
// THE INVARIANT, and it is the transmit path's: **never transmit a message different from
// the one shown.** The operator decision (2026-09-14) is WARN-ONLY — WSJT-X's CQ grammar is
// not widened and nothing on the air is rewritten. A malformed CQ must be refused visibly,
// before anything keys, with the valid form named where we can derive it honestly.
//
// Both directions are asserted, because a refusal that also refuses the valid form is the
// same defect wearing the other hat: the good text must still reach `startCq` byte for byte.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

const startCq = vi.fn(async () => null)
const startRunning = vi.fn(async (_dir?: string | null): Promise<null> => null)
const pushToast = vi.fn()

vi.mock('../api', () => ({
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  getSettings: vi.fn(async () => ({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
  startCq: (...a: unknown[]) => startCq(...(a as [])),
  startRunning: (...a: unknown[]) => startRunning(...(a as [string | null])),
  startQsoRecording: vi.fn(async () => null),
  stopQsoRecording: vi.fn(async () => null),
  setSkipTx1: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  setSatTransponder: vi.fn(async () => null),
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => null),
  postSpot: vi.fn(async () => null),
  setFrequency: vi.fn(async () => null),
  setRit: vi.fn(async () => null),
  setXit: vi.fn(async () => null),
  setVfo: vi.fn(async () => null),
  getSpectrumRow: vi.fn(async () => null),
  setDecodeDepth: vi.fn(async () => null),
  atuTune: vi.fn(async () => null),
  setMsk144Period: vi.fn(async () => null),
}))
vi.mock('../toast', () => ({
  pushToast: (...a: unknown[]) => pushToast(...(a as [])),
  subscribeToasts: () => () => {},
  dismissToast: vi.fn(),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

/** The reporter's own callsign, so the malformed text below is his verbatim. */
const MYCALL = 'KD9WES'

function makeSnap(): AppSnapshot {
  return {
    mycall: MYCALL,
    mygrid: 'EN61',
    stations: [],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 14.074,
      band: '20m',
      sideband: 'USB',
      slot: 0,
      source: 'native',
      sourceLabel: 'Native',
      nextSlotMs: 5000,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      txEven: true,
      txCycleAuto: true,
      txEnabled: false,
      txAllowed: true,
      transmitting: false,
      tuning: false,
      atu: true,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(): PanelLayoutApi<OperatePanelId> {
  const state: Partial<Record<OperatePanelId, PanelState>> = {}
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => panelStateIn(OPERATE_PANELS, { v: 1, state, share: {} }, id),
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  }
}

function renderCockpit(snap: AppSnapshot = makeSnap()) {
  return render(cockpit(snap))
}

function cockpit(snap: AppSnapshot) {
  const noop = () => {}
  return (
    <OperateCockpit
      snap={snap}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={noop}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      onClearSelection={noop}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi()}
      active
    />
  )
}

/** The Tx6 editable CQ box and its Tx 6 button. */
const cqField = () => screen.getByLabelText('Tx6 Call CQ (edit for a directed CQ)') as HTMLInputElement
const cqButton = () => screen.getByTitle('Call CQ (Alt+6)')

/** Type `text` into Tx6 and press Tx 6. */
function callCq(text: string) {
  fireEvent.change(cqField(), { target: { value: text } })
  expect(cqField().value, 'control: the edit took').toBe(text)
  fireEvent.click(cqButton())
}

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
  startCq.mockClear()
  startRunning.mockReset()
  startRunning.mockImplementation(async () => null)
  pushToast.mockClear()
})
afterEach(cleanup)

describe('#254 — a malformed directed CQ is refused, never quietly replaced', () => {
  it('"CQ KD9WES POTA" does not key, and says so', async () => {
    renderCockpit()
    callCq(`CQ ${MYCALL} POTA`)

    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    expect(
      startCq,
      'the words are in the wrong order — nothing may go on the air',
    ).not.toHaveBeenCalled()

    const [body, kind] = pushToast.mock.calls[0] as [string, string]
    expect(kind, 'a refusal is an error, not a note').toBe('error')
    expect(
      body,
      'the refusal names the form that would work, so the operator can retype it',
    ).toContain(`CQ POTA ${MYCALL}`)

    // …and the box still shows what the operator typed. Nothing was rewritten under them.
    expect(cqField().value).toBe(`CQ ${MYCALL} POTA`)
  })

  it('a CQ naming somebody else is refused too, with no suggestion invented', async () => {
    renderCockpit()
    callCq('CQ POTA W1ABC')

    await waitFor(() => expect(pushToast).toHaveBeenCalled())
    expect(startCq).not.toHaveBeenCalled()
    const [body] = pushToast.mock.calls[0] as [string]
    expect(
      body,
      'we must never propose a message the operator did not type',
    ).not.toContain('W1ABC')
  })

  // THE POSITIVE CONTROL. A guard that refuses everything is not a guard.
  it('the valid directed form still calls CQ, byte for byte', async () => {
    renderCockpit()
    callCq(`CQ POTA ${MYCALL}`)

    await waitFor(() => expect(startCq).toHaveBeenCalledTimes(1))
    expect(startCq).toHaveBeenCalledWith('POTA')
    expect(pushToast, 'a valid CQ raises nothing').not.toHaveBeenCalled()
  })

  it('a plain CQ still calls CQ', async () => {
    renderCockpit()
    callCq(`CQ ${MYCALL} EN61`)

    await waitFor(() => expect(startCq).toHaveBeenCalledTimes(1))
    expect(startCq).toHaveBeenCalledWith(null)
    expect(pushToast).not.toHaveBeenCalled()
  })

  // A CQ the station refuses says why, as the contest's run four lines above it always did: the
  // press used to drop it, so CQ from a computer driving a station did nothing and said nothing.
  it('a refused CQ shows the refusal', async () => {
    startCq.mockImplementationOnce(() => Promise.reject('Not connected to the station.'))
    renderCockpit()
    callCq(`CQ ${MYCALL} EN61`)

    await waitFor(() => expect(pushToast).toHaveBeenCalledWith('Not connected to the station.', 'error'))
  })
})

// THE STRIP'S CALL CQ IS THE CONTEST'S RUNNING WHILE A CONTEST RUNS. The engine decides that
// (`Engine::start_running`); on this computer the strip asks for it by its own verb — outside a
// contest the Tx6 row keeps `startCq`, which sends the CQ it shows — says why when it is refused,
// and shows the contest's run as its own: in a contest the snapshot carries it in `fieldDay`.
describe("the strip's Call CQ", () => {
  const roles = () => screen.getByRole('group', { name: 'Sequencer role' })
  const stripButton = (name: 'Call CQ' | 'S&P') => {
    const found = [...roles().querySelectorAll('button')].find((b) => b.textContent === name)
    if (!found) throw new Error(`no ${name} in the strip`)
    return found
  }
  const inContest = (running: boolean) =>
    ({ ...makeSnap(), mode: 'fieldDay', qso: null, fieldDay: { running, state: running ? 'CallingCq' : 'Listening', dxcall: null } }) as unknown as AppSnapshot

  it('asks for the run by its own verb, never the Tx6 one', async () => {
    renderCockpit()
    fireEvent.click(stripButton('Call CQ'))
    await waitFor(() => expect(startRunning).toHaveBeenCalledTimes(1))
    expect(startCq).not.toHaveBeenCalled()
  })

  it('says why when the run is refused', async () => {
    const why = 'Set your Field Day class and ARRL/RAC section in Settings first'
    startRunning.mockImplementation(async () => {
      throw why
    })
    renderCockpit()
    fireEvent.click(stripButton('Call CQ'))
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(why, 'error'))
  })

  it("lights Call CQ while the contest's run is on, and S&P while its S&P is", () => {
    renderCockpit(inContest(true))
    expect(stripButton('Call CQ').getAttribute('aria-pressed')).toBe('true')
    expect(stripButton('S&P').getAttribute('aria-pressed')).toBe('false')
    cleanup()
    renderCockpit(inContest(false))
    expect(stripButton('Call CQ').getAttribute('aria-pressed')).toBe('false')
    expect(stripButton('S&P').getAttribute('aria-pressed')).toBe('true')
  })
})

// WHILE A CONTEST RUNS, THE TX6 ROW IS THE CONTEST'S CQ, as WSJT-X's Tx6 is its activity's CQ. The
// engine names the CQ the contest's Running calls (`contestCq`); the row shows it, read-only, and
// Tx 6 asks for the contest's Running by the strip's own verb, so what goes on the air is what the
// row shows. The operator's own CQ, directed or plain, stays underneath and is back when the contest
// ends; on the Remote page the row never changes, as its Call CQ does not.
describe('the Tx6 row while a contest runs', () => {
  /** The Tx6 box, found by its place in the panel rather than by its label, which changes. */
  const tx6Box = () => document.querySelector('.txp-cq-field input') as HTMLInputElement
  const inContest = (snap: AppSnapshot = makeSnap()) =>
    ({ ...snap, contestCq: `CQ FD ${MYCALL} EN61` }) as AppSnapshot

  it("shows the contest's CQ, read-only, and calls the contest's Running", async () => {
    renderCockpit(inContest())
    expect(tx6Box().value).toBe(`CQ FD ${MYCALL} EN61`)
    expect(tx6Box().readOnly, 'what the row shows is what is sent, so it is not edited here').toBe(true)
    expect(tx6Box().getAttribute('aria-label')).toBe("Tx6 Call CQ (the contest's CQ)")
    expect(screen.getByText("The contest's CQ — Tx 6 calls it as Call CQ does")).toBeTruthy()
    fireEvent.click(cqButton())
    await waitFor(() => expect(startRunning).toHaveBeenCalledTimes(1))
    expect(startCq, 'never the plain CQ the row no longer shows').not.toHaveBeenCalled()
  })

  it('says why when the contest cannot start, and sends nothing else', async () => {
    const why = 'Set your Field Day class and ARRL/RAC section in Settings first'
    startRunning.mockImplementation(async () => {
      throw why
    })
    renderCockpit(inContest())
    fireEvent.click(cqButton())
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(why, 'error'))
    expect(startCq).not.toHaveBeenCalled()
  })

  it("keeps the operator's own CQ underneath and gives it back when the contest ends", async () => {
    const { rerender } = renderCockpit()
    fireEvent.change(tx6Box(), { target: { value: `CQ POTA ${MYCALL}` } })
    rerender(cockpit(inContest()))
    expect(tx6Box().value).toBe(`CQ FD ${MYCALL} EN61`)
    rerender(cockpit(makeSnap()))
    expect(tx6Box().value, 'the directed CQ is back').toBe(`CQ POTA ${MYCALL}`)
    expect(tx6Box().readOnly).toBe(false)
    fireEvent.click(cqButton())
    await waitFor(() => expect(startCq).toHaveBeenCalledWith('POTA'))
    expect(startRunning).not.toHaveBeenCalled()
  })
})

// WHERE WSJT-X HAS NO ACTIVITY FOR THE RUNNING CONTEST, TX6 IS THE OPERATOR'S OWN, as in WSJT-X's
// normal mode ("What I typed, like WSJT-X"): the row stays editable, and on this computer Tx 6 and
// Call CQ hand the direction typed there to the contest's Running (`contestCqTyped`), so the contact
// stays the contest's. A Tx6 that is no CQ is refused by both, as #254 refuses it.
describe('the Tx6 row in a contest WSJT-X has no activity for', () => {
  const tx6Box = () => document.querySelector('.txp-cq-field input') as HTMLInputElement
  const typedContest = () => ({ ...makeSnap(), contestCqTyped: true }) as AppSnapshot
  const callCqButton = () => {
    const roles = screen.getByRole('group', { name: 'Sequencer role' })
    const found = [...roles.querySelectorAll('button')].find((b) => b.textContent === 'Call CQ')
    if (!found) throw new Error('no Call CQ in the strip')
    return found
  }

  it("keeps the row editable and sends what was typed through the contest's Running", async () => {
    renderCockpit(typedContest())
    expect(tx6Box().readOnly).toBe(false)
    expect(tx6Box().value, 'the generated CQ, until edited').toBe(`CQ ${MYCALL} EN61`)
    callCq(`CQ QP ${MYCALL} EN61`)
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith('QP'))
    expect(startCq, 'the contact stays the contest’s').not.toHaveBeenCalled()
  })

  it('a plain Tx6 calls the Running with no direction', async () => {
    renderCockpit(typedContest())
    callCq(`CQ ${MYCALL} EN61`)
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null))
    expect(startCq).not.toHaveBeenCalled()
  })

  it('Call CQ calls the CQ typed in Tx6', async () => {
    renderCockpit(typedContest())
    fireEvent.change(tx6Box(), { target: { value: `CQ TEST ${MYCALL} EN61` } })
    fireEvent.click(callCqButton())
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith('TEST'))
  })

  it('a Tx6 that is no CQ is refused, by Tx 6 and by Call CQ alike', async () => {
    renderCockpit(typedContest())
    callCq(`CQ ${MYCALL} POTA`)
    await waitFor(() => expect(pushToast).toHaveBeenCalledTimes(1))
    fireEvent.click(callCqButton())
    await waitFor(() => expect(pushToast).toHaveBeenCalledTimes(2))
    expect(startRunning, 'nothing different from the row goes on the air').not.toHaveBeenCalled()
    expect(startCq).not.toHaveBeenCalled()
  })

  it('outside such a contest Call CQ stays plain, whatever Tx6 says', async () => {
    renderCockpit()
    fireEvent.change(tx6Box(), { target: { value: `CQ QP ${MYCALL} EN61` } })
    fireEvent.click(callCqButton())
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null))
  })
})

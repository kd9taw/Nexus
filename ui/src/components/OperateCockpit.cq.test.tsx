// @vitest-environment jsdom
//
// TX6 GOES OUT AS TYPED, AS WSJT-X'S DOES, until WSJT-X would write it again.
//
// WSJT-X 3.0.2 keys the text of its Tx6 box (`guiUpdate`, widgets/mainwindow.cpp 7801, 7904): a
// CQ with the grid typed, or free text, whatever packs. Editing the box keeps its second word as
// the CQ type (`on_tx6_editingFinished`, 10075-10086), and `genStdMsgs` (9566) writes the box
// again as that type, our call and our grid (`genCQMsg`, 9466) when a caller is answered (9444),
// on Clear DX (9830), on Generate Std Msgs (10505) and on a band or frequency change (12520,
// 13858). The row's text goes to the station whole; the station sends what the packer makes of
// it and refuses, with its reason, a CQ under another call (its tests).
//
// This replaces the refusal of any Tx6 outside WSJT-X's CQ grammar (#254, the operator's
// 2026-09-14 decision), which the operator reversed to match WSJT-X. The contest's typed CQ
// keeps that refusal: its Running takes only the direction.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

const startCq = vi.fn(async () => null)
const startRunning = vi.fn(async (_dir?: string | null, _text?: string): Promise<null> => null)
const pushToast = vi.fn()

vi.mock('../api', () => ({
  setDxBox: vi.fn(async () => undefined),
  qrzLookup: vi.fn(async () => null),
  resolveEntity: vi.fn(async () => null),
  getSettings: vi.fn(async () => ({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
  clearDxCall: vi.fn(async () => null),
  startCq: (...a: unknown[]) => startCq(...(a as [])),
  startRunning: (...a: unknown[]) => startRunning(...(a as [string | null, string | undefined])),
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

/** A fictional callsign standing in for the reporter's own; the malformed text is their
 *  report's word order. */
const MYCALL = 'W9ABC'

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

describe('Tx6 goes out as typed, as WSJT-X does', () => {
  it('a directed CQ with a typed grid reaches the station whole', async () => {
    renderCockpit()
    callCq(`CQ DX ${MYCALL} FN42`)

    await waitFor(() => expect(startCq).toHaveBeenCalledTimes(1))
    expect(startCq).toHaveBeenCalledWith(null, `CQ DX ${MYCALL} FN42`)
    expect(pushToast).not.toHaveBeenCalled()
  })

  it('a plain CQ reaches the station whole', async () => {
    renderCockpit()
    callCq(`CQ ${MYCALL} EN61`)

    await waitFor(() => expect(startCq).toHaveBeenCalledTimes(1))
    expect(startCq).toHaveBeenCalledWith(null, `CQ ${MYCALL} EN61`)
    expect(pushToast).not.toHaveBeenCalled()
  })

  it('a row that is no CQ goes to the station too, as WSJT-X sends it as free text', async () => {
    renderCockpit()
    callCq(`CQ ${MYCALL} POTA`)
    await waitFor(() => expect(startCq).toHaveBeenCalledWith(null, `CQ ${MYCALL} POTA`))
    callCq('TNX QSO 73')
    await waitFor(() => expect(startCq).toHaveBeenCalledWith(null, 'TNX QSO 73'))
    expect(pushToast).not.toHaveBeenCalled()
  })

  it("a CQ under another call is the station's to refuse, and its reason is shown", async () => {
    const why = 'Tx 6 calls CQ as another station'
    startCq.mockImplementationOnce(() => Promise.reject(why))
    renderCockpit()
    callCq('CQ POTA W1ABC')
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith(why, 'error'))
  })
})

describe("Tx6 is written again at WSJT-X's points, its CQ word kept", () => {
  const edited = `CQ DX ${MYCALL} FN42`
  const regenerated = `CQ DX ${MYCALL} EN61`

  it('Generate Std Msgs (mainwindow.cpp 10505)', () => {
    renderCockpit()
    fireEvent.change(cqField(), { target: { value: edited } })
    fireEvent.click(screen.getByText('Generate Std Msgs'))
    expect(cqField().value).toBe(regenerated)
  })

  it('Clear DX (9830)', () => {
    renderCockpit()
    fireEvent.change(cqField(), { target: { value: edited } })
    fireEvent.click(screen.getByText('Clear'))
    expect(cqField().value).toBe(regenerated)
  })

  it('a station answered (processMessage, 9444)', () => {
    const { rerender } = renderCockpit()
    fireEvent.change(cqField(), { target: { value: edited } })
    rerender(
      cockpit({
        ...makeSnap(),
        qso: { dxcall: 'K1XYZ', dxgrid: 'FN42', state: 'AwaitReport', cqRunning: true } as unknown as AppSnapshot['qso'],
      }),
    )
    expect(cqField().value).toBe(regenerated)
  })

  it('a band change (12520)', () => {
    const { rerender } = renderCockpit({ ...makeSnap(), bandChangeTick: 1 } as AppSnapshot)
    fireEvent.change(cqField(), { target: { value: edited } })
    rerender(cockpit({ ...makeSnap(), bandChangeTick: 2 } as AppSnapshot))
    expect(cqField().value).toBe(regenerated)
  })

  it('a frequency change, not while keyed (12520, 13858)', () => {
    const snap = makeSnap()
    const at = (dialMhz: number, transmitting: boolean) =>
      cockpit({ ...snap, radio: { ...snap.radio, dialMhz, transmitting } })
    const { rerender } = render(at(14.074, false))
    fireEvent.change(cqField(), { target: { value: edited } })
    rerender(at(14.076, true))
    expect(cqField().value, 'keyed: left alone').toBe(edited)
    rerender(at(14.076, false))
    expect(cqField().value).toBe(regenerated)
  })

  it("free text's second word is its CQ word, as WSJT-X reads it (10082)", () => {
    renderCockpit()
    fireEvent.change(cqField(), { target: { value: 'TNX QSO 73' } })
    fireEvent.click(screen.getByText('Generate Std Msgs'))
    expect(cqField().value).toBe(`CQ QSO ${MYCALL} EN61`)
  })

  it('a plain CQ typed makes the CQ plain again', () => {
    renderCockpit()
    fireEvent.change(cqField(), { target: { value: edited } })
    fireEvent.change(cqField(), { target: { value: `CQ ${MYCALL} FN42` } })
    fireEvent.click(screen.getByText('Generate Std Msgs'))
    expect(cqField().value).toBe(`CQ ${MYCALL} EN61`)
  })
})

describe('the station refuses a CQ', () => {
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

  // Calling CQ IS sending Tx6 in WSJT-X (mainwindow.cpp 7801): the row's text goes with it.
  it('sends Tx6, as generated or as typed', async () => {
    renderCockpit()
    fireEvent.click(stripButton('Call CQ'))
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null, `CQ ${MYCALL} EN61`))
    fireEvent.change(cqField(), { target: { value: `CQ DX ${MYCALL} FN42` } })
    fireEvent.click(stripButton('Call CQ'))
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null, `CQ DX ${MYCALL} FN42`))
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
    await waitFor(() => expect(startCq).toHaveBeenCalledWith(null, `CQ POTA ${MYCALL}`))
    expect(startRunning).not.toHaveBeenCalled()
  })
})

// WHERE WSJT-X HAS NO ACTIVITY FOR THE RUNNING CONTEST, TX6 IS THE OPERATOR'S OWN, as in WSJT-X's
// normal mode ("What I typed, like WSJT-X"): the row stays editable, and on this computer Tx 6 and
// Call CQ hand the row's text to the contest's Running (`contestCqTyped`), which sends it as typed,
// as WSJT-X keys its Tx6 there, so the contact stays the contest's.
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
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null, `CQ QP ${MYCALL} EN61`))
    expect(startCq, 'the contact stays the contest’s').not.toHaveBeenCalled()
  })

  it('a plain Tx6 goes to the Running as typed', async () => {
    renderCockpit(typedContest())
    callCq(`CQ ${MYCALL} EN61`)
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null, `CQ ${MYCALL} EN61`))
    expect(startCq).not.toHaveBeenCalled()
  })

  it('Call CQ calls the CQ typed in Tx6', async () => {
    renderCockpit(typedContest())
    fireEvent.change(tx6Box(), { target: { value: `CQ TEST ${MYCALL} EN61` } })
    fireEvent.click(callCqButton())
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null, `CQ TEST ${MYCALL} EN61`))
  })

  it('a Tx6 that is no CQ goes to the Running too, by Tx 6 and by Call CQ alike', async () => {
    renderCockpit(typedContest())
    callCq(`CQ ${MYCALL} POTA`)
    await waitFor(() => expect(startRunning).toHaveBeenCalledTimes(1))
    fireEvent.click(callCqButton())
    await waitFor(() => expect(startRunning).toHaveBeenCalledTimes(2))
    expect(startRunning).toHaveBeenLastCalledWith(null, `CQ ${MYCALL} POTA`)
    expect(startCq).not.toHaveBeenCalled()
    expect(pushToast).not.toHaveBeenCalled()
  })

  it('outside such a contest Call CQ sends Tx6 too', async () => {
    renderCockpit()
    fireEvent.change(tx6Box(), { target: { value: `CQ QP ${MYCALL} EN61` } })
    fireEvent.click(callCqButton())
    await waitFor(() => expect(startRunning).toHaveBeenCalledWith(null, `CQ QP ${MYCALL} EN61`))
  })
})

// A LOGGER'S REPLY TO A CQ, WORKED BY THE CONTEST'S S&P (a field report). GridTracker's
// click on a station whose latest line is a CQ is a WSJT-X Reply, and while a contest runs whose own
// FT8/FT4 messages apply the engine hands that station to the contest's S&P, as a double-click on the
// CQ does. The snapshot then carries no `qso`: the contest's sequencer is in `fieldDay`, and the over
// it sends in `contestTxNow`. With no click on this screen the strip read "TX — listening" and Idle,
// and DX Call and Tx1–Tx5 stayed blank, while the radio called the station every period. WSJT-X shows the
// station it calls in its DX Call box and the message in its Tx panel (`processMessage`).
describe("a logger's Reply worked by the contest's S&P", () => {
  const worked = () =>
    ({
      ...makeSnap(),
      mode: 'fieldDay',
      qso: null,
      fieldDay: { running: false, state: 'AwaitExchange', dxcall: 'W9XYZ' },
      contestTxNow: `W9XYZ ${MYCALL} EN61`,
    }) as unknown as AppSnapshot
  const text = (sel: string) => document.querySelector(sel)?.textContent ?? null

  it('shows the station, the state and the over the contest sends', () => {
    renderCockpit(worked())
    expect(text('.cockpit-ftstrip .cq-dx')).toBe('W9XYZ')
    expect(text('.cockpit-ftstrip .cq-state')).toBe('AwaitExchange')
    expect(text('.cockpit-ftstrip .cq-now-msg')).toBe(`W9XYZ ${MYCALL} EN61`)
    expect(
      (screen.getByTitle('Re-arm and re-send this message') as HTMLButtonElement).disabled,
      "Resend re-arms the QSO sequencer's step, which the contest's has none of",
    ).toBe(true)
  })

  it("leaves Log QSO off: the contest logs its own contact as it closes", () => {
    renderCockpit(worked())
    const log = document.querySelector('.cockpit-ftstrip .cq-log') as HTMLButtonElement
    expect(log.disabled).toBe(true)
    cleanup()
    renderCockpit({ ...worked(), logQsoWaiting: 1 } as AppSnapshot)
    const waiting = document.querySelector('.cockpit-ftstrip .cq-log') as HTMLButtonElement
    expect(waiting.disabled, 'CONTROL: a contact waiting for Log QSO keeps it live').toBe(false)
  })

  it('fills DX Call, and Tx 1 is the over on the air', () => {
    renderCockpit(worked())
    expect((screen.getByLabelText('DX callsign') as HTMLInputElement).value).toBe('W9XYZ')
    const rows = Array.from(document.querySelectorAll('.tx-panel .txp-row'))
    expect(rows).toHaveLength(6)
    expect(rows[0].textContent).toContain(`W9XYZ ${MYCALL} EN61`)
    expect(rows.findIndex((r) => r.querySelector('.txp-dot.on')) + 1, 'the dot on Tx 1').toBe(1)
  })

  it("keeps the grid a click put beside the same station, and clears another station's", () => {
    const view = renderCockpit()
    const call = () => screen.getByLabelText('DX callsign') as HTMLInputElement
    const grid = () => screen.getByLabelText('DX grid locator') as HTMLInputElement
    // The operator's own click on W9XYZ's CQ fills both boxes before the engine answers.
    fireEvent.change(call(), { target: { value: 'W9XYZ' } })
    fireEvent.change(grid(), { target: { value: 'EM12' } })
    view.rerender(cockpit(worked()))
    expect(grid().value, "the contest's sequencer names no grid: the click's stays").toBe('EM12')
    const next = worked()
    view.rerender(cockpit({ ...next, fieldDay: { ...next.fieldDay, dxcall: 'K1ABC' } } as AppSnapshot))
    expect(call().value).toBe('K1ABC')
    expect(grid().value, "W9XYZ's grid is not K1ABC's").toBe('')
  })

  it('outside a contest nothing new shows: the strip is listening', () => {
    renderCockpit()
    expect(text('.cockpit-ftstrip .cq-dx')).toBe(null)
    expect(text('.cockpit-ftstrip .cq-state')).toBe('Idle')
    expect(text('.cockpit-ftstrip .cq-now-msg')).toBe('— listening')
  })
})

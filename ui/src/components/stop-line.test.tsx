// @vitest-environment jsdom
//
// THE STOP LINE, COMPUTED — the wiring half of the rule in features/panelState.ts.
//
//   THE OPERATOR MUST NEVER BE UNABLE TO STOP A TRANSMISSION.
//
// Mechanically: in every cockpit, at least one control that stops a transmission renders
// OUTSIDE every ⊞-removable pane. This file is what computes that half of it.
//
// panelState.test.ts holds the NAME half: no vocabulary may contain an id named for a stop
// control. That half reads names and nothing else, so it walks straight past a stop control
// gated on an id called `dsp` — which is exactly the hole the old rule's "enforced by
// computation" claim papered over.
//
// THIS suite reads wiring and ignores names. For each cockpit it drives EVERY id in the
// REAL vocabulary through the REAL hide path — one at a time, then all at once, the state
// an operator reaches by unticking down the menu — and looks for the stop controls by their
// ACCESSIBLE NAME. If any panel id gates any of them, in any combination, this goes red and
// says which id took which control.
//
// WHAT BELONGS IN A CASE'S `stopControls`, AND WHAT MUST NOT. The list is the
// OUTSIDE-EVERY-PANE set: the controls the guarantee rests on. A stop control that lives
// INSIDE a ⊞-removable pane is deliberately absent, and there are two of them in the app —
// Phone's voice keyer hosts ■ Stop (→ stopVoice → Engine::stop_voice, which flushes the
// output ring and unkeys), and RTTY's `stream` pane hosts the "Auto on" toggle (off-click →
// seq.abort() + Engine::rtty_stop(): queue cleared, rig unkeyed). Both go away with their
// pane, and that is allowed: a pane's own stop is a CONVENIENCE built on the guarantee, never
// what holds it up. Adding either one below would make this sweep demand its pane be
// unhideable — which is exactly how the FIRST wording of the rule excluded the voice keyer,
// the pane it was written to admit. Do not "fix" a red by unhiding a pane; check first
// whether the control you added belongs on the list at all.
//
// EACH COCKPIT IS RENDERED WITH THE PROPS APP GIVES IT, and that is load-bearing rather than
// tidiness: CockpitHeader draws the TX-enable latch (▼ TX On / ■ TX Off) only when it is
// handed `onSetTxEnabled`, which App passes to RTTY and SSTV and to nothing else — those two
// cockpits have no other Enable-Tx affordance, the TopBar's being hidden with the digital
// chrome. The first version of this file omitted the prop, so for RTTY and SSTV the latch was
// never in the document and the sweep proved nothing about the one control the rule names
// BY NAME (gating it on a panel id in either cockpit was green). Phone and CW arm elsewhere
// and legitimately have no latch on screen; their `stopControls` say so by not listing one.
//
// WHAT THIS FILE DOES NOT CARE ABOUT: whether a pane can START a transmission. Six can —
// Operate's Tx messages, its two decode panes and its two rosters, Phone's voice keyer — and
// all of them are hideable, correctly. The rule is about what is left ON SCREEN and nothing
// else, so the only lists here are `stopControls`.
//
// WHY THE HEADER IS NOT STUBBED HERE. Every *.structure.test.tsx mocks CockpitHeader down
// to an empty <header>, which is right for a shell census and useless for this: Stop TX and
// Tune live INSIDE that header in Phone, CW and RTTY, so a stubbed header can only prove a
// container rendered. This file pays the mock cost to render the real one, so "Stop TX is
// reachable" is an assertion about the button the operator presses.
//
// Operate is swept in OperateCockpit.structure.test.tsx instead ("every protected control
// renders INSIDE .cockpit-qso with every panel id removed") — its stop controls are in the
// merged QSO strip rather than a CockpitHeader, and that suite already owns the mock
// surface for them. It is driven off OPERATE_PANEL_IDS for the same reason as here.
// IT IS NOT THIS SWEEP'S EQUIVALENT, and it is not described as one: it takes no baseline,
// compares no `disabled` state and hides every id at once rather than one at a time. It
// catches a control that VANISHES; it would not catch one left mounted-and-disabled.
//
// EACH LIST BELOW IS A SUBSET OF ITS COCKPIT'S CENSUS, NOT A COPY OF IT. panelState.ts once
// claimed "each cockpit's sweep list is the same set, which is how this is checkable in
// minutes"; that was false for four of the five swept cockpits. Two kinds of holder cannot be
// swept here, by construction rather than by oversight — this file finds BUTTONS BY ACCESSIBLE
// NAME, in one fixed fixture state:
//   · KEYBOARD-ONLY. Phone's Space bar (window keyup → setPtt(false), only while Lock is off)
//     and CW's Esc (window keydown → the same abort() Stop TX calls) have no accessible name
//     and no element. Operate's Esc is the same, in its own sweep.
//   · CONDITIONALLY RENDERED. RTTY's auto-sequencer Abort renders only inside
//     `{auto && seqState !== 'idle'}`, and the `rttyState` fixture above is auto:false /
//     seqState:'idle' — so there is nothing on screen to look for. Adding a second RTTY case
//     with an in-flight sequence would sweep it; not done here, and not claimed.
// SSTV is the one cockpit whose list and census match exactly.
//
// WHAT THIS DOES NOT COMPUTE, stated rather than guarded:
//   · A STOP CONTROL THAT IS PRESENT, ENABLED AND INERT. Every assertion here is about the
//     button being in the document and operable — never about what pressing it does.
//     Verified: `onClick={() => {}}` on CockpitHeader's Stop TX passes this file and the
//     whole suite. Nothing below would go red.
//   · The keyboard-only and conditionally rendered holders above.
//   · That a NEWLY ADDED stop control was added to the list below. Adding one is a human
//     step. Each cockpit's list is its case's `stopControls`, so the next person editing a
//     dock finds it beside the cockpit it guards.
// What IS computed is that every vocabulary in the app has a sweep at all — the last test
// in the file, driven off ALL_PANEL_VOCABULARIES, so a sixth cockpit cannot ship without one.
//
// THE FIELD DAY COCKPIT IS SWEPT AT THE BOTTOM OF THIS FILE, IN A BLOCK OF ITS OWN, because it
// has NO ⊞ vocabulary at all — the `CASES` machinery above is driven by panel ids and there are
// none to drive it with, and ALL_PANEL_VOCABULARIES cannot notice a cockpit that never enters it.
// Its analogue of "hide every id" is "reach every state the cockpit reaches on its own": the
// position's MODE CLASS, which swaps the whole dock strip (the PH class's PTT row is a stop
// control that simply is not drawn on a CW or digital position). That block also computes the
// thing this file's header calls a human step — that the fixture passes THE PROPS APP PASSES —
// by reading `<FdCockpit …/>` out of App.tsx and comparing the names. The omitted-prop blinding
// that cost RTTY and SSTV their latch is the one failure a stop-line sweep cannot survive.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { render, screen, cleanup, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import { SstvView } from './SstvView'
import { FdCockpit } from './FdCockpit'
import {
  ALL_PANEL_VOCABULARIES,
  PHONE_PANEL_IDS,
  CW_PANEL_IDS,
  RTTY_PANEL_IDS,
  PSK_PANEL_IDS,
  SSTV_PANEL_IDS,
} from '../features/panelState'
import type { PanelLayoutApi } from '../features/panelState'
import type { AppSnapshot, FieldDayStatus, PskState, RttyState, SstvState } from '../types'

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: ['CQ CQ DE KD9TAW K'],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

const rttyState = {
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
} as unknown as RttyState

const pskState = {
  armed: true,
  afcHz: 0,
  signal: false,
  centerHz: 1000,
  text: 'CQ CQ de KD9TAW',
  charConf: [],
  sending: false,
  latched: false,
  keyerError: null,
} as unknown as PskState

const sstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: {
    armed: false,
    audioPeak: 0,
    lastAudioUnix: null,
    drains: 0,
    visSeen: 0,
    lastVisUnix: null,
    unknownVis: 0,
    lastUnknownVisCode: null,
    lastUnknownVisUnix: null,
    images: 0,
    lastImageUnix: null,
  },
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
} as unknown as SstvState

// One api mock for four cockpits — the union of what they call on mount.
vi.mock('../api', async (importOriginal) => {
  // ⭐ DERIVED FROM THE REAL MODULE, not a hand-kept list. A hand-kept mock omits any export
  // added after it was written, and a component that calls one THROWS ON MOUNT — so the suite
  // goes red at a seam nothing in the diff explains, and the tempting fix is to make the test
  // pass rather than ask why. That cost five files one evening when a single API call was added
  // to the CW cockpit, this one among them.
  //
  // Every function the module exports is auto-stubbed here; the entries below override only the
  // ones this file's assertions actually depend on, so their shapes are unchanged.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // Hand-kept mock: an export CwCockpit calls but this list omits makes it THROW ON MOUNT,
    // which reads as a behaviour regression rather than the stale mock it actually is.
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    setPtt: vi.fn(async () => {}),
    setRfPower: vi.fn(async () => {}),
    setMicGain: vi.fn(async () => {}),
    setNrLevel: vi.fn(async () => {}),
    setAgc: vi.fn(async () => ({})),
    setScopeSpan: vi.fn(async () => ({})),
    setScopeRef: vi.fn(async () => {}),
    setFlexPanSpan: vi.fn(async () => ({})),
    setFlexPanRef: vi.fn(async () => ({})),
    startQsoRecording: vi.fn(async () => ({})),
    stopQsoRecording: vi.fn(async () => ({})),
    setTune: vi.fn(async () => ({})),
    haltTx: vi.fn(async () => ({})),
    setFrequency: vi.fn(async () => ({})),
    setSplit: vi.fn(async () => ({})),
    setRigFunc: vi.fn(async () => ({})),
    setSidebandOverride: vi.fn(async () => ({})),
    setFilterWidth: vi.fn(async () => ({})),
    openPanelWindow: vi.fn(async () => {}),
    getVoiceMessages: vi.fn(async () => []),
    playVoiceMessage: vi.fn(async () => ({})),
    stopVoice: vi.fn(async () => ({})),
    startVoiceRecording: vi.fn(async () => ({})),
    stopVoiceRecording: vi.fn(async () => []),
    cancelVoiceRecording: vi.fn(async () => ({})),
    clearVoiceMessage: vi.fn(async () => []),
    importVoiceMessage: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    setSettings: vi.fn(async () => ({})),
    sendCw: vi.fn(async () => {}),
    setCwKeyer: vi.fn(async () => null),
    setCwWpm: vi.fn(async () => {}),
    stopCw: vi.fn(async () => {}),
    cwDecode: vi.fn(async () => decodeState),
    cwClear: vi.fn(async () => {}),
    setAiCw: vi.fn(async () => {}),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    pointRotatorAtCall: vi.fn(async () => 0),
    // The real CockpitHeader hosts RotorStrip, which polls these on mount.
    readRotator: vi.fn(async () => null),
    stopRotator: vi.fn(async () => ({})),
    getDeclination: vi.fn(async () => 0),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    setSatTransponder: vi.fn(async () => {}),
    stopSatTrack: vi.fn(async () => ({})),
    getRttyState: vi.fn(async () => rttyState),
    getLicensedBandPlan: vi.fn(async () => []),
    rttyArm: vi.fn(async () => rttyState),
    // `rtty_auto_arm` fires on the rising edge of `active`; a hand-kept mock must carry it or
    // the cockpit throws on mount.
    rttyAutoArm: vi.fn(async () => rttyState),
    rttySend: vi.fn(async () => rttyState),
    rttyStop: vi.fn(async () => rttyState),
    rttyClear: vi.fn(async () => rttyState),
    rttyAfcReset: vi.fn(async () => rttyState),
    rttyNet: vi.fn(async () => rttyState),
    rttySetAuto: vi.fn(async () => rttyState),
    rttyAutoCq: vi.fn(async () => rttyState),
    rttyAutoAnswer: vi.fn(async () => rttyState),
    rttyAutoAbort: vi.fn(async () => rttyState),
    getPskState: vi.fn(async () => pskState),
    pskArm: vi.fn(async () => pskState),
    pskAutoArm: vi.fn(async () => pskState),
    pskClear: vi.fn(async () => pskState),
    pskAfcReset: vi.fn(async () => pskState),
    pskNet: vi.fn(async () => pskState),
    pskSend: vi.fn(async () => pskState),
    pskSetLatched: vi.fn(async () => pskState),
    pskType: vi.fn(async () => pskState),
    pskStop: vi.fn(async () => pskState),
    getSstvState: vi.fn(async () => sstvState),
    sstvArm: vi.fn(async () => sstvState),
    sstvAutoArm: vi.fn(async () => sstvState),
    sstvSend: vi.fn(async () => sstvState),
    sstvStop: vi.fn(async () => sstvState),
    setOperatingMode: vi.fn(async () => ({})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas/scope children only. CockpitHeader is DELIBERATELY REAL — see the file header.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function panelsWith<P extends string>(removed: readonly P[]): PanelLayoutApi<P> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

const radio = {
  dialMhz: 14.2,
  band: '20m',
  catOk: true,
  sideband: 'USB',
  sidebandOverride: null,
  rigMode: 'USB',
  transmitting: false,
  tuning: false,
  txEnabled: true,
  txAllowed: true,
  qsoRecording: false,
  rfPower: null,
  micGain: null,
  nrLevel: 0.3,
  agc: 'fast',
  nb: true,
  nr: true,
  notch: null,
  comp: null,
  vox: null,
  filterWidthHz: 500,
  splitTxMhz: null,
  smeterDb: null,
  cwWpm: 22,
  cwKeyer: 'cat',
  phoneSegLo: null,
  phoneSegHi: null,
}
const snap = { mycall: 'KD9TAW', radio } as unknown as AppSnapshot

/**
 * One cockpit's stop-line case. `stopControls` are accessible-name matchers for the controls
 * that END a transmission AND RENDER OUTSIDE EVERY ⊞-REMOVABLE PANE — the set the guarantee
 * rests on, not every stop control on the screen. See the file header: a pane-resident stop
 * (Phone's ■ Stop, RTTY's Auto toggle) must stay off these lists.
 */
interface Case<P extends string> {
  cockpit: string
  /** The vocabulary's own `view` id — how the coverage check below matches a case to a
   *  vocabulary, so neither list can drift out from under the other. */
  view: string
  ids: readonly P[]
  stopControls: Array<[label: string, name: RegExp]>
  render: (panels: PanelLayoutApi<P>) => void
}

const phone: Case<(typeof PHONE_PANEL_IDS)[number]> = {
  cockpit: 'Phone',
  view: 'phone',
  ids: PHONE_PANEL_IDS,
  // NOT LISTED, DELIBERATELY: the voice keyer's ■ Stop (→ stopVoice → Engine::stop_voice,
  // which flushes the output ring and unkeys). It lives inside the `voiceKeyer` pane and goes
  // away with it — a convenience, not what the guarantee rests on. Listing it here would make
  // this sweep forbid the very ⊞ entry this batch was about.
  stopControls: [
    // The alternation is the PTT button's FULL label set, not a sample of it — the sweep
    // finds this control by accessible name and nothing else, so a label the button can
    // render and this regex cannot match is a hole the sweep reports as a missing control.
    // Four states since #81: the two live ones, the licence lock, and TX-switched-off
    // (PhoneCockpit.txoff.test.tsx owns what each one SAYS; this only has to find it).
    ['PTT', /push to talk|on air — release to stop|tx locked|tx off — click to enable/i],
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
  // ⚠️ `fieldDay` IS PASSED, and it is not decoration. App passes it whenever the master switch
  // is on, and it swaps this cockpit's log strip for the Field Day one — a different strip,
  // with different focus behaviour. Sweeping only the off-event shape left the FD shape of a
  // SHIPPED cockpit unswept for the whole of the weekend it exists for, which is how a mount
  // autofocus added for another cockpit reached this one and disarmed its Space PTT.
  render: (panels) =>
    render(
      <PhoneCockpit
        snap={snap}
        theme="dark"
        onWorkSpot={() => {}}
        spots={[]}
        panels={panels}
        fieldDay={fdStatus}
      />,
    ),
}

const cw: Case<(typeof CW_PANEL_IDS)[number]> = {
  cockpit: 'CW',
  view: 'cw',
  ids: CW_PANEL_IDS,
  stopControls: [
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
  // `fieldDay` for the same reason as Phone's case above: the FD log strip is a different
  // strip, and this cockpit is one of the two that host it during the event.
  render: (panels) =>
    render(
      <CwCockpit
        snap={snap}
        theme="dark"
        onWorkSpot={() => {}}
        spots={[]}
        panels={panels}
        fieldDay={fdStatus}
      />,
    ),
}

/** The TX-enable latch as CockpitHeader labels it. `radio.txEnabled` is true and
 *  `transmitting` false in this fixture, so it reads "▼ TX On"; the disarmed face is matched
 *  too, so a fixture flip cannot make the sweep silently stop finding the control.
 *
 *  IT IS A STOP CONTROL IN THESE TWO COCKPITS AND NOWHERE ELSE, which is why it appears only
 *  in the RTTY and SSTV cases. `set_tx_enabled(false)` clears rtty_queue + arms rtty_abort
 *  (engine.rs ~7120) and drops sstv_tx + arms sstv_abort (~7124); tempo-audio/service.rs turns
 *  either into flush + rig.ptt(false) while an over is in flight. It deliberately does NOT arm
 *  `slot_tx_abort`, so in Operate the same handler lets the FT over complete — that is the
 *  operator's 2026-07-31 ruling, and it is why Operate's TX On/Off is NOT on any stop list.
 *  CockpitHeader draws the latch as a BUTTON only while `radio.transmitting` is false; that
 *  flag is the slot-TX indicator alone (RTTY/SSTV report through rtty_sending/sstv_sending),
 *  so it is still a button through every RTTY and SSTV over. */
const TX_LATCH: [string, RegExp] = ['TX-enable latch', /^▼ tx on$|^■ tx off$/i]

const rtty: Case<(typeof RTTY_PANEL_IDS)[number]> = {
  cockpit: 'RTTY',
  view: 'rtty',
  ids: RTTY_PANEL_IDS,
  // The dock's abort is labelled by its CONTENT, not its title, and the two spans abut with
  // no whitespace, so the accessible name is "EscStop" — hence \s* rather than a space.
  //
  // NOT LISTED, DELIBERATELY: the "Auto on" toggle, which lives INSIDE the `stream` pane and
  // whose off-click is rttySetAuto(false) → seq.abort() + Engine::rtty_stop() (queue cleared,
  // rig unkeyed). It is a real stop control and it goes away when `stream` is hidden — the
  // second of the app's two pane-resident stops, and the reason the fourth wording of the rule
  // was falsified. Listing it here would demand that RTTY's ONLY ⊞ entry be unhideable.
  //
  // ALSO NOT LISTED, for a different reason: the auto-sequencer's Esc/Abort (dock, → seq.abort()
  // + Engine::rtty_stop()). It IS one of RTTY's census holders and it has no ⊞ id, but it
  // renders only inside `{auto && seqState !== 'idle'}` and the fixture above is idle, so
  // listing it would fail the baseline assertion ("not on screen with every panel SHOWN")
  // rather than prove anything. Census-only, said so in panelState.ts.
  //
  // AND NOT THE CONTINUOUS-TX ("TX") BUTTON, for the plainest reason of all: it is a SENDER.
  // Clicking it off stops accepting characters and lets what was already typed finish keying —
  // a mode toggle, deliberately not an immediate cut. The immediate cuts for a latched over are
  // the two swept below plus the TX-enable latch, and each of them drops the latch as well as
  // the over. Its `disabled={!(sending || latched)}` sibling — the Esc/Stop macro — is swept,
  // and that predicate is what keeps it live in the tick between the latch going up and the
  // first chunk being keyed; `RttyCockpit.test.tsx` pins that case directly, because this sweep
  // renders a fixture that is neither sending nor latched and so can only see the baseline.
  stopControls: [
    ['Stop TX', /^stop tx$/i],
    ['Stop (RTTY abort)', /^esc\s*stop$/i],
    ['Tune', /^tune$|^tuning…$/i],
    TX_LATCH,
  ],
  // onSetTxEnabled exactly as App passes it (the .rtty-host block). Without it CockpitHeader
  // renders a display-only pill and the latch is not on screen to sweep.
  render: (panels) => render(<RttyCockpit snap={snap} panels={panels} onSetTxEnabled={() => {}} />),
}

const psk: Case<(typeof PSK_PANEL_IDS)[number]> = {
  cockpit: 'PSK',
  view: 'psk',
  ids: PSK_PANEL_IDS,
  // RTTY's dock shape, PSK's instantiation (Keyboard Modes Phase 2). The dock's
  // abort is labelled by its content spans with no whitespace, hence \s*. The
  // continuous-TX ("TX") button is a SENDER, not a stop — same ruling as
  // RTTY's, same reason — and must never be added here.
  stopControls: [
    ['Stop TX', /^stop tx$/i],
    ['Stop (PSK abort)', /^esc\s*stop$/i],
    // Tune stops the carrier it started, exactly as it does in Phone, CW, Operate and
    // RTTY. It arrived in this header with the drive control it exists to set (PSK's
    // one operating hazard is overdrive), and it is swept here the day it arrived.
    ['Tune', /^tune$|^tuning…$/i],
    TX_LATCH,
  ],
  // onSetTxEnabled exactly as App passes it (the .psk-host block). Without it
  // CockpitHeader renders a display-only pill and the latch is not on screen
  // to sweep — the documented blindness this file's header records.
  render: (panels) => render(<PskCockpit snap={snap} panels={panels} onSetTxEnabled={() => {}} />),
}

const sstv: Case<(typeof SSTV_PANEL_IDS)[number]> = {
  cockpit: 'SSTV',
  view: 'sstv',
  ids: SSTV_PANEL_IDS,
  stopControls: [['Stop', /^stop$/i], TX_LATCH],
  // Same as App's .sstv-host block.
  render: (panels) => render(<SstvView snap={snap} panels={panels} onSetTxEnabled={() => {}} />),
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const CASES: Array<Case<any>> = [phone, cw, rtty, psk, sstv]

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('the stop line, computed against the real cockpits', () => {
  it.each(CASES.map((c) => [c.cockpit, c] as const))(
    '%s: no ⊞ panel id gates any control that stops a transmission',
    async (_name, c) => {
      // "Still there" is not enough — a control gated `disabled={!shown('x')}` is mounted
      // and useless — so record what each stop control looks like with NOTHING hidden and
      // require the hides to leave it alone. (Several of these are legitimately disabled
      // when idle: RTTY's and SSTV's Stop are dead until `sending`. That is a property of
      // the transmitter, not of the ⊞ menu, which is exactly why the baseline is the
      // comparison rather than `disabled === false`.)
      const found = (name: RegExp) => screen.queryAllByRole('button', { name })
      // Explicit <string>: an empty literal would infer `never` and fail the case's own
      // PanelLayoutApi<P>.
      c.render(panelsWith<string>([]))
      await settle()
      const baseline = new Map(
        c.stopControls.map(([label, name]) => {
          const els = found(name) as HTMLButtonElement[]
          expect(
            els.length,
            `${c.cockpit}: "${label}" is not on screen with every panel SHOWN — the sweep ` +
              'below would then be asserting nothing at all',
          ).toBeGreaterThan(0)
          return [label, els.some((e) => !e.disabled)]
        }),
      )
      cleanup()

      // One id at a time, then the whole vocabulary at once. Both matter: a control might
      // survive every single hide and still vanish when two panes go, because the cockpit
      // collapses a column that happens to host it.
      const combos: Array<readonly string[]> = [...c.ids.map((id: string) => [id]), [...c.ids]]
      for (const removed of combos) {
        c.render(panelsWith(removed))
        await settle()
        for (const [label, name] of c.stopControls) {
          const els = found(name) as HTMLButtonElement[]
          expect(
            els.length,
            `${c.cockpit}: hiding {${removed.join(', ')}} took "${label}" with it — the ` +
              'operator can hide a way to stop a transmission',
          ).toBeGreaterThan(0)
          expect(
            els.some((e) => !e.disabled),
            `${c.cockpit}: hiding {${removed.join(', ')}} left "${label}" on screen but ` +
              'DISABLED — mounted and unusable is the same loss as gone',
          ).toBe(baseline.get(label))
        }
        cleanup()
      }
    },
  )

  it('EVERY vocabulary in the app is swept — here, or in a file named here', () => {
    // A sweep is worth only what it covers, and the failure this whole batch came from was
    // a guard that looked exhaustive and silently skipped a cockpit. So the coverage is
    // computed against ALL_PANEL_VOCABULARIES rather than asserted about this file's own
    // list: add a sixth cockpit and this goes red naming it, whatever anybody remembers.
    //
    // ELSEWHERE is the honest part — one vocabulary is swept in another file, and it has
    // to be declared here to count.
    // The declared sweep for `operate` is PRESENCE-ONLY (no baseline, no disabled
    // comparison, no one-id-at-a-time pass) — weaker than the cases above, and named that
    // way here so nobody reads this map as "every cockpit gets the same sweep".
    const ELSEWHERE: Record<string, string> = {
      operate:
        'OperateCockpit.structure.test.tsx — "every protected control renders INSIDE ' +
        '.cockpit-qso with every panel id removed" (PRESENCE-ONLY: all ids at once, no ' +
        'baseline, no disabled comparison)',
    }
    const here = new Set(CASES.map((c) => c.view))
    for (const vocab of ALL_PANEL_VOCABULARIES) {
      expect(
        here.has(vocab.view) || vocab.view in ELSEWHERE,
        `the "${vocab.view}" cockpit has a ⊞ vocabulary and no rendered stop-line sweep — ` +
          'add a case above, or sweep it in its own structure test and name that file in ' +
          'ELSEWHERE here',
      ).toBe(true)
    }
    // …and the cases above really do drive the real vocabularies, not copies of them.
    for (const c of CASES) {
      const vocab = ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)
      expect(vocab, `no vocabulary named "${c.view}"`).toBeDefined()
      expect([...c.ids], `${c.cockpit} sweeps a stale id list`).toEqual([...vocab!.panelIds])
    }
  })
})

// ── THE FIELD DAY COCKPIT — the same rule, computed without a vocabulary ─────────────────
//
// FdCockpit has no ⊞ menu, no `usePanelLayout` and no panel ids, so the sweep above cannot
// reach it: `panelsWith([...ids])` has nothing to hide and ALL_PANEL_VOCABULARIES never names
// it. The guarantee is identical all the same — THE OPERATOR MUST NEVER BE UNABLE TO STOP A
// TRANSMISSION — so it is computed here against the two axes this cockpit really has:
//
//   · THE MODE CLASS. It is derived from the rig (`fdModeClassFromRig`) and it swaps the whole
//     dock strip. On a PH position the strip is the PTT row; on CW it is the macro buttons and
//     on DIG a note. So the PH class has one more stop control than the other two, and each
//     class is swept for its OWN list rather than for a union that would be wrong twice.
//   · THE PROPS APP PASSES. Read out of App.tsx below, not retyped — see `appProps`.
//
// NOT LISTED, and for the reasons the cases above give: Esc (window keydown → the same
// `abort()` Stop TX calls) and the Space-bar PTT release are KEYBOARD-ONLY, so they have no
// accessible name and no element for this file to find; `FdCockpit.structure.test.tsx` drives
// Esc directly, including from inside the callsign field. Nothing in this cockpit lives inside
// a removable pane, because nothing in it is removable.
const fdRadio = {
  dialMhz: 14.25,
  band: '20m',
  catOk: true,
  sideband: 'USB',
  sidebandOverride: null,
  rigMode: 'USB',
  operatingMode: 'phone',
  transmitting: false,
  tuning: false,
  txEnabled: true,
  txAllowed: true,
  slot: 0,
  rxOffsetHz: 1500,
  cwWpm: 22,
  rfPower: null,
  micGain: null,
  smeterDb: null,
  splitTxMhz: null,
}

function fdSnap(rigMode: string): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    harqRescues: 0,
    recentDecodes: [],
    radio: { ...fdRadio, rigMode },
  } as unknown as AppSnapshot
}

const fdStatus = {
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: 'running',
  qsoCount: 12,
  sections: 4,
  points: 24,
  workedSections: ['WI', 'EMA'],
  log: [{ call: 'W1AW', band: '20m', mode: 'PH', class: '3A', section: 'EMA', whenUnix: 100 }],
} as unknown as FieldDayStatus

/**
 * EXACTLY THE PROPS App's `case 'fieldDay'` passes, and the test below proves it by reading
 * that JSX rather than by anyone remembering. This is the file's own named blindness — RTTY
 * and SSTV were swept for a year with `onSetTxEnabled` omitted, so the TX-enable latch was
 * never in the document and gating it on a panel id was green — turned into a computation.
 */
const appProps = {
  snap: fdSnap('USB'),
  onSnap: () => {},
  fieldDay: fdStatus,
  onSetMode: () => {},
  fdRuleset: null,
  tier: 'FT8' as const,
  onOpenDashboard: () => {},
  onOpenOperate: () => {},
}

/** Vite rewrites a LITERAL `new URL('…', import.meta.url)` into a served asset URL, and
 *  `fileURLToPath` then throws. A named const is read at runtime instead. */
const APP_SRC = '../App.tsx'

/** Per mode class: the stop controls that must be on screen, by accessible name. */
const FD_STOP: Record<string, Array<[label: string, name: RegExp]>> = {
  // The rig is on USB, so the position is PH and the dock draws the PTT row. Its full label
  // set, lifted from Phone's case above because the row itself was lifted from Phone.
  USB: [
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
    ['PTT', /push to talk|on air — release to stop|tx locked|tx off — click to enable/i],
  ],
  // CW and a data sub-mode: no PTT row exists to find, and claiming one would fail the
  // baseline rather than prove anything.
  CW: [
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
  PKTUSB: [
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
}

describe('the stop line in the Field Day cockpit, which has no ⊞ vocabulary to sweep', () => {
  it.each(Object.keys(FD_STOP))(
    'rigMode %s: every stop control is on screen, enabled, and outside every pane',
    async (rigMode) => {
      render(<FdCockpit {...appProps} snap={fdSnap(rigMode)} />)
      await settle()
      const region = document.querySelector('.cockpit-panes')
      expect(region, 'the cockpit did not render its pane region').not.toBeNull()
      for (const [label, name] of FD_STOP[rigMode]) {
        const els = screen.queryAllByRole('button', { name }) as HTMLButtonElement[]
        expect(
          els.length,
          `Field Day (${rigMode}): "${label}" is not on screen — the operator has no way to ` +
            'stop a transmission from this position',
        ).toBeGreaterThan(0)
        expect(
          els.some((e) => !e.disabled),
          `Field Day (${rigMode}): "${label}" is on screen but DISABLED — mounted and ` +
            'unusable is the same loss as gone',
        ).toBe(true)
        for (const el of els) {
          expect(
            el.closest('.pane-frame'),
            `Field Day (${rigMode}): "${label}" renders INSIDE a pane frame — the one place ` +
              'a stop control may not live',
          ).toBeNull()
          expect(
            region!.contains(el),
            `Field Day (${rigMode}): "${label}" renders inside the pane region`,
          ).toBe(false)
        }
      }
      cleanup()
    },
  )

  it('Field Day OFF the event: the stop line holds with no status block at all', () => {
    // `snap.fieldDay` is `Some` only inside `Mode::FieldDay`, and the cockpit is reachable
    // while it is `None` (the operator picked the cockpit face, the engine has not answered
    // yet, or the master switch just went off). The name guard above compares prop NAMES; this
    // is the one VALUE axis that changes what renders — no exchange, no boards data, no rate.
    render(<FdCockpit {...appProps} fieldDay={null} />)
    for (const [label, name] of FD_STOP.USB) {
      const els = screen.queryAllByRole('button', { name }) as HTMLButtonElement[]
      expect(els.length, `Field Day (no status): "${label}" is not on screen`).toBeGreaterThan(0)
      expect(
        els.some((e) => !e.disabled),
        `Field Day (no status): "${label}" is on screen but DISABLED`,
      ).toBe(true)
    }
    cleanup()
  })

  it('is rendered with the props App really passes — read out of App.tsx, not retyped', () => {
    // THE HUMAN STEP, COMPUTED. A prop App passes and this fixture omits can silently take a
    // control off the screen, and the sweep above would then pass while asserting nothing —
    // which is exactly what happened to RTTY and SSTV. Set equality, so it fails in both
    // directions: a prop added to App is unswept until it is added here, and a prop dropped
    // from App leaves a stale entry that fails too.
    const src = readFileSync(fileURLToPath(new URL(APP_SRC, import.meta.url)), 'utf8')
    const open = src.indexOf('<FdCockpit')
    expect(open, 'App.tsx no longer renders <FdCockpit — this guard is reading nothing').toBeGreaterThan(-1)
    const jsx = src.slice(open, src.indexOf('/>', open))
    const inApp = [...jsx.matchAll(/^\s+([a-zA-Z][\w]*)=/gm)].map((m) => m[1]).sort()
    expect(inApp.length, 'no props parsed out of App.tsx — the extractor is broken').toBeGreaterThan(0)
    expect(inApp).toEqual(Object.keys(appProps).sort())
  })
})

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
// tidiness: the TX strip (CockpitTxStrip) draws the TX-enable latch as a button (TX On / TX Off)
// only when it is handed `onSetTxEnabled`, which App passes to RTTY, PSK, SSTV and JS8 — in JS8
// the latch is
// NOT a stop (slotted mode), so the JS8 case passes the prop for parity with App and lists
// Stop TX + Tune only. RTTY and SSTV have no other Enable-Tx affordance, the TopBar's being
// hidden with the digital chrome. The first version of this file omitted the prop, so for
// RTTY and SSTV the latch was never in the document and the sweep proved nothing about the
// one control the rule names BY NAME (gating it on a panel id in either cockpit was green).
// Phone and CW arm elsewhere and legitimately have no latch on screen; their `stopControls`
// say so by not listing one. The same holds for their two feeds (`spots`, `needed`): each
// renders only with the board App lends it, so both cases lend one — without it the two ids
// were in the sweep and their panes never on screen, and hiding them proved nothing.
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
//     and Esc have no accessible name and no element. Esc is a stop on every operating screen
//     and on Satellites (CW's: window keydown → the same abort() Stop TX calls; App binds
//     Tempo's, Phone's, SSTV's, APRS's and Satellites'); each is NAMED in the last block of this
//     file and pressed in stop-control-wiring.test.tsx.
//   · CONDITIONALLY RENDERED. RTTY's auto-sequencer Abort renders only inside
//     `{auto && seqState !== 'idle'}`, and the `rttyState` fixture above is auto:false /
//     seqState:'idle' — so there is nothing on screen to look for. Adding a second RTTY case
//     with an in-flight sequence would sweep it; not done here, and not claimed.
// SSTV is the one cockpit whose list and census match exactly, Esc (keyboard-only) apart.
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
// FIELD DAY HAS NO SWEEP OF ITS OWN, AND THAT IS NOT AN OMISSION. Field Day is a MODE the app
// enters, not a screen: the contacts are made in the five cockpits swept below, whose stop
// lines are the same on the event weekend as off it. The Field Day section itself draws no
// transmit control at all — it is setup, score, sections, bonuses, the log and the club board.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import * as api from '../api'
import { sectionFeatures } from '../features/registry'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import { Js8Cockpit } from './Js8Cockpit'
import { SstvView } from './SstvView'
import {
  ALL_PANEL_VOCABULARIES,
  PHONE_PANEL_IDS,
  CW_PANEL_IDS,
  RTTY_PANEL_IDS,
  PSK_PANEL_IDS,
  JS8_PANEL_IDS,
  SSTV_PANEL_IDS,
} from '../features/panelState'
import type { PanelLayoutApi } from '../features/panelState'
import { arrangeIds, coerceLeftSide, coercePlacement, moveArranged, movePane, type Arrangement, type ArrangeSpec, type PaneMove, type PanePlacement } from '../features/panelPlace'
import type { AppSnapshot, FieldDayStatus, Js8State, PskState, RttyState, SstvState } from '../types'

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

const js8State = {
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: true,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  cqOn: false,
  cqNextAtMs: null,
  cqIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
} as unknown as Js8State

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
    getJs8State: vi.fn(async () => js8State),
    // The JS8 roster joins against the logbook (features/callHistory) for its ✓/Name/
    // Comment columns; the auto-stub's `{}` is not a log this sweep can render against.
    // `js8_enter` fires on the rising edge of `active`; the auto-stub would answer `{}` and
    // the cockpit would then render a state with no `armed` — pin the fixture.
    js8Enter: vi.fn(async () => js8State),
    js8Arm: vi.fn(async () => js8State),
    js8Send: vi.fn(async () => js8State),
    js8SendCommand: vi.fn(async () => js8State),
    js8CallCq: vi.fn(async () => js8State),
    js8SetSpeed: vi.fn(async () => js8State),
    js8DropQueue: vi.fn(async () => js8State),
    js8InboxMark: vi.fn(async () => js8State),
    js8InboxDelete: vi.fn(async () => js8State),
    setTxLevel: vi.fn(async () => ({})),
    setRxOffset: vi.fn(async () => ({})),
    setTxOffset: vi.fn(async () => ({})),
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

function panelsWith<P extends string>(removed: readonly P[], place?: PanePlacement<P>, leftSide?: P[]): PanelLayoutApi<P> {
  return {
    layout: place || leftSide ? { v: 2, state: {}, share: {}, ...(place ? { place } : {}), ...(leftSide ? { leftSide } : {}) } : { v: 1, state: {}, share: {} },
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

/** Field Day switched ON, as App hands it to the cockpits during an event — the state that
 *  swaps their log strip for the FD one. See the Phone case's own note on why it is swept. */
const fdStatus = {
  composing: [
    { key: 'CLASS', raw: '3A' },
    { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
  ],
  running: true,
  state: 'running',
  qsoCount: 12,
  sections: 4,
  points: 24,
  workedSections: ['WI', 'EMA'],
  log: [{ call: 'W1AW', band: '20m', mode: 'PH', class: '3A', section: 'EMA', whenUnix: 100 }],
} as unknown as FieldDayStatus

/** The Spots and Needed boards as App lends them to Phone and CW (#345, plan H8), empty. */
const spotsBoard = { bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} }
const neededBoard = { alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} }

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
        spotsBoard={spotsBoard}
        neededBoard={neededBoard}
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
        spotsBoard={spotsBoard}
        neededBoard={neededBoard}
      />,
    ),
}

/** The TX-enable latch as the TX strip labels it — FT's words since 2026-10-01 (it was
 *  "▼ TX On" / "■ TX Off" in the cockpit header). `radio.txEnabled` is true and `transmitting`
 *  false in this fixture, so it reads "TX On"; the disarmed face is matched
 *  too, so a fixture flip cannot make the sweep silently stop finding the control.
 *
 *  IT IS A STOP CONTROL IN THESE TWO COCKPITS AND NOWHERE ELSE, which is why it appears only
 *  in the RTTY and SSTV cases. `set_tx_enabled(false)` clears rtty_queue + arms rtty_abort
 *  (engine.rs ~7120) and drops sstv_tx + arms sstv_abort (~7124); tempo-audio/service.rs turns
 *  either into flush + rig.ptt(false) while an over is in flight. It deliberately does NOT arm
 *  `slot_tx_abort`, so in Operate the same handler lets the FT over complete — that is the
 *  operator's 2026-07-31 ruling, and it is why Operate's TX On/Off is NOT on any stop list.
 *  The strip draws the latch as a BUTTON only while `radio.transmitting` is false; that
 *  flag is the slot-TX indicator alone (RTTY/SSTV report through rtty_sending/sstv_sending),
 *  so it is still a button through every RTTY and SSTV over. */
const TX_LATCH: [string, RegExp] = ['TX-enable latch', /^tx on$|^tx off$/i]

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

const js8: Case<(typeof JS8_PANEL_IDS)[number]> = {
  cockpit: 'JS8',
  view: 'js8',
  ids: JS8_PANEL_IDS,
  // The OPERATE shape of the census, because JS8 is a SLOTTED mode: Stop TX (→ halt_tx, which
  // also empties the JS8 queue from B7 on) and Tune, both in the header. NOT LISTED, on
  // purpose: the TX-enable latch — `set_tx_enabled(false)` deliberately does not arm
  // slot_tx_abort (the operator's 2026-07-31 ruling: the frame in flight completes), so in a
  // slotted mode it is not a stop; and the dock's "Drop queue", a SENDER-class control (the
  // RTTY "TX" ruling) that empties the queue without cutting anything. Esc is keyboard-only
  // and census-only. `onSetTxEnabled` is still passed, exactly as App passes it.
  stopControls: [
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
  render: (panels) => render(<Js8Cockpit snap={snap} panels={panels} onSetTxEnabled={() => {}} />),
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
/** The same station with a SECOND RECEIVER Nexus can command (an IC-7610 on its own CI-V
 *  daemon): Phone and CW then draw a SUB row and a MAIN plate. Neither carries an id or a stop
 *  control, and these two cases hold that to the same sweep — hiding every id, singly and all
 *  at once, with the SUB row on screen, leaves every stop control where it was. */
const dualSnap = {
  mycall: 'KD9TAW',
  radio: {
    ...radio,
    receivers: {
      main: { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' } },
      sub: { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' } },
      subCapability: 'present',
      subCommandable: true,
    },
  },
} as unknown as AppSnapshot

const phoneDual: Case<(typeof PHONE_PANEL_IDS)[number]> = {
  ...phone,
  cockpit: 'Phone with a Sub receiver',
  render: (panels) =>
    render(
      <PhoneCockpit
        snap={dualSnap}
        theme="dark"
        onWorkSpot={() => {}}
        spots={[]}
        panels={panels}
        fieldDay={fdStatus}
        spotsBoard={spotsBoard}
        neededBoard={neededBoard}
      />,
    ),
}
const cwDual: Case<(typeof CW_PANEL_IDS)[number]> = {
  ...cw,
  cockpit: 'CW with a Sub receiver',
  render: (panels) =>
    render(
      <CwCockpit
        snap={dualSnap}
        theme="dark"
        onWorkSpot={() => {}}
        spots={[]}
        panels={panels}
        fieldDay={fdStatus}
        spotsBoard={spotsBoard}
        neededBoard={neededBoard}
      />,
    ),
}

const CASES: Array<Case<any>> = [phone, cw, rtty, psk, js8, sstv, phoneDual, cwDual]

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

/** Each stop control on the screen, by label: the buttons `screen.queryAllByRole('button', { name })`
 *  finds for its name, in the same order. That query works out the accessible name of every button
 *  on the screen before it matches one, so a query per control worked out the same names once per
 *  control. Here one query works them out once, and each control's name is matched against them. */
function stopsOnScreen(controls: Array<[label: string, name: RegExp]>): Map<string, HTMLButtonElement[]> {
  const names = new Map<Element, string>()
  const all = screen.queryAllByRole('button', {
    name: (accessible, el) => {
      names.set(el, accessible)
      return controls.some(([, name]) => name.test(accessible))
    },
  }) as HTMLButtonElement[]
  return new Map(controls.map(([label, name]) => [label, all.filter((e) => name.test(names.get(e)!))]))
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
      // Explicit <string>: an empty literal would infer `never` and fail the case's own
      // PanelLayoutApi<P>.
      c.render(panelsWith<string>([]))
      await settle()
      const shown = stopsOnScreen(c.stopControls)
      const baseline = new Map(
        c.stopControls.map(([label]) => {
          const els = shown.get(label)!
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
        const on = stopsOnScreen(c.stopControls)
        for (const [label] of c.stopControls) {
          const els = on.get(label)!
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
    // A budget for real work (2026-10-02): eleven fresh mounts for Phone, each followed by one
    // accessible-name pass over every button, and Phone, the first case, also pays the file's
    // first render. It took 0.55 s alone, 2.6 s in the full suite, 3.0 s at a fifth of a CPU
    // and 5.9 s at a tenth, past the 5 s default.
    20_000,
  )

  it('the two Sub-receiver cases really draw a SUB row — else they would sweep a copy of their twin', async () => {
    for (const c of [phoneDual, cwDual] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      expect(document.querySelector('[data-receiver="sub"]'), `${c.cockpit}: no SUB row`).not.toBeNull()
      cleanup()
    }
  })

  it('Phone’s and CW’s two feeds are on screen with nothing hidden — else hiding them would sweep nothing', async () => {
    for (const c of [phone, cw, phoneDual, cwDual] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      for (const id of ['spots', 'needed']) {
        expect(c.ids, `${c.cockpit}: "${id}" left the vocabulary`).toContain(id)
        expect(document.querySelector(`[data-pane="${id}"]`), `${c.cockpit}: the ${id} pane is not on screen`).not.toBeNull()
      }
      cleanup()
    }
  })

  it('the RF scope pane is on screen with nothing hidden in RTTY, PSK, JS8 and SSTV — else hiding it would sweep nothing', async () => {
    // It SHIPS HIDDEN (defaultRemoved), and every sweep above starts from "nothing hidden", so it is
    // on screen at their baseline and every hide of it — singly and with everything else — is a real
    // one. Operate's twin is in OperateCockpit.structure.test.tsx.
    for (const c of [rtty, psk, js8, sstv] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      expect(c.ids, `${c.cockpit}: "rfScope" left the vocabulary`).toContain('rfScope')
      expect(document.querySelector('[data-pane="rfScope"]'), `${c.cockpit}: the RF scope pane is not on screen`).not.toBeNull()
      cleanup()
      c.render(panelsWith<string>(['rfScope']))
      await settle()
      expect(document.querySelector('[data-pane="rfScope"]'), `${c.cockpit}: hidden, the RF scope pane is still on screen`).toBeNull()
      cleanup()
    }
  })

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
      // Connect has a ⊞ vocabulary (its seven slots) and NO transmit control of any kind, so
      // there is no stop to lose — what is swept is the property that remains meaningful:
      // closing a pane, singly and all at once, leaves every control outside the panes on
      // screen (the map toolbar, the header).
      // ⚠️ CONNECT IS THE STOP LINE'S ONE RULED EXCEPTION. App draws no top bar there, so there is
      // no Stop TX on Connect at all — the operator, 2026-10-01: "remove all radio control from
      // connect, reclaim that space". Transmit on Connect is stopped by Esc or by leaving the
      // screen; stop-control-wiring.test.tsx holds Esc to halt_tx on Connect and every OTHER screen
      // to the bar. Nothing here is loosened for any other cockpit or screen by it.
      connect:
        'ConnectView.panes.test.tsx — "hiding every pane leaves every control outside the ' +
        'panes on screen" (Connect renders no transmit control; PRESENCE-ONLY, by name); and ' +
        'the ruled exception ("remove all radio control from connect, reclaim that space"): no top ' +
        'bar on Connect, Esc sends halt_tx there — stop-control-wiring.test.tsx',
      // The dashboard rail's four slots. The rail is a sibling of the cockpit in App's shell and
      // renders no transmit control; what is swept is that it costs no cockpit a stop control:
      // with the rail on, every control on each cockpit's list below is on screen, no more disabled
      // than with it off, and not inside the rail — in the real App, every operating cockpit.
      dashrail:
        'DashRail.stopLine.test.tsx — every operating cockpit\'s stop-line list, rail off vs on ' +
        '(PRESENCE + DISABLED, by name, not layout); DashRail.test.tsx — no transmit control in the ' +
        'rail, every box in the registry placed in it',
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

// ── THE ARRANGEMENT SWEEP (layout L3) ──────────────────────────────────────────────────────────
// ⊞ Arrange can put a cockpit's panes in any column and any order, so the stop line has to hold under
// every arrangement too, not only the stock one: for each cockpit whose vocabulary ARRANGES, 50 random
// valid placements (seeded — a red names a reproducible placement), each with every id hidden singly
// and all at once, every listed stop control on screen by accessible name and no more disabled than
// with nothing hidden in the stock arrangement. Half the placements are built the way the menu builds
// them (random moves); half are hand-edited junk pushed through the record's own coercion, which is
// the path a stored or foreign record takes. What makes this hold is structural — the controls are in
// the header and the dock, which no placement can reach, and a stop control has no id to place — so
// this is the computation of that, and it proves the placements reached the region (a sweep that
// rendered the stock grouping 50 times would prove nothing).
function rng(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** `n` placements of `spec`: menu-built (random moves) and coerced junk, alternately. */
function randomPlacements<P extends string>(spec: ArrangeSpec<P>, n: number, seed: number): Array<PanePlacement<P>> {
  const next = rng(seed)
  const ids = arrangeIds(spec)
  const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
  const out: Array<PanePlacement<P>> = []
  while (out.length < n) {
    if (out.length % 2 === 0) {
      let place: PanePlacement<P> | undefined
      for (let k = 1 + Math.floor(next() * 14); k > 0; k--) {
        place = movePane(spec, place, undefined, pick(ids), pick(['up', 'down', 'left', 'right'] as PaneMove[]), () => true) ?? place
      }
      if (place) out.push(place)
    } else {
      const raw: Record<string, unknown> = {}
      for (const id of [...ids, 'ptt', 'stopTx', 'scope', 'txmeters']) {
        if (next() < 0.7) raw[id] = { col: pick(['a', 'b', 'log', 'c']), order: Math.floor(next() * 9) - 2 }
      }
      const place = coercePlacement(spec, raw)
      if (place) out.push(place)
    }
  }
  return out
}

describe('THE ARRANGEMENT SWEEP: no placement of the panes gates a control that stops a transmission', () => {
  // One case per arranging vocabulary (the first; the Sub-receiver twins change no placement, and
  // each pass is 550 renders).
  const ARRANGING = CASES.filter(
    (c, i) => ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)?.arrange && CASES.findIndex((d) => d.view === c.view) === i,
  )

  it('some cockpit arranges, so this sweep is reading something', () => {
    expect(ARRANGING.map((c) => c.view)).toContain('phone')
  })

  it.each(ARRANGING.map((c) => [c.cockpit, c] as const))(
    '%s: 50 random placements, every id hidden singly and all at once, every stop control where it was',
    async (_name, c) => {
      const spec = ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)!.arrange! as ArrangeSpec<string>
      const order = () => [...document.querySelectorAll('.cockpit-panes .pane-frame')].map((f) => f.getAttribute('data-pane'))
      c.render(panelsWith<string>([]))
      await settle()
      const stock = order()
      const shown = stopsOnScreen(c.stopControls)
      const baseline = new Map(c.stopControls.map(([label]) => [label, shown.get(label)!.some((e) => !e.disabled)]))
      cleanup()
      let differs = 0
      const placements = randomPlacements(spec, 50, 20260929)
      expect(placements.length).toBe(50)
      for (const [i, place] of placements.entries()) {
        const combos: Array<readonly string[]> = [[], ...c.ids.map((id: string) => [id]), [...c.ids]]
        for (const removed of combos) {
          c.render(panelsWith(removed, place))
          await settle()
          if (removed.length === 0 && order().join() !== stock.join()) differs++
          const on = stopsOnScreen(c.stopControls)
          for (const [label] of c.stopControls) {
            const els = on.get(label)!
            const where = `${c.cockpit}, placement #${i} ${JSON.stringify(place)}, hiding {${removed.join(', ')}}`
            expect(els.length, `${where} took "${label}" with it`).toBeGreaterThan(0)
            expect(els.some((e) => !e.disabled), `${where} left "${label}" on screen but DISABLED`).toBe(baseline.get(label))
          }
          cleanup()
        }
      }
      // The placements reached the region: most of them render in an order the stock one does not.
      expect(differs, `${c.cockpit}: the random placements left the region in its stock order — the sweep is reading nothing`).toBeGreaterThan(20)
    },
    // A budget for real work (2026-10-02): 550 fresh mounts a pass, each followed by one
    // accessible-name pass over every button, and the time grows in step with the CPU share.
    // Phone took 15 s alone, 57 s in the full suite, 83 s at a fifth of a CPU and 160 s at a
    // tenth; CW 17 s alone and 90 s at a fifth. Only one of Phone's 50 placements repeats, so
    // there is no repeated render left to skip.
    240_000,
  )
})

/** `n` arrangements of Phone's spec with its LEFT SIDE in play (2026-10-03), alternately menu-built
 *  (every move made as on a window wide enough for the side, so ◀ puts listed panes there and ▶ takes
 *  them back) and a coerced junk record (any id, stop controls included, on the side). */
function randomArrangements<P extends string>(spec: ArrangeSpec<P>, n: number, seed: number): Array<Arrangement<P>> {
  const next = rng(seed)
  const ids = arrangeIds(spec)
  const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
  const out: Array<Arrangement<P>> = []
  while (out.length < n) {
    if (out.length % 2 === 0) {
      let arr: Arrangement<P> = {}
      for (let k = 2 + Math.floor(next() * 14); k > 0; k--) {
        arr = moveArranged(spec, arr, pick(ids), pick(['up', 'down', 'left', 'right'] as PaneMove[]), () => true, true) ?? arr
      }
      out.push(arr)
    } else {
      const raw = [...ids, 'ptt', 'stopTx', 'tune', 'scope', 'txmeters'].filter(() => next() < 0.5)
      out.push({ place: randomPlacements(spec, 1, Math.floor(next() * 1e9))[0], leftSide: coerceLeftSide(spec, raw) })
    }
  }
  return out
}

describe('THE LEFT SIDE SWEEP (2026-10-03): no arrangement with Phone’s left side gates a control that stops a transmission', () => {
  // The side stands between the header and the dock, beside the scope, the TX strip and the region;
  // the strip moves into the stage beside it. Swept on a window wide enough for the side, with every
  // id hidden singly and all at once, as the arrangement sweep above does for the columns.
  afterEach(() => document.documentElement.style.removeProperty('--vw-eff'))

  it(
    'Phone: 30 random arrangements on a wide window, every id hidden singly and all at once, every stop control where it was',
    async () => {
      document.documentElement.style.setProperty('--vw-eff', '1600px')
      const spec = ALL_PANEL_VOCABULARIES.find((v) => v.view === 'phone')!.arrange! as ArrangeSpec<string>
      phone.render(panelsWith<(typeof PHONE_PANEL_IDS)[number]>([]))
      await settle()
      const shown = stopsOnScreen(phone.stopControls)
      const baseline = new Map(phone.stopControls.map(([label]) => [label, shown.get(label)!.some((e) => !e.disabled)]))
      cleanup()
      let sided = 0
      for (const [i, arr] of randomArrangements(spec, 30, 20261003).entries()) {
        const combos: Array<readonly string[]> = [[], ...phone.ids.map((id: string) => [id]), [...phone.ids]]
        for (const removed of combos) {
          ;(phone.render as (p: PanelLayoutApi<string>) => void)(panelsWith(removed, arr.place, arr.leftSide))
          await settle()
          if (removed.length === 0 && document.querySelector('.cockpit-left')) sided++
          const on = stopsOnScreen(phone.stopControls)
          for (const [label] of phone.stopControls) {
            const els = on.get(label)!
            const where = `Phone, arrangement #${i} ${JSON.stringify(arr)}, hiding {${removed.join(', ')}}`
            expect(els.length, `${where} took "${label}" with it`).toBeGreaterThan(0)
            expect(els.some((e) => !e.disabled), `${where} left "${label}" on screen but DISABLED`).toBe(baseline.get(label))
            // …and none of them is on the side.
            expect(els.some((e) => e.closest('.cockpit-left') != null), `${where} put "${label}" on the left side`).toBe(false)
          }
          cleanup()
        }
      }
      expect(sided, 'the random arrangements never put the side on screen — the sweep is reading nothing').toBeGreaterThan(10)
    },
    // 330 fresh mounts; the budget of the arrangement sweep above, scaled.
    240_000,
  )
})

describe('RTTY: the macro editor never stands between the operator and a stop', () => {
  // The F-key editor is the one RTTY surface that takes the caret out of the dock and puts a form
  // in front of the operator, and Esc is the key both "cancel" and "stop" live on. So, with it
  // open: Stop TX and the dock's Esc/Stop are on screen and live while an over is on the air, and
  // Esc stops exactly as it does without the editor. With nothing on the air, Esc closes the
  // editor ONLY — a stop there is haltTx, which turns TX off over a key meant as "cancel".
  //
  // Rendered with the REAL header, as every case above is, because Stop TX lives in it.
  const rttyStop = () => vi.mocked(api.rttyStop)
  const haltTx = () => vi.mocked(api.haltTx)

  /** The station this fixture stands in for, answering EVERY state question the same way. The
   *  cockpit asks twice at mount — its poll's leading read (`getRttyState`) and the decoder's
   *  auto-arm (`rttyAutoArm`) — and both answers carry the whole RTTY state, so whichever lands
   *  last is what renders. Only the poll used to be set: the auto-arm kept answering the idle
   *  file fixture, landed second and wiped `sending`/`latched`, and the check passed only because
   *  the poll's NEXT tick restored them — 500 ms later, on a wall clock, inside `waitFor`'s 1 s
   *  window. At load ~32 that tick came late and the test went red with nothing wrong. */
  const answering = (state: RttyState) => {
    vi.mocked(api.getRttyState).mockImplementation(async () => state)
    vi.mocked(api.rttyAutoArm).mockImplementation(async () => state)
  }
  const asked = () => ({ reads: vi.mocked(api.getRttyState).mock.results.length, arms: vi.mocked(api.rttyAutoArm).mock.results.length })

  /** Await every state answer the cockpit has asked for since `from`, inside act so what they
   *  deliver is committed — the event itself, never a clock, so a loaded box can make this
   *  slower but never red. The caller then asserts `ready` synchronously: a pill or a control
   *  that never comes still fails, and at once. */
  async function answered(from: { reads: number; arms: number }) {
    const reads = vi.mocked(api.getRttyState).mock
    const arms = vi.mocked(api.rttyAutoArm).mock
    expect(reads.results.length, 'the cockpit never read its RTTY state').toBeGreaterThan(from.reads)
    for (let r = from.reads, a = from.arms; r < reads.results.length || a < arms.results.length; ) {
      const pending = [...reads.results.slice(r), ...arms.results.slice(a)].map((x) => x.value)
      r = reads.results.length
      a = arms.results.length
      await act(async () => {
        await Promise.all(pending)
      })
    }
  }

  /** Mount RTTY in `state`, wait until the cockpit has it, and open the F1 editor. */
  async function openEditor(state: RttyState, ready: () => void) {
    answering(state)
    const from = asked()
    render(<RttyCockpit snap={snap} active onSetTxEnabled={() => {}} />)
    await settle()
    await answered(from)
    ready()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Edit the F1 macro' }))
    })
    expect(document.querySelector('.rtty-macro-editor'), 'fixture: the editor is open').not.toBeNull()
    rttyStop().mockClear()
    haltTx().mockClear()
  }
  const onAir = () => expect(screen.getByText('TX ▲')).toBeTruthy()
  const latchUp = () =>
    expect(document.querySelector('.rtty-tx-latch')?.getAttribute('aria-pressed')).toBe('true')

  afterEach(() => answering(rttyState))

  it('Stop TX and the Esc/Stop macro are on screen and enabled while an over is on the air', async () => {
    await openEditor({ ...rttyState, sending: true } as RttyState, onAir)
    const stopTx = screen.getByRole('button', { name: /^stop tx$/i }) as HTMLButtonElement
    const escStop = screen.getByRole('button', { name: /^esc\s*stop$/i }) as HTMLButtonElement
    expect(stopTx.disabled).toBe(false)
    expect(escStop.disabled).toBe(false)
    // …and they still do what they are for, with the editor open.
    fireEvent.click(stopTx)
    expect(haltTx()).toHaveBeenCalled()
    fireEvent.click(escStop)
    expect(rttyStop()).toHaveBeenCalled()
  })

  it('Esc while an over is on the air STOPS — the editor does not swallow it', async () => {
    await openEditor({ ...rttyState, sending: true } as RttyState, onAir)
    fireEvent.keyDown(screen.getByLabelText(/^title$/i), { key: 'Escape' })
    expect(rttyStop()).toHaveBeenCalled()
    expect(haltTx()).toHaveBeenCalled()
  })

  it('Esc while continuous TX is latched STOPS', async () => {
    await openEditor({ ...rttyState, latched: true } as RttyState, latchUp)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(rttyStop()).toHaveBeenCalled()
    expect(haltTx()).toHaveBeenCalled()
  })

  it('Esc with nothing on the air closes the editor and stops NOTHING', async () => {
    await openEditor(rttyState, () => expect(document.querySelector('.rtty-macros')).not.toBeNull())
    fireEvent.keyDown(screen.getByLabelText(/^title$/i), { key: 'Escape' })
    expect(document.querySelector('.rtty-macro-editor'), 'Esc did not close the editor').toBeNull()
    expect(rttyStop()).not.toHaveBeenCalled()
    expect(haltTx(), 'Esc-to-cancel turned TX off').not.toHaveBeenCalled()
    // POSITIVE CONTROL: with the editor closed, the same idle Esc is a stop, as it always was.
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(rttyStop()).toHaveBeenCalled()
    expect(haltTx()).toHaveBeenCalled()
  })
})

// ── ESC, NAMED ON EVERY SCREEN ─────────────────────────────────────────────────────────────────
// Esc is the stop the keyboard holds, on every screen the operator transmits from and on
// Satellites (N71, 2026-10-01; N66 measured it missing on Tempo, SSTV and APRS, Phone's stopping
// only its voice keyer, and Satellites with no stop at all). This file cannot press it: it finds
// controls by accessible name, and five of these screens' Esc is bound by App, which no
// cockpit-level render contains. So each screen is NAMED here with what binds its Esc, and the
// list is computed against the registry: a section the operator can transmit from that is not
// named here goes red, whatever anybody remembers. Every one is PRESSED, with a real keydown, in
// stop-control-wiring.test.tsx ("Esc on every screen in the registry"), which mounts the real App
// over a fake bridge and asserts the exact commands that left the UI; that census must name each
// one too, so the two lists cannot drift apart. The hosted page's Esc is pressed in
// remote-web/remote-stop-line.test.tsx.
describe('Esc is a stop on every operating screen and on Satellites', () => {
  const ESC: Record<string, string> = {
    operate: 'OperateCockpit, on the shared capture listener (useEscStop) while on show → App handleHaltTx (halt_tx)',
    cw: 'CwCockpit, on the shared capture listener → the abort() its Stop TX calls (stop_cw, halt_tx)',
    rtty: 'RttyCockpit, on the shared capture listener → stop() (rtty_stop, halt_tx); an open F-key editor closes instead while nothing is on the air',
    psk: 'PskCockpit, as RTTY (psk_stop, halt_tx)',
    js8: 'Js8Cockpit, on the shared capture listener → stop() (halt_tx)',
    chat: 'App, while Tempo is on show → handleHaltTx, the top bar Stop TX on this screen (halt_tx)',
    phone: 'App, while Phone is on show → handleHaltTx (halt_tx, as its header Stop TX); the voice keyer also stops itself',
    sstv: 'App, while SSTV is on show → handleHaltTx (halt_tx, as its header Stop TX)',
    aprs: 'App, while APRS is on show → handleHaltTx (halt_tx); APRS draws no stop control',
    sats: 'App, while Satellites is on show → handleHaltTx (halt_tx); Satellites draws no stop control',
  }
  // Filed under Operate in the registry and never transmit: manager views that do not touch the rig.
  const NEVER_TRANSMIT = ['memories', 'program']

  it('every section the operator transmits from, and Satellites, is named with what binds its Esc', () => {
    const operating = sectionFeatures()
      .filter((f) => f.category === 'Operate' && !NEVER_TRANSMIT.includes(f.id))
      .map((f) => f.id as string)
    expect(operating.length, 'control: the registry has operating sections').toBeGreaterThan(5)
    expect(Object.keys(ESC).sort()).toEqual([...operating, 'sats'].sort())
  })

  it('…and the wire census presses Esc on each of them, expecting a stop', () => {
    const wiring = readFileSync(resolve(__dirname, '../stop-control-wiring.test.tsx'), 'utf8')
    const census = wiring.slice(wiring.indexOf('const ESC_SCREENS'))
    expect(census.length, 'control: the census is in the wiring suite').toBeLessThan(wiring.length)
    for (const view of Object.keys(ESC)) {
      expect(
        census.includes(`view: '${view}'`),
        `${view}: the wiring census does not press Esc on it`,
      ).toBe(true)
    }
  })
})

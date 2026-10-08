// THE API ANSWERS FOR THE STOP-LINE SWEEPS (components/stop-line*.test.tsx): one mock of the api module
// for every cockpit they render, the union of what those cockpits call on mount. Each sweep file builds
// its `vi.mock('../api', …)` from this, inside the factory:
//
//   vi.mock('../api', async (importOriginal) =>
//     (await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()))
//
// so every file answers alike, as the one file did before the sweeps were split by cockpit. It imports
// nothing of the app but types: it is loaded while the api module is being mocked, before any cockpit.
import { vi } from 'vitest'
import type { Js8State, PskState, RttyState, SstvState } from '../types'

export const decodeState = {
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

export const rttyState = {
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

export const pskState = {
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

export const js8State = {
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

export const sstvState = {
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

// ⭐ DERIVED FROM THE REAL MODULE, not a hand-kept list. A hand-kept mock omits any export
// added after it was written, and a component that calls one THROWS ON MOUNT — so the suite
// goes red at a seam nothing in the diff explains, and the tempting fix is to make the test
// pass rather than ask why. That cost five files one evening when a single API call was added
// to the CW cockpit, the stop-line sweep among them.
//
// Every function the module exports is auto-stubbed here; the entries below override only the
// ones the sweeps' assertions actually depend on, so their shapes are unchanged.
/** The mocked api module: every function the real one exports, auto-stubbed, with the answers the sweeps
 *  depend on set below. `actual` is the real module (`importOriginal()`). */
export function stopLineApi(actual: Record<string, unknown>): Record<string, unknown> {
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
    // FT's callsign card (the case selects a station, so the card is on screen to sweep): the award
    // entity is a name, and the log has nothing to say about the station, as the FT suites answer.
    resolveEntity: vi.fn(async () => 'United States'),
    askLog: vi.fn(async () => null),
  }
}

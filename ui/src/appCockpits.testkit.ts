// THE REAL APP ON ANY OPERATING COCKPIT, IN JSDOM — the api answers that let App boot and each
// cockpit mount, for the suites that put something BESIDE a cockpit (the dashboard rail's
// App.dashRail.test.tsx and DashRail.stopLine.test.tsx). Lifted from App.js8workspace.test.tsx's boot
// list and stop-line.test.tsx's cockpit fixtures, so the two answer alike.
//
// ⚠️ A cockpit handed `{}` for its own state crashes into App's boundary, and an assertion about
// anything beside it — the rail, the NOW bar — then passes over a crash panel. Every consumer should
// assert the cockpit itself rendered.
//
// Used from inside a `vi.mock('…/api', …)` factory: `{ ...auto, ...(await import(…)).appApiAnswers() }`.
// It writes no storage: a suite sets its own keys (storage-scope.test.ts reads this file as a source).
import { vi } from 'vitest'
import type { Js8State, PskState, RttyState, SstvState } from './types'

export const APP_SNAPSHOT = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
}

const decodeState = {
  text: '',
  wpm: 22,
  sent: [],
  keyerError: null,
  candidates: [],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null,
  workedCall: null,
  rst: null,
  name: null,
}
const rttyState = {
  armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170, markHz: 2125,
  spaceHz: 2295, sending: false, latched: false, backend: 'afsk', keyerError: null, auto: false, seqState: 'idle',
  peer: null, peerExchange: [], heardCq: null,
} as unknown as RttyState
const pskState = {
  armed: true, afcHz: 0, signal: false, centerHz: 1000, text: '', charConf: [], sending: false, latched: false,
  keyerError: null,
} as unknown as PskState
const js8State = {
  speed: 'normal', rxSpeeds: 15, txEnabled: true, sending: false, hbOn: false, hbNextAtMs: null, hbIntervalMin: 0,
  cqOn: false, cqNextAtMs: null, cqIntervalMin: 0, autoreply: true, relay: true, hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false }, idleMinutes: 0, idleLimitMin: 60,
  idleTripped: false, activity: [], stations: [], inbox: [], queue: [], pendingReply: null, lastError: null,
} as unknown as Js8State
const sstvState = {
  armed: false, mode: null, linesDone: 0, linesTotal: 0, previewRgbBase64: null, previewWidth: 0, previewHeight: 0,
  hedrShiftHz: 0, gallery: [],
  health: {
    armed: false, audioPeak: 0, lastAudioUnix: null, drains: 0, visSeen: 0, lastVisUnix: null, unknownVis: 0,
    lastUnknownVisCode: null, lastUnknownVisUnix: null, images: 0, lastImageUnix: null,
  },
  sending: false, txMode: null, txProgress: 0, txElapsedSecs: 0, txTotalSecs: 0,
} as unknown as SstvState

/** The answers App needs to boot, Connect's feeds, and each cockpit's own state. */
export function appApiAnswers(): Record<string, unknown> {
  const rtty = vi.fn(async () => rttyState)
  const psk = vi.fn(async () => pskState)
  const js8 = vi.fn(async () => js8State)
  const sstv = vi.fn(async () => sstvState)
  return {
    askLog: vi.fn(async () => {
      throw new Error('no log in this test')
    }),
    getSnapshot: vi.fn(async () => APP_SNAPSHOT),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => null),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    logActivations: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => APP_SNAPSHOT),
    setArea: vi.fn(async () => APP_SNAPSHOT),
    appVersion: vi.fn(async () => '0.0.0-test'),
    selectPeer: vi.fn(async () => APP_SNAPSHOT),
    getDxccEntityLocations: vi.fn(async () => []),
    // Connect's feeds, as the dashboard rail beside a cockpit reads them.
    getGettingOut: vi.fn(async () => ({
      count: 1,
      maxKm: 1500,
      reports: [{ call: 'K1ABC', octant: 'E', km: 1500, band: '20m', snr: -12 }],
    })),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getDxpedWindows: vi.fn(async () => []),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getPathOutlook: vi.fn(async () => null),
    // The rail's Spots and POTA/SOTA boxes: a Work's and a hunt's answers, the log prefill the cockpit
    // a Work opens looks up, and the POTA/SOTA board's own reads (a suite gives it its spots).
    workSpot: vi.fn(async () => APP_SNAPSHOT),
    setHuntTarget: vi.fn(async () => APP_SNAPSHOT),
    resolveEntity: vi.fn(async () => null),
    getOtaSpots: vi.fn(async () => []),
    getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
    parksCount: vi.fn(async () => 0),
    huntedParksCount: vi.fn(async () => 0),
    // Phone and CW.
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getVoiceMessages: vi.fn(async () => []),
    cwDecode: vi.fn(async () => decodeState),
    previewCw: vi.fn(async (t: string) => t),
    readRotator: vi.fn(async () => null),
    getDeclination: vi.fn(async () => 0),
    getSatTransponder: vi.fn(async () => null),
    // The keyboard and picture cockpits.
    getRttyState: rtty,
    rttyArm: rtty,
    rttyAutoArm: rtty,
    getPskState: psk,
    pskArm: psk,
    pskAutoArm: psk,
    getJs8State: js8,
    js8Enter: js8,
    js8Arm: js8,
    getSstvState: sstv,
    sstvArm: sstv,
    sstvAutoArm: sstv,
    // APRS (AprsCockpit.layout.test.tsx's shapes).
    aprsArm: vi.fn(async () => []),
    aprsAutoArm: vi.fn(async () => true),
    getAprsHeard: vi.fn(async () => []),
    getAprsHealth: vi.fn(async () => ({
      arm: 'auto' as const,
      audioPeak: 0,
      lastAudioUnix: null,
      framesSeen: 0,
      framesDecoded: 0,
      lastDecodeUnix: null,
    })),
    getAprsIsStatus: vi.fn(async () => ({
      enabled: false, connected: false, verified: false, packets: 0, lastPacketUnix: null,
      uplinkEnabled: false, uploaded: 0, gateRejected: 0, lastReject: null,
    })),
    getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  }
}

/** The main element each operating cockpit renders, for "did it really mount?". */
export const COCKPIT_MAIN: Record<string, string> = {
  operate: 'main.operate-cockpit',
  phone: 'main.phone-cockpit',
  cw: 'main.cw-cockpit',
  rtty: 'main.rtty-cockpit',
  psk: 'main.psk-cockpit',
  sstv: 'main.sstv-view',
  aprs: 'main.aprs-cockpit',
  js8: 'main.js8-cockpit',
}

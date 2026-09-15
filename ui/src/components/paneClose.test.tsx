// @vitest-environment jsdom
//
// THE PANE'S OWN ✕, COMPUTED — the five keyboard/phone cockpits (2026-09-15).
//
// The operator went looking for a way to close a panel on the FT8 screen and could not find
// one. ⊞ Panels had always been able to; nothing on the pane said so. Every removable pane
// now carries a ✕ in its own header, and this file is what keeps that true across the
// cockpits Operate's own sweep (OperateCockpit.paneClose.test.tsx) does not cover.
//
// WHAT IT COMPUTES:
//   1. Every ⊞ id whose pane is ON SCREEN in this fixture has a ✕, found by its ACCESSIBLE
//      NAME — "Hide <the pane's own head title>". That is the name ON THE PANE, deliberately,
//      rather than the ⊞ label: the two differ in wording for several panes ("Band activity"
//      on the head, "Band Activity" in the menu; "Decode" vs "CW Decode"), and a screen
//      reader must say what the operator is looking at.
//   2. Pressing it REMOVES that pane and only that pane, read back off the record the
//      cockpit itself writes. This is the half a presence test cannot see: `onClick={() =>
//      {}}` renders, reads and focuses correctly and does nothing (the same blind spot the
//      stop-line sweeps name in their own headers).
//   3. Phone's voice keyer — the ONE pane in the app whose hide ends something in flight —
//      carries the SAME consequence sentence on its ✕ that the ⊞ entry prints under its
//      checkbox. One wording, two doors. THE PRACTICE half of THE STOP LINE puts the
//      consequence BEFORE the act, and a ✕ is an act with no "before" unless it says so.
//   4. A pane with NO vocabulary entry has NO ✕ (the log strips, CW's merged rig-control
//      frame). Not removable must stay unrepresentable, not merely guarded.
//
// WHAT IT DOES NOT COMPUTE:
//   · `scope` — its ✕ is inside <Waterfall/> in RTTY/PSK/SSTV/JS8 and inside the cockpit's
//     own `.ph-scope-head` in Phone/CW. Waterfall is STUBBED here (canvas), so the four
//     waterfall cockpits' `scope` is swept in Waterfall.paneClose.test.tsx instead; Phone's
//     and CW's render here and are swept here.
//   · `txmeters` — a bare meter strip with no head of its own in any cockpit, so it has no
//     ✕ and ⊞ Panels stays its only route. Asserted as an ABSENCE below.
//   · CW's `scopeCtl` / `dsp` / `rxdsp` — three ⊞ ids sharing ONE frame ("Rig controls",
//     paneId="rigctl", which is not in CW_PANEL_IDS). One ✕ on that frame could only be
//     ambiguous about which of the three it closes, so it has none. Asserted as an absence.
//
// THIS FILE MAKES NO CLAIM ABOUT THE STOP LINE. stop-line.test.tsx owns that sweep and is
// unchanged by this work: the ✕ writes `removed` for an id the menu already listed, so it
// cannot reach a control that has no id.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { useState } from 'react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import { Js8Cockpit } from './Js8Cockpit'
import { SstvView } from './SstvView'
import {
  PHONE_PANEL_IDS,
  CW_PANEL_IDS,
  RTTY_PANEL_IDS,
  PSK_PANEL_IDS,
  JS8_PANEL_IDS,
  SSTV_PANEL_IDS,
} from '../features/panelState'
import type { PanelLayoutApi, PanelState } from '../features/panelState'
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
    getLog: vi.fn(async () => []),
    // `js8_enter` fires on the rising edge of `active`; the auto-stub would answer `{}` and
    // the cockpit would then render a state with no `armed` — pin the fixture.
    js8Enter: vi.fn(async () => js8State),
    js8Arm: vi.fn(async () => js8State),
    js8Send: vi.fn(async () => js8State),
    js8SendCommand: vi.fn(async () => js8State),
    js8CallCq: vi.fn(async () => js8State),
    js8SetSpeed: vi.fn(async () => js8State),
    js8Cancel: vi.fn(async () => js8State),
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


// ── The live record. `panelsWith` above is FROZEN — right for the stop-line sweep, which
//    renders a state and looks at it, and wrong here: these cases press a button and need
//    the cockpit to re-read what it wrote. This is `usePanelLayout`'s shape with React state
//    behind it, so the write and the re-render are the app's own.
let live: PanelLayoutApi<string> | null = null

function useLivePanels(): PanelLayoutApi<string> {
  const [state, setState] = useState<Partial<Record<string, PanelState>>>({})
  const api: PanelLayoutApi<string> = {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => state[id] ?? 'docked',
    setPanelState: (id, s) => setState((cur) => ({ ...cur, [id]: s })),
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
  live = api
  return api
}

const stateOf = (id: string): PanelState => live?.stateOf(id) ?? 'docked'

/** The record above is keyed by `string`; each cockpit's prop wants its own id union. It is
 *  the SAME object either way — this only re-labels it, so a case cannot quietly hand a
 *  cockpit a different record from the one `stateOf` reads back. */
const as = <P extends string>(api: PanelLayoutApi<string>) => api as unknown as PanelLayoutApi<P>

/** One cockpit's ✕ case. `panes` is the [⊞ id, ⊞ label] pairs whose pane is on screen in
 *  this fixture — a SUBSET of the vocabulary, because `scope` and `txmeters` are excluded
 *  for the reasons in the header, and because a capability-gated pane (Phone's DSP rows)
 *  renders nothing against a fixture radio that reports no DSP fields. */
interface CloseCase {
  cockpit: string
  ids: readonly string[]
  panes: Array<[id: string, label: string]>
  /** A pane on screen that has NO ⊞ entry, and therefore must have no ✕. */
  unremovable: string[]
  /** id → the sentence the ⊞ entry prints AND the ✕ must carry. */
  endsOnHide?: Record<string, RegExp>
  Host: (p: { panels: PanelLayoutApi<string> }) => React.ReactElement
}

const CASES: CloseCase[] = [
  {
    cockpit: 'Phone',
    ids: PHONE_PANEL_IDS,
    // `rigscope`/`dsp`/`dspLevels` are capability-gated on what the fixture radio reports;
    // `bandActivity` needs an onWorkSpot, which this host passes. `scope` renders here and
    // is swept — its ✕ is in PhoneCockpit's own `.ph-scope-head`, not in the stubbed child.
    panes: [
      ['scope', 'Scope'],
      ['bandActivity', 'Band activity'],
      ['voiceKeyer', 'Voice keyer'],
    ],
    unremovable: ['Log'],
    // The ONE pane in the app whose hide ends something: unmounting the keyer stops a voice
    // message that is playing and throws away a recording in progress.
    endsOnHide: { voiceKeyer: /stops a voice message that is playing/i },
    Host: ({ panels }) => (
      <PhoneCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} panels={as(panels)} fieldDay={fdStatus} />
    ),
  },
  {
    cockpit: 'CW',
    ids: CW_PANEL_IDS,
    panes: [
      ['scope', 'Scope'],
      ['decode', 'Decode'],
      ['bandActivity', 'Band activity'],
      ['copilot', 'Copilot'],
    ],
    unremovable: ['Log', 'Rig controls'],
    Host: ({ panels }) => (
      <CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} panels={as(panels)} fieldDay={fdStatus} />
    ),
  },
  {
    cockpit: 'RTTY',
    ids: RTTY_PANEL_IDS,
    // `stream` HOSTS A STOP CONTROL of its own (the Auto toggle) and is still hideable —
    // that is THE STOP LINE holding, not a hole in it. Its hide ENDS nothing (unmounting the
    // pane calls no wire), so it correctly carries no consequence note, on the tick or the ✕.
    panes: [['stream', 'Decoded text']],
    unremovable: ['Log'],
    Host: ({ panels }) => <RttyCockpit snap={snap} panels={as(panels)} onSetTxEnabled={() => {}} />,
  },
  {
    cockpit: 'PSK',
    panes: [['stream', 'Decoded text']],
    ids: PSK_PANEL_IDS,
    unremovable: ['Log'],
    Host: ({ panels }) => <PskCockpit snap={snap} panels={as(panels)} onSetTxEnabled={() => {}} />,
  },
  {
    cockpit: 'JS8',
    ids: JS8_PANEL_IDS,
    panes: [
      ['activity', 'Activity'],
      ['offsets', 'Band activity'],
      ['stations', 'Stations'],
      ['inbox', 'Inbox'],
      ['log', 'Log'],
    ],
    unremovable: [],
    Host: ({ panels }) => <Js8Cockpit snap={snap} panels={as(panels)} onSetTxEnabled={() => {}} />,
  },
  {
    cockpit: 'SSTV',
    ids: SSTV_PANEL_IDS,
    panes: [
      ['txcompose', 'Transmit'],
      ['gallery', 'Gallery'],
    ],
    unremovable: [],
    Host: ({ panels }) => <SstvView snap={snap} panels={as(panels)} onSetTxEnabled={() => {}} />,
  },
]

function Mount({ Host }: { Host: CloseCase['Host'] }) {
  const panels = useLivePanels()
  return <Host panels={as(panels)} />
}

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe("every removable pane carries its own ✕, and it is the ⊞ tick's own act", () => {
  for (const c of CASES) {
    for (const [id, label] of c.panes) {
      it(`${c.cockpit}: the ✕ on ${label} removes ${id} and nothing else`, async () => {
        live = null
        render(<Mount Host={c.Host} />)
        await settle()
        const close = screen.getByRole('button', { name: `Hide ${label}` })
        expect(stateOf(id)).toBe('docked')
        fireEvent.click(close)
        await settle()
        // THE RECORD MOVED — the assertion an inert ✕ cannot survive.
        expect(stateOf(id)).toBe('removed')
        for (const other of c.ids) {
          if (other !== id) expect(stateOf(other), `${c.cockpit}/${other}`).toBe('docked')
        }
        // …and the pane is off the screen, its own ✕ with it.
        expect(screen.queryByRole('button', { name: `Hide ${label}` })).toBeNull()
        cleanup()
      })
    }

    it(`${c.cockpit}: a pane with no ⊞ entry has no ✕`, async () => {
      live = null
      render(<Mount Host={c.Host} />)
      await settle()
      // POSITIVE CONTROL: this render DOES contain ✕ buttons, so an absence below is the
      // pane and not a broken query.
      expect(screen.getAllByRole('button', { name: /^Hide / }).length).toBeGreaterThan(0)
      for (const label of c.unremovable) {
        expect(screen.queryByRole('button', { name: `Hide ${label}` }), label).toBeNull()
      }
      // TX meters is a bare strip in every cockpit that has the entry — no head, no ✕.
      expect(screen.queryByRole('button', { name: /^Hide TX Meters$/i })).toBeNull()
      cleanup()
    })
  }

  it('Phone: the keyer ✕ carries the SAME sentence the ⊞ entry prints — one wording, two doors', async () => {
    const phone = CASES[0]
    live = null
    render(<Mount Host={phone.Host} />)
    await settle()
    const close = screen.getByRole('button', { name: 'Hide Voice keyer' })
    const warning = phone.endsOnHide!.voiceKeyer
    // On the tooltip, for a pointer operator…
    expect(close.getAttribute('title')).toMatch(warning)
    // …and as the accessible DESCRIPTION, which is what is read on focus — before the press.
    const describedBy = close.getAttribute('aria-describedby')
    expect(describedBy, 'the ✕ carries no accessible description').toBeTruthy()
    expect(document.getElementById(describedBy!)?.textContent).toMatch(warning)
    // The ⊞ entry says the same thing, because both read the cockpit's one `endsOnHide`.
    fireEvent.click(screen.getByRole('button', { name: /panels/i }))
    // The menu's own note line — matched by its class so the ✕'s hidden description (which
    // holds the identical string, which is the point) cannot stand in for it.
    const menuNote = [...document.querySelectorAll('.panels-menu-why')].map((e) => e.textContent ?? '')
    expect(menuNote.some((txt) => warning.test(txt)), 'the ⊞ entry lost its note').toBe(true)
    // SAME STRING, not merely both matching the pattern: two doors onto one act must not be
    // able to describe it two ways.
    expect(menuNote.find((txt) => warning.test(txt))).toBe(
      document.getElementById(describedBy!)?.textContent,
    )
    // POSITIVE CONTROL: a pane whose hide ends NOTHING carries no description at all — a
    // note the operator cannot act on teaches him to ignore the next one.
    const plain = screen.getByRole('button', { name: 'Hide Band activity' })
    expect(plain.getAttribute('aria-describedby')).toBeNull()
    expect(plain.getAttribute('title')).not.toMatch(warning)
  })
})

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
// WHAT THIS DOES NOT COMPUTE, stated rather than guarded:
//   · A STOP CONTROL THAT IS PRESENT, ENABLED AND INERT. Every assertion here is about the
//     button being in the document and operable — never about what pressing it does.
//     Verified: `onClick={() => {}}` on CockpitHeader's Stop TX passes this file and the
//     whole suite. Nothing below would go red.
//   · That a NEWLY ADDED stop control was added to the list below. Adding one is a human
//     step. Each cockpit's list is its case's `stopControls`, so the next person editing a
//     dock finds it beside the cockpit it guards.
// What IS computed is that every vocabulary in the app has a sweep at all — the last test
// in the file, driven off ALL_PANEL_VOCABULARIES, so a sixth cockpit cannot ship without one.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { RttyCockpit } from './RttyCockpit'
import { SstvView } from './SstvView'
import {
  ALL_PANEL_VOCABULARIES,
  PHONE_PANEL_IDS,
  CW_PANEL_IDS,
  RTTY_PANEL_IDS,
  SSTV_PANEL_IDS,
} from '../features/panelState'
import type { PanelLayoutApi } from '../features/panelState'
import type { AppSnapshot, RttyState, SstvState } from '../types'

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
  backend: 'afsk',
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
} as unknown as RttyState

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
vi.mock('../api', () => ({
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
  stopSatTrack: vi.fn(async () => ({})),
  getRttyState: vi.fn(async () => rttyState),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyArm: vi.fn(async () => rttyState),
  rttySend: vi.fn(async () => rttyState),
  rttyStop: vi.fn(async () => rttyState),
  rttyClear: vi.fn(async () => rttyState),
  rttyAfcReset: vi.fn(async () => rttyState),
  rttyNet: vi.fn(async () => rttyState),
  rttySetAuto: vi.fn(async () => rttyState),
  rttyAutoCq: vi.fn(async () => rttyState),
  rttyAutoAnswer: vi.fn(async () => rttyState),
  rttyAutoAbort: vi.fn(async () => rttyState),
  getSstvState: vi.fn(async () => sstvState),
  sstvArm: vi.fn(async () => sstvState),
  sstvAutoArm: vi.fn(async () => sstvState),
  sstvSend: vi.fn(async () => sstvState),
  sstvStop: vi.fn(async () => sstvState),
  setOperatingMode: vi.fn(async () => ({})),
}))
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
    ['PTT', /push to talk|on air — release to stop|tx locked/i],
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
  render: (panels) =>
    render(
      <PhoneCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />,
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
  render: (panels) =>
    render(<CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels} />),
}

/** The TX-enable latch as CockpitHeader labels it. `radio.txEnabled` is true and
 *  `transmitting` false in this fixture, so it reads "▼ TX On"; the disarmed face is matched
 *  too, so a fixture flip cannot make the sweep silently stop finding the control. */
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
  stopControls: [
    ['Stop TX', /^stop tx$/i],
    ['Stop (RTTY abort)', /^esc\s*stop$/i],
    TX_LATCH,
  ],
  // onSetTxEnabled exactly as App passes it (the .rtty-host block). Without it CockpitHeader
  // renders a display-only pill and the latch is not on screen to sweep.
  render: (panels) => render(<RttyCockpit snap={snap} panels={panels} onSetTxEnabled={() => {}} />),
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
const CASES: Array<Case<any>> = [phone, cw, rtty, sstv]

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

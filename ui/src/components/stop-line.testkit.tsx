// THE STOP-LINE SWEEPS' CASES AND TOOLS (components/stop-line*.test.tsx). The sweeps ran from one file
// until 2026-10-07, when the cockpits' arrangement sweeps grew long enough that the file was the slowest
// in the UI suite by far; they are split by cockpit now, so each runs on a worker of its own, and this is
// what they share: the fixtures, one case per cockpit (the props App gives it and its stop controls),
// the placement generators and the run of the arrangement sweep. Nothing here is a test of its own.
//
// The rule, what each case's `stopControls` may hold and what the sweeps do not prove are in the header
// of stop-line.test.tsx, which keeps the vocabulary sweep, the coverage checks and the Esc census.
// ⚠️ A sweep file mocks the api module from stop-line.api.testkit.ts BEFORE it imports this, so the
// cockpits below mount against those answers.
import { expect } from 'vitest'
import { render, screen, cleanup, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { RttyCockpit } from './RttyCockpit'
import { PskCockpit } from './PskCockpit'
import { Js8Cockpit } from './Js8Cockpit'
import { SstvView } from './SstvView'
import { OperateCockpit } from './OperateCockpit'
import {
  ALL_PANEL_VOCABULARIES,
  OPERATE_ARRANGE,
  OPERATE_PANEL_IDS,
  PHONE_PANEL_IDS,
  CW_PANEL_IDS,
  RTTY_PANEL_IDS,
  PSK_PANEL_IDS,
  JS8_PANEL_IDS,
  SSTV_PANEL_IDS,
} from '../features/panelState'
import type { PanelLayoutApi } from '../features/panelState'
import type { BoxSource } from './panes/CockpitBox'
import {
  arrangeIds,
  coerceLeftSide,
  coercePlacement,
  columnsOf,
  dropArranged,
  moveArranged,
  movePane,
  placedColumns,
  type Arrangement,
  type ArrangeSpec,
  type PaneColumn,
  type PaneMove,
  type PanePlacement,
} from '../features/panelPlace'
import type { AppSnapshot, FieldDayStatus } from '../types'

/** A panel record with every id shown but `removed`, and the placement given: the record's own, or, for a
 *  cockpit arranged per layout (FT), that layout's (`layoutName`), with `extras` (the other layout's panes)
 *  added to that layout. */
export function panelsWith<P extends string>(removed: readonly P[], place?: PanePlacement<P>, leftSide?: P[], layoutName?: string, extras?: readonly P[]): PanelLayoutApi<P> {
  const placed = place ? (layoutName != null ? { places: { [layoutName]: place } } : { place }) : {}
  const added = extras?.length && layoutName != null ? { extras: { [layoutName]: [...extras] } } : {}
  return {
    layout: place || leftSide || extras?.length ? { v: 2, state: {}, share: {}, ...placed, ...(leftSide ? { leftSide } : {}), ...added } : { v: 1, state: {}, share: {} },
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
export const snap = { mycall: 'KD9TAW', radio } as unknown as AppSnapshot

/** Field Day switched ON, as App hands it to the cockpits during an event — the state that
 *  swaps their log strip for the FD one. See the Phone case's own note on why it is swept. */
export const fdStatus = {
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
export const spotsBoard = { bandPlan: [], selectedCall: null, onSelect: () => {}, onWork: () => {} }
export const neededBoard = { alerts: [], bandPlan: [], selectedCall: null, onQsy: () => {}, onSelect: () => {} }
/** What App lends the boxes of Phone, CW and JS8 on the desktop (any pane in any area, 2026-10-07). */
export const boxSource: BoxSource = { myGrid: 'EN52', theme: 'dark', stations: [], prop: null, needByCall: new Map() }

/**
 * One cockpit's stop-line case. `stopControls` are accessible-name matchers for the controls
 * that END a transmission AND RENDER OUTSIDE EVERY ⊞-REMOVABLE PANE — the set the guarantee
 * rests on, not every stop control on the screen. See the file header: a pane-resident stop
 * (Phone's ■ Stop, RTTY's Auto toggle) must stay off these lists.
 */
export interface Case<P extends string> {
  cockpit: string
  /** The vocabulary's own `view` id — how the coverage check below matches a case to a
   *  vocabulary, so neither list can drift out from under the other. */
  view: string
  ids: readonly P[]
  stopControls: Array<[label: string, name: RegExp]>
  render: (panels: PanelLayoutApi<P>) => void
  /** A cockpit arranged per layout (FT): the layout this case renders, its ArrangeSpec, and how to read
   *  the order its panes stand in, which the arrangement sweep compares with the stock one. */
  layout?: string
  arrange?: ArrangeSpec<P>
  order?: () => Array<string | null>
}

export const phone: Case<(typeof PHONE_PANEL_IDS)[number]> = {
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
        boxes={boxSource}
      />,
    ),
}

export const cw: Case<(typeof CW_PANEL_IDS)[number]> = {
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
        boxes={boxSource}
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
export const TX_LATCH: [string, RegExp] = ['TX-enable latch', /^tx on$|^tx off$/i]

export const rtty: Case<(typeof RTTY_PANEL_IDS)[number]> = {
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

export const psk: Case<(typeof PSK_PANEL_IDS)[number]> = {
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

export const js8: Case<(typeof JS8_PANEL_IDS)[number]> = {
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
  render: (panels) => render(<Js8Cockpit snap={snap} panels={panels} onSetTxEnabled={() => {}} boxes={boxSource} />),
}

export const sstv: Case<(typeof SSTV_PANEL_IDS)[number]> = {
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

export const phoneDual: Case<(typeof PHONE_PANEL_IDS)[number]> = {
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
        boxes={boxSource}
      />,
    ),
}
export const cwDual: Case<(typeof CW_PANEL_IDS)[number]> = {
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
        boxes={boxSource}
      />,
    ),
}

/** FT's panes in the order they stand, from the wrappers it has always drawn — the stock branch puts no
 *  `data-pane` on them, the arranged one keeps the same wrappers — so a stock and an arranged screen read
 *  alike. */
function operateOrder(): string[] {
  const lower = document.querySelector('.cockpit-lower')
  if (!lower) return []
  const of = (el: Element): string | null =>
    el.getAttribute('data-pane') ??
    (el.matches('.cockpit-decodes, .cockpit-decodes-side')
      ? 'bandActivity'
      : el.matches('.cockpit-rxfreq')
        ? 'rxfreq'
        : el.matches('.cockpit-roster-main')
          ? 'callRoster'
          : el.matches('.cockpit-roster')
            ? 'stations'
            : el.matches('.tx-panel')
              ? 'txmsgs'
              : el.matches('.recall-card')
                ? 'recall'
                : null)
  const out: string[] = []
  for (const el of lower.querySelectorAll('[data-pane], .cockpit-decodes, .cockpit-decodes-side, .cockpit-rxfreq, .cockpit-roster-main, .cockpit-roster, .tx-panel, .recall-card')) {
    const id = of(el)
    if (id && out[out.length - 1] !== id) out.push(id)
  }
  return out
}

/** FT, in each of its layouts, with the props App gives it and the boxes App lends it. Its stop-line
 *  census is Stop TX (`.op-btn.stop` in the QSO strip → halt_tx, the only control that cuts an over in
 *  flight), Tune (the carrier it started) and Esc (keyboard-only, census-only). TX On/Off and S&P are NOT
 *  stops (`set_tx_enabled` lets the over in flight complete, by the operator's 2026-07-31 ruling; S&P
 *  ends the CQ run and arms nothing), so they are not listed; OperateCockpit.structure.test.tsx holds the
 *  whole strip — every TX and sequencer control — inside the QSO strip, a wider claim than this. */
const operateCase = (layout: 'classic' | 'roster'): Case<(typeof OPERATE_PANEL_IDS)[number]> => ({
  cockpit: layout === 'classic' ? 'FT, Classic' : 'FT, Roster',
  view: 'operate',
  ids: OPERATE_PANEL_IDS,
  stopControls: [
    ['Stop TX', /^stop tx$/i],
    ['Tune', /^tune$|^tuning…$/i],
  ],
  layout,
  arrange: OPERATE_ARRANGE[layout],
  order: operateOrder,
  render: (panels) =>
    render(
      <OperateCockpit
        snap={operateSnap}
        theme="dark"
        tier="FT8"
        onTierChange={() => {}}
        bandPlan={[]}
        onSetFrequency={() => {}}
        onSourceChange={() => {}}
        onTune={() => {}}
        onCall={() => {}}
        onSetTxLevel={() => {}}
        onSetMode={() => {}}
        onSetTxEven={() => {}}
        onSetTxCycleAuto={() => {}}
        onResend={() => {}}
        onFreetext={() => {}}
        onLog={() => {}}
        onOverrideTx={() => {}}
        onHaltTx={() => {}}
        onSetTxEnabled={() => {}}
        onSetTune={() => {}}
        onSetHoldTxFreq={() => {}}
        // App's Stations list, as far as a sweep reads it: its head is the grip ⊞ Arrange drags it by.
        roster={
          <div data-testid="stations-roster">
            <div data-pane-grip="stations">Stations</div>
          </div>
        }
        needByCall={new Map()}
        selectedCall="W1ABC"
        onSelect={() => {}}
        layoutMode={layout}
        onLayoutMode={() => {}}
        panels={panels}
        boxes={boxSource}
      />,
    ),
})

/** FT's station: a station heard and selected, so the callsign card is on screen at every baseline. */
const operateSnap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  stations: [{ call: 'W1ABC', grid: 'FN42', snr: -7, lastHeardSlot: 0, heardCount: 3, presence: 'live', worked: false, country: 'United States' }],
  recentDecodes: [],
  conversations: [],
  highlights: [],
  harqRescues: 0,
  clearTick: 0,
  qso: null,
  link: { tier: 'FT8', periodSecs: 15 },
  radio: {
    ...radio,
    dialMhz: 14.074,
    slot: 0,
    source: 'native',
    sourceLabel: 'Native',
    nextSlotMs: 5000,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    txEven: true,
    txCycleAuto: true,
    atu: true,
    decodeDepth: 2,
  },
} as unknown as AppSnapshot

export const operateClassic = operateCase('classic')
export const operateRoster = operateCase('roster')

export const CASES: Array<Case<any>> = [phone, cw, rtty, psk, js8, sstv, phoneDual, cwDual, operateClassic, operateRoster]

/** Each case by the name it is exported under: how stop-line.test.tsx reads which case a sweep file sweeps
 *  (its coverage checks), so a file that sweeps the wrong cockpit, or none, is red there. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const CASE_BY_NAME: Readonly<Record<string, Case<any>>> = { phone, cw, rtty, psk, js8, sstv, phoneDual, cwDual, operateClassic, operateRoster }

export async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

/** Each stop control on the screen, by label: the buttons `screen.queryAllByRole('button', { name })`
 *  finds for its name, in the same order. That query works out the accessible name of every button
 *  on the screen before it matches one, so a query per control worked out the same names once per
 *  control. Here one query works them out once, and each control's name is matched against them. */
export function stopsOnScreen(controls: Array<[label: string, name: RegExp]>): Map<string, HTMLButtonElement[]> {
  const names = new Map<Element, string>()
  const all = screen.queryAllByRole('button', {
    name: (accessible, el) => {
      names.set(el, accessible)
      return controls.some(([, name]) => name.test(accessible))
    },
  }) as HTMLButtonElement[]
  return new Map(controls.map(([label, name]) => [label, all.filter((e) => name.test(names.get(e)!))]))
}

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
export function randomPlacements<P extends string>(spec: ArrangeSpec<P>, n: number, seed: number): Array<PanePlacement<P>> {
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

/** THE ARRANGEMENT SWEEP's tests for the case `c`: its fifty placements in five runs of ten, then (2026-10-08)
 *  a sixth run of ten arrangements made by DRAGGING panes (`draggedArrangements`), each the test title's
 *  parameters (the cockpit, what the run sweeps), then the case and the run. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function arrangementRuns(c: Case<any>) {
  return [
    ...[0, 1, 2, 3, 4].map((k) => [c.cockpit, `random placements ${k * 10 + 1}–${k * 10 + 10} of 50`, c, k] as const),
    [c.cockpit, 'ten arrangements made by dragging panes', c, DRAGGED_RUN] as const,
  ]
}
/** The sixth run: arrangements made by drops, not by arrows (THE ARRANGEMENT SWEEP, above). */
export const DRAGGED_RUN = 5

/** `n` arrangements made by DROPS (⊞ Arrange by drag, 2026-10-08): each a run of random drops — any pane onto
 *  any place, above any pane there or at its foot, the left side in play where the cockpit has one — through
 *  the drop itself (`dropArranged`), so the sweep covers what a drag can make. */
export function draggedArrangements<P extends string>(spec: ArrangeSpec<P>, n: number, seed: number): Array<Arrangement<P>> {
  const next = rng(seed)
  const ids = arrangeIds(spec)
  const pick = <T,>(xs: readonly T[]) => xs[Math.floor(next() * xs.length)]
  const side = spec.leftSide != null
  const areas: Array<PaneColumn | 'side'> = [...columnsOf(spec), ...(side ? (['side'] as const) : [])]
  const out: Array<Arrangement<P>> = []
  while (out.length < n) {
    let arr: Arrangement<P> = {}
    for (let k = 2 + Math.floor(next() * 12); k > 0; k--) {
      const id = pick(ids)
      const area = pick(areas)
      const there = (area === 'side' ? (arr.leftSide ?? []) : placedColumns(spec, arr.place)[area]).filter((x) => x !== id)
      const before = next() < 0.3 || there.length === 0 ? null : pick(there)
      arr = dropArranged(spec, arr, id, { area, before }, () => true, side) ?? arr
    }
    out.push(arr)
  }
  return out
}

/** One run of THE ARRANGEMENT SWEEP (above) for the case `c`: placements 10k + 1 to 10k + 10 of the fifty,
 *  the same seed in every run, so the five runs are exactly the placements, hides and assertions of one
 *  run of fifty. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function arrangementRun(c: Case<any>, k: number): Promise<void> {
  const spec = (c.arrange ?? ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)!.arrange!) as ArrangeSpec<string>
  const order = c.order ?? (() => [...document.querySelectorAll('.cockpit-panes .pane-frame')].map((f) => f.getAttribute('data-pane')))
  // A layout's `extra` panes (FT: the Call Roster in Classic, the Tx messages and Stations in Roster) are added
  // to it in every placement, so the sweep places them in every column and hides them with every other id.
  const extras = spec.extra
  c.render(panelsWith<string>([]))
  await settle()
  const shown = stopsOnScreen(c.stopControls)
  const baseline = new Map(c.stopControls.map(([label]) => [label, shown.get(label)!.some((e) => !e.disabled)]))
  cleanup()
  // The order a placement is compared with: the stock arrangement with the same panes added.
  c.render(panelsWith<string>([], undefined, undefined, c.layout, extras))
  await settle()
  const stock = order()
  cleanup()
  let differs = 0
  const placements: Array<Arrangement<string>> =
    k === DRAGGED_RUN ? draggedArrangements(spec, 10, 20261008) : randomPlacements(spec, 50, 20260929).slice(k * 10, k * 10 + 10).map((place) => ({ place }))
  expect(placements.length).toBe(10)
  for (const [j, { place, leftSide }] of placements.entries()) {
    const i = k * 10 + j
    const combos: Array<readonly string[]> = [[], ...c.ids.map((id: string) => [id]), [...c.ids]]
    for (const removed of combos) {
      c.render(panelsWith(removed, place, leftSide, c.layout, extras))
      await settle()
      if (removed.length === 0 && order().join() !== stock.join()) differs++
      // The added panes are on screen with nothing hidden, wherever the placement put them.
      if (removed.length === 0) for (const id of extras ?? []) expect(order(), `${c.cockpit}, placement #${i}: ${id} was added but is not on screen`).toContain(id)
      const on = stopsOnScreen(c.stopControls)
      for (const [label] of c.stopControls) {
        const els = on.get(label)!
        const where = `${c.cockpit}, placement #${i} ${JSON.stringify(leftSide ? { place, leftSide } : place)}, hiding {${removed.join(', ')}}`
        expect(els.length, `${where} took "${label}" with it`).toBeGreaterThan(0)
        expect(els.some((e) => !e.disabled), `${where} left "${label}" on screen but DISABLED`).toBe(baseline.get(label))
      }
      cleanup()
    }
  }
  // The placements reached the region: most of them render in an order the stock one does not.
  expect(differs, `${c.cockpit}: the random placements left the region in its stock order — the sweep is reading nothing`).toBeGreaterThan(4)
}

/** The arrangement sweep's budget, per run of ten placements (2026-10-07): a fresh mount per hide (17 a
 *  placement for Phone, 19 for CW, 15 for JS8), each followed by one accessible-name pass over every
 *  button, and the time grows in step with the CPU share. A run took, alone, 7.7 s for Phone, 9.5 s for CW
 *  and 3.8 s for JS8; at a fifth of a CPU 44 s (Phone) and 51 s (CW); at a tenth 97 s and 111 s. Run as one
 *  test of fifty, CW at a fifth of a CPU ran out of this same 240 s, where it took 132 s before the boxes.
 *  The first budget (2026-10-02, one run of fifty, eleven hides for Phone): Phone 15 s alone, 57 s in the
 *  full suite, 83 s at a fifth and 160 s at a tenth. Only one of Phone's 50 placements repeats, so there is
 *  no repeated render left to skip. At 1 ms every run times out. */
export const ARRANGEMENT_RUN_BUDGET_MS = 240_000

/** FT's arrangement sweep's budget, per run of ten placements (2026-10-07): seventeen mounts a placement
 *  (its fifteen ids hidden singly, then none and all), each with the QSO strip, the header, the callsign
 *  card and six boxes, and one accessible-name pass. The first run of each layout (the heaviest) took,
 *  alone, 12.6 s for Classic and 8.9 s for Roster; at a fifth of a CPU 43.9 s and 43.7 s; at a tenth
 *  113.9 s and 93.4 s — CW's figures, and the same 240 s keeps the same headroom.
 *  Re-measured 2026-10-08, when each layout's mounts gained the other layout's panes (Classic the Call
 *  Roster, Roster the Tx messages and Stations): the heaviest run is now placements 31–40, alone 13.7 s for
 *  Classic and 14.0 s for Roster, and Roster's at a tenth of a CPU 104.8 s, so 240 s still leaves 2.3×. */
export const OPERATE_RUN_BUDGET_MS = 240_000

/** `n` arrangements of Phone's spec with its LEFT SIDE in play (2026-10-03), alternately menu-built
 *  (every move made as on a window wide enough for the side, so ◀ puts listed panes there and ▶ takes
 *  them back) and a coerced junk record (any id, stop controls included, on the side). */
export function randomArrangements<P extends string>(spec: ArrangeSpec<P>, n: number, seed: number): Array<Arrangement<P>> {
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

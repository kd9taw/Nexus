// @vitest-environment jsdom
//
// A NEED CHIP READS IN EVERY THEME, WHEREVER IT SITS (operator, 2026-09-30, "Extend app-wide": "One rule and the same kind
// of guard, so every need chip reads the same in light themes"; then "Same rule in dark").
//
// A need chip (NEW ONE, ZONE, BAND, MODE, GRID, STATE, LoTW, DXPED, POTA, SOTA, WATCH) was lettered in its need colour on a
// 24 % tint of that colour, and lettered on its own tint it read under the 4.5:1 floor in both themes: as low as 1.37:1 in
// the light one and 2.49:1 in the dark one, in Chrome. Connect's chips took the provenance chip's treatment first: the
// word in the theme's ink, and the need on the chip's border at full strength. The same rule now holds for every chip in
// the app, in every theme. On a Band Activity row tinted in its own need's colour the chip drops its tint and is an outline
// in that colour (operator, 2026-09-30: "Chip drops its tint there"): stacked on the row's tint, the chip's own made WATCH
// on a watched station's row read 3.12:1 in the night modes, in Chrome. And at night a chip's own tint is 10 % of its need
// colour (operator, 2026-09-30: "10 % fill"), so the dim night ink reads on a row that is tinted, selected or dimmed.
//
// THE HOSTS. Nine components letter a need chip, and each is rendered here in its host's own chain: Band Activity
// (OperateDecodes: Operate's Band Activity and Rx Frequency panes, and the Tempo rail), the Call Roster (OperateRoster), the
// station cards (StationCard, via StationList: Tempo's Stations rail and Operate's Stations pane), the WATCH tile
// (WatchTile: on the roster, the cards and Spots, in the view, in Phone's pane and in Connect's Spots box, in Connect and in
// the dashboard rail beside the cockpits), the Needed board (NeededPanel: the view,
// Phone's pane and the pop-out), the Satellites section's earn chips (the schedule, the Next-up strip and the pass timeline,
// in the view and the pop-out), and Connect's Selection, Chase and Chase-tonight panes, in Connect and in the dashboard
// rail. Every chip that letters a need
// by its tag is then swept through all eleven need classes (a row that letters one need can letter any of them), and
// each is measured on what it sits on in its host.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { ReactNode } from 'react'
import { OperateDecodes } from './OperateDecodes'
import { OperateRoster } from './OperateRoster'
import { StationList } from './StationList'
import { NeededPanel } from './NeededPanel'
import { SpotsPanel } from './SpotsPanel'
import { SatellitesView } from './SatellitesView'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PaneFrame } from './connect/PaneFrame'
import type { PaneContext } from './connect/paneContext'
import type { PaneId } from '../features/connectConfig'
import { NEED_TIER } from '../features/needs'
import { saveWatchlist } from '../watchlist'
import { SKINS } from '../features/skins'
import {
  BASE_MODES,
  PALETTE_SETS,
  SENTINEL_MODES,
  baseTheme,
  isNight,
  chainOf,
  cmpSpec,
  compoundMatches,
  contrast,
  expandWith,
  parseRules,
  reachesChain,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../cssCascade'
import type {
  BandOutlook,
  DecodeRow,
  NeedAlert,
  NeedTag,
  PathPrediction,
  PropagationSnapshot,
  SatDetail,
  SatPass,
  SatView,
  SpotRow,
  Station,
} from '../types'
import FIXTURE from '../remote-web/__fixtures__/navigation-connect.json'

const NOW = Math.floor(Date.now() / 1000)

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  openQrzPage: vi.fn(async () => {}),
  getDeclination: vi.fn(async () => null),
  getSatellites: vi.fn(async () => SAT_VIEW),
  getSatSchedule: vi.fn(async () => SAT_PASSES),
  getSatPassNeeds: vi.fn(async () => SAT_PASSES),
  getSatDetail: vi.fn(async () => SAT_DETAIL),
  getSettings: vi.fn(async () => ({ mygrid: 'EN52', rotatorModel: 0, rotatorHost: '', satDopplerOff: false, satVfoMap: 'main-down-sub-up' })),
  setSettings: vi.fn(async () => ({})),
  setSatTransponder: vi.fn(async () => {}),
  getSatTransponder: vi.fn(async () => null),
  startSatTrack: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => {}),
  getSatTrackStatus: vi.fn(async () => null),
  getContests: vi.fn(async () => []),
  getDxpedWindows: vi.fn(async () => []),
}))
vi.mock('./MapView', () => ({ MapView: () => null }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (f: () => Promise<unknown>) => f()) }))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

// ── the data: every host with chips of several needs, on each kind of row it has ────────────────────────────────
const SLOT = 100
const station = (call: string, over: Partial<Station> = {}): Station =>
  ({ call, grid: 'EN52', snr: -10, lastHeardSlot: SLOT, heardCount: 1, presence: 'active', worked: false, ...over }) as Station
const alert = (call: string, tags: NeedTag[], over: Partial<NeedAlert> = {}): NeedAlert =>
  ({ call, entity: 'United States', band: '20m', zone: 4, tags, priority: NEED_TIER[tags[0]], headline: '', mode: 'FT8', freqMhz: 14.074, ...over }) as NeedAlert

/** Digital needs, by call (the roster, the station cards and Band Activity read them through the same map). */
const ALERTS: NeedAlert[] = [
  alert('VP8PJ', ['Wanted', 'NewEntity', 'NewZone'], { entity: 'Falkland Islands', zone: 13 }),
  alert('JA1ABC', ['NewBand', 'NewMode'], { entity: 'Japan', zone: 25 }),
  alert('K7ABC', ['NewState', 'NewGrid']),
  alert('W1AW', ['Confirm']),
  alert('K4PRK', ['NewPark', 'Pota']),
  alert('W7SUM', ['Sota']),
  alert('3Y0J', ['NewEntity', 'Dxped'], { entity: 'Bouvet', zone: 38 }),
]
/** Needs only Band Activity letters (no card, roster row or board row): rows led by the tinted needs the rest leave out. */
const BA_ALERTS: NeedAlert[] = [
  alert('VK0ZN', ['NewZone', 'NewMode'], { entity: 'Heard Island', zone: 39 }),
  alert('ZS6MD', ['NewMode'], { entity: 'South Africa', zone: 38 }),
]
const byCall = (alerts: NeedAlert[]) => new Map(alerts.map((a) => [a.call, [a]]))
const leadByCall = (alerts: NeedAlert[]) => new Map(alerts.map((a) => [a.call, a.tags[0]]))
const STATIONS: Station[] = [
  station('VP8PJ', { country: 'Falkland Islands', grid: null }),
  station('JA1ABC', { country: 'Japan', grid: 'PM95' }),
  station('K7ABC', { country: 'United States', grid: 'DN31' }),
  // Worked before, on another band: its row and card are dimmed, and its LoTW chip with them.
  station('W1AW', { country: 'United States', grid: 'FN31', worked: true, workedBand: false }),
  station('K4PRK', { country: 'United States', grid: 'EM73' }),
  station('W7SUM', { country: 'United States', grid: 'DM43' }),
  station('3Y0J', { country: 'Bouvet', grid: null }),
]
const decode = (from: string, over: Partial<DecodeRow> = {}): DecodeRow =>
  ({ from, snr: -12, dtSec: 0.2, freqHz: 1200, message: `CQ ${from} FN31`, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: 0, ...over }) as DecodeRow
/** One decode per row colour Band Activity has for a station with a need (`rowClass`). */
const DECODES: DecodeRow[] = [
  decode('VP8PJ', { freqHz: 400 }), // the watch list's lime row
  decode('JA1ABC', { freqHz: 600 }), // a band need's row (and, selected, the selection's)
  decode('K7ABC', { freqHz: 800, isCq: false, message: 'W9XYZ K7ABC EN52' }), // a state need's row
  decode('W1AW', { freqHz: 1000, isCq: false, message: 'K1XX W1AW R-12' }), // a confirmation's row
  decode('K4PRK', { freqHz: 1190 }), // a needed park's CQ: the park colours the row (the POTA badge alone never would)
  decode('W7SUM', { freqHz: 1210, isCq: false, message: 'K1XX W7SUM -10' }), // a plain new row
  decode('3Y0J', { freqHz: 1400, isCq: false, directedToMe: true, message: 'W9XYZ 3Y0J -05' }), // calling me
  decode('K1GRD', { freqHz: 1600, newGrid: true }), // the decode's own new-grid flag
  decode('ZL9ZZ', { freqHz: 1800, newDxcc: true }), // the decode's own new-one flag: a NEW ONE row
  decode('VK0ZN', { freqHz: 2000 }), // a zone need's row
  decode('ZS6MD', { freqHz: 2200 }), // a mode need's row
]
const PHONE_ALERTS: NeedAlert[] = [
  alert('VP8PJ', ['Wanted', 'NewEntity'], { entity: 'Falkland Islands', mode: 'SSB', freqMhz: 14.2 }),
  alert('JA1ABC', ['NewBand', 'NewMode'], { entity: 'Japan', mode: 'SSB', freqMhz: 14.21 }),
  alert('K4PRK', ['NewPark', 'Pota'], { mode: 'SSB', freqMhz: 14.28 }),
]
const spot = (call: string, over: Partial<SpotRow> = {}): SpotRow =>
  ({ call, entity: 'United States', zone: 5, state: null, band: '20m', freqMhz: 14.025, mode: 'CW', submode: 'CW', spotter: 'W3LPL', corroborators: [],
    ageSecs: 60, comment: 'UP 2', licensed: true, spotterLocal: true, ...over }) as SpotRow
const SPOTS: SpotRow[] = [spot('VP8PJ', { entity: 'Falkland Islands' }), spot('3Y0J', { entity: 'Bouvet', freqMhz: 14.03 })]
const PHONE_SPOTS: SpotRow[] = [
  spot('VP8PJ', { entity: 'Falkland Islands', mode: 'Phone', submode: null, freqMhz: 14.2 }),
  spot('3Y0J', { entity: 'Bouvet', mode: 'Phone', submode: null, freqMhz: 14.21 }),
]

const SAT_EARN = { newGrids: 5, gridSample: ['EN12', 'EN13'], newEntities: 2, entitySample: ['Palau'], score: 1005 }
const SAT_PASSES: SatPass[] = [
  { name: 'AO-91', aosUnix: NOW + 600, losUnix: NOW + 1200, maxElDeg: 30, aosAzDeg: 200, losAzDeg: 320, status: 'alive', earn: null },
  { name: 'RS-44', aosUnix: NOW + 720, losUnix: NOW + 1500, maxElDeg: 62, aosAzDeg: 100, losAzDeg: 260, status: 'alive', earn: SAT_EARN },
]
const SAT_VIEW = {
  tleAgeDays: 1, usableCount: 300, agingCount: 0, heldBackCount: 0, tleFetchedAt: NOW, tleSource: 'mirror',
  birds: [
    { name: 'RS-44', norad: 44909, lat: 0, lon: 0, altKm: 500, footprintKm: 2000, track: [], status: 'alive', amateur: true },
    { name: 'AO-91', norad: 43017, lat: 0, lon: 0, altKm: 500, footprintKm: 2000, track: [], status: 'alive', amateur: true },
  ],
  passes: SAT_PASSES.map((p) => ({ ...p, earn: undefined })),
  excluded: [],
} as unknown as SatView
const SAT_DETAIL = {
  name: 'RS-44', norad: 44909, status: 'alive', transmitters: [], dataFetchedAt: NOW, pass: SAT_PASSES[1],
  passTrack: [[NOW + 720, 100, 0], [NOW + 1110, 180, 62], [NOW + 1500, 260, 0]],
} as unknown as SatDetail

// Connect's three need panes, on the context the panes read (the station selected, the needs, the band outlook).
const hourly = (v: number) => Array.from({ length: 24 }, () => v)
const OUTLOOK_BANDS: BandOutlook[] = [
  { band: '20m', workability: 'Good', score: 0.7, window: '0000–2400Z', grayline: false, hourly: hourly(0.7), reliability: 80 },
  { band: '17m', workability: 'Fair', score: 0.4, window: '1200–2000Z', grayline: false, hourly: hourly(0.4), reliability: 60 },
]
const OUTLOOK: PathPrediction = { engine: 'p533', bands: OUTLOOK_BANDS, mufNow: 14.2, mufHourly: hourly(14) }
const BASE_PROP = FIXTURE.prop as unknown as PropagationSnapshot
const PROP: PropagationSnapshot = {
  ...BASE_PROP,
  source: 'live',
  asOf: NOW,
  dxpeditions: {
    workableNow: [
      { call: '3Y0K', entity: 'Bouvet', need: 'Atno', band: '20m', bearingDeg: 150, octant: 'SE', distanceKm: 13000, status: 'WorkNow',
        likelihood: 'Good', likelihoodScore: 0.7, liveConfirmed: true, howToCall: '', windowHint: '', priority: 9, modes: ['CW'] },
    ],
    active: ['3Y0K'],
    upcoming: [{ call: '3Y0K', entity: 'Bouvet', region: 'Antarctica', startUnix: NOW - 10 * 86400, endUnix: NOW + 86400, bands: ['20m'],
      modes: ['CW'], octant: 'SE', bearingDeg: 150, distanceKm: 13000, outlook: [], best: '' } as never],
  } as never,
}
const CONNECT_NEEDS: NeedAlert[] = [
  alert('VP8XYZ', ['NewEntity'], { entity: 'Falkland Islands', zone: 13, admittedAt: NOW - 60 }),
  alert('JA1ABC', ['NewBand'], { entity: 'Japan', band: '17m', zone: 25, mode: 'CW', freqMhz: 18.08, admittedAt: NOW - 120 }),
  alert('ZS6XX', ['NewMode'], { entity: 'South Africa', band: '20m', zone: 38, mode: 'SSB', freqMhz: 14.3, admittedAt: NOW - 180 }),
  alert('K7XYZ', ['NewState'], { admittedAt: NOW - 240 }),
]
const connectCtx = (): PaneContext =>
  ({
    myGrid: 'EN52', entityCentroids: null, theme: 'dark', intent: 'dx',
    prop: PROP, prov: { label: 'LIVE', cls: 'live' },
    needByCall: new Map([['VP8XYZ', 'NewEntity']]), needAlerts: CONNECT_NEEDS,
    selectedCall: 'VP8XYZ', selStation: null, selSpot: null, selDxped: null, selDxpedWindow: null, dxpedWindows: new Map(), selGrid: null,
    pathPred: OUTLOOK, bandOutlook: OUTLOOK, pathOpen: OUTLOOK_BANDS, outlookOpen: OUTLOOK_BANDS,
    getout: null, focusBand: null, amp: null, scales: null, alerts: [], muf: [],
    // The Spots box is the Spots board itself, with the Spots view's rows (a watched station's WATCH tile).
    spotsFeed: { rows: SPOTS, board: { bandPlan: [], selectedCall: 'VP8PJ', onSelect: () => {}, onWork: () => {} } },
    onSelectCall: () => {}, toggleFocusBand: () => {},
  }) as unknown as PaneContext

// ── the hosts, each in its own chain ─────────────────────────────────────────────────────────────────────────────
const noop = () => {}
const decodesProps = { decodes: DECODES, slot: SLOT, band: '20m', tier: 'FT8' as const, harqRescues: 0, onCall: noop, needAlertsByCall: byCall([...ALERTS, ...BA_ALERTS]), myGrid: 'EN52' }
const stationList = () => (
  <StationList
    stations={STATIONS} myGrid="EN52" currentSlot={SLOT} activePeer="JA1ABC" unreadByPeer={{}} needByCall={leadByCall(ALERTS)}
    needAlertsByCall={byCall(ALERTS)} band="20m" feedMode="FT8" onSelect={noop} onCall={noop} conversations={[]} onArchive={noop}
    bandActive={false} bandUnread={0} onSelectBand={noop}
  />
)
const needed = (alerts: NeedAlert[], pane?: { filterKey: string; modes: ['Phone'] }) => (
  <NeededPanel alerts={alerts} bandPlan={[]} selectedCall="JA1ABC" myGrid="EN52" onQsy={noop} onSelect={noop} pane={pane} />
)
const operate = (lower: ReactNode) => (
  <div className="app">
    <div className="operate-host">
      <main className="layout single operate-cockpit">{lower}</main>
    </div>
  </div>
)
const tempo = (cell: ReactNode) => (
  <div className="app">
    <main className="layout" data-three-pane="">{cell}</main>
  </div>
)
const phonePane = (paneId: string, title: string, body: ReactNode) => (
  <div className="app">
    <main className="layout single phone-cockpit">
      <div className="cockpit-panes">
        <div className="cockpit-col">
          <CockpitPaneFrame title={title} paneId={paneId}>
            <div className="np-pane">{body}</div>
          </CockpitPaneFrame>
        </div>
      </div>
    </main>
  </div>
)
const connectPane = (host: 'rail' | 'strip', paneId: PaneId) => {
  const frame = <PaneFrame slotId={host === 'rail' ? 'left1' : 'bottom1'} paneId={paneId} ctx={connectCtx()} onAssign={noop} onHide={noop} />
  return (
    <div className="app">
      <main className="layout single">
        <div className="connect-shell">
          <div className="connect" data-rails="both">
            {host === 'rail' ? <div className="connect-rail" data-side="left">{frame}</div> : <div className="connect-strip">{frame}</div>}
          </div>
        </div>
      </main>
    </div>
  )
}

/** Connect's boxes in the dashboard rail beside a cockpit (components/DashRail): App's shell, the rail, its column. */
const dashPane = (paneId: PaneId) => (
  <div className="app">
    <div className="shell" data-dash-rail="on">
      <aside className="dash-rail">
        <div className="dash-rail-col dash-boxes">
          <PaneFrame slotId="rail1" slotName="rail1" paneId={paneId} ctx={connectCtx()} onAssign={noop} onHide={noop} share={1} />
        </div>
      </aside>
    </div>
  </div>
)

/** Each host, and whether a chip there can letter any need (a chip drawn from the need it names), or only its own (the
 *  WATCH tile alone in a Spots row; the Satellites section's NEW ONE and GRID). */
const HOSTS: Array<[string, () => ReactNode, 'any' | 'own']> = [
  ['Operate Band Activity', () => operate(
    <div className="cockpit-lower classic" data-cols="three">
      <div className="cockpit-decodes panel"><OperateDecodes {...decodesProps} rxOffsetHz={1200} selectedCall="JA1ABC" /></div>
    </div>,
  ), 'any'],
  ['Operate Rx Frequency', () => operate(
    <div className="cockpit-lower classic" data-cols="three">
      <div className="cockpit-qsocol">
        <div className="cockpit-rxfreq panel"><OperateDecodes {...decodesProps} rxOffsetHz={1200} lockedFilter="rx" compact /></div>
      </div>
    </div>,
  ), 'any'],
  ['Operate roster-layout Band Activity', () => operate(
    <div className="cockpit-lower roster">
      <aside className="cockpit-side">
        <div className="cockpit-decodes-side panel"><OperateDecodes {...decodesProps} rxOffsetHz={1200} /></div>
      </aside>
    </div>,
  ), 'any'],
  ['Operate Call Roster', () => operate(
    <div className="cockpit-lower roster">
      <div className="cockpit-roster-main panel">
        <OperateRoster
          stations={STATIONS} myGrid="EN52" currentSlot={SLOT} needByCall={leadByCall(ALERTS)} needAlertsByCall={byCall(ALERTS)} band="20m"
          feedMode="FT8" selectedCall="JA1ABC" workingCall="K7ABC" onSelect={noop} onCall={noop}
        />
      </div>
    </div>,
  ), 'any'],
  ['Operate Stations pane', () => operate(
    <div className="cockpit-lower classic" data-cols="three">
      <div className="cockpit-roster panel">{stationList()}</div>
    </div>,
  ), 'any'],
  ['Tempo Stations rail', () => tempo(<div className="grid-stations">{stationList()}</div>), 'any'],
  ['Tempo Band Activity rail', () => tempo(
    <div className="grid-waterfall">
      <aside className="right-rail panel"><OperateDecodes {...decodesProps} rxOffsetHz={1200} compact /></aside>
    </div>,
  ), 'any'],
  ['the Needed view', () => <div className="app">{needed(ALERTS)}</div>, 'any'],
  ['the Needed pop-out', () => <div className="app detached">{needed(ALERTS)}</div>, 'any'],
  ['Phone Needed pane', () => phonePane('needed', 'Needed', needed(PHONE_ALERTS, { filterKey: 'nexus.phone.neededFilters', modes: ['Phone'] })), 'any'],
  ['the Spots view', () => <div className="app"><SpotsPanel spots={SPOTS} bandPlan={[]} selectedCall="VP8PJ" onSelect={noop} onWork={noop} /></div>, 'own'],
  ['Phone Spots pane', () => phonePane('spots', 'Spots', (
    <SpotsPanel spots={PHONE_SPOTS} bandPlan={[]} selectedCall={null} onSelect={noop} onWork={noop} pane={{ scope: 'phone', modes: ['Phone'], band: '20m' }} />
  )), 'own'],
  ['the Satellites view', () => <div className="app"><main className="layout single"><SatellitesView focusSat="RS-44" /></main></div>, 'own'],
  ['the Satellites pop-out', () => <div className="app detached"><SatellitesView focusSat="RS-44" /></div>, 'own'],
  ['Connect Selection (rail)', () => connectPane('rail', 'selection'), 'any'],
  ['Connect Chase (rail)', () => connectPane('rail', 'chase'), 'any'],
  ['Connect Chase tonight (rail)', () => connectPane('rail', 'chaseFeed'), 'any'],
  ['Connect Chase tonight (strip)', () => connectPane('strip', 'chaseFeed'), 'any'],
  ['Connect Spots box (rail)', () => connectPane('rail', 'spots'), 'own'],
  ['Connect Selection (dashboard rail)', () => dashPane('selection'), 'any'],
  ['Connect Chase (dashboard rail)', () => dashPane('chase'), 'any'],
  ['Connect Chase tonight (dashboard rail)', () => dashPane('chaseFeed'), 'any'],
  ['Connect Spots box (dashboard rail)', () => dashPane('spots'), 'own'],
]

/** Every need class the sheet colours a chip with (`.need-<cls> { --need-color }`), and the eleven NEED_CHIP draws. */
const NEED_CLASSES = ['entity', 'zone', 'band', 'mode', 'grid', 'state', 'confirm', 'dxped', 'pota', 'sota', 'watch']

interface Chip {
  host: string
  needs: 'any' | 'own'
  what: string
  cls: string
  nodes: Element[]
  chain: El[]
}
const BODY: El = { tag: 'body', classes: [], attrs: {} }
/** The ancestor-or-self elements below <body>, outermost first, parallel to chainOf. */
function nodesOf(node: Element): Element[] {
  const out: Element[] = []
  for (let n: Element | null = node; n && n.tagName !== 'BODY' && n.tagName !== 'HTML'; n = n.parentElement) out.unshift(n)
  return out
}
const needOf = (classes: readonly string[]) => classes.find((c) => c.startsWith('need-') && c !== 'need-chip')?.slice(5) ?? ''
/** The row a chip sits in, for the report: the nearest ancestor with classes of its own that are not the chip's cell. */
function rowOf(nodes: Element[]): string {
  for (let i = nodes.length - 2; i >= 0; i--) {
    const c = [...nodes[i].classList]
    if (c.some((x) => /row|card|best|earn|sel|chase/.test(x))) return `.${c.join('.')}`
  }
  return ''
}

async function renderHosts(): Promise<Chip[]> {
  const out: Chip[] = []
  for (const [host, make, needs] of HOSTS) {
    localStorage.clear()
    sessionStorage.clear()
    localStorage.setItem('nexus.sats.chasing', JSON.stringify(['RS-44', 'AO-91']))
    saveWatchlist([
      { id: 'w-vp8', kind: 'call', value: 'VP8*' },
      { id: 'w-3y', kind: 'dxcc', value: 'Bouvet' },
    ])
    let r!: ReturnType<typeof render>
    await act(async () => {
      r = render(<>{make()}</>)
    })
    // The self-fetching hosts (the Satellites section) answer over the next ticks.
    for (let k = 0; k < 6; k++)
      await act(async () => {
        await new Promise((res) => setTimeout(res, 0))
      })
    for (const el of r.container.querySelectorAll('.need-chip')) {
      if (el.closest('[hidden]')) continue
      const nodes = nodesOf(el)
      const cls = needOf([...el.classList])
      out.push({ host, needs, cls, what: `${host} ${rowOf(nodes)} .need-${cls} "${(el.textContent ?? '').trim()}"`, nodes, chain: [BODY, ...chainOf(el)] })
    }
    cleanup()
  }
  return out
}

/** One chip per shape: two with the same chain and the same inline styles resolve alike in every mode. */
const shapeOf = (c: Chip) => chainKey(c.chain) + JSON.stringify(c.nodes.map((n) => (n as HTMLElement).style?.cssText ?? ''))
/** Each chip that can letter any need, in each of the eleven: the same element, lettering another need. */
function everyNeed(chips: Chip[]): Chip[] {
  const out = new Map<string, Chip>()
  for (const c of chips)
    for (const cls of c.needs === 'any' ? NEED_CLASSES : [c.cls]) {
      const own = c.chain[c.chain.length - 1]
      const swapped: El = { ...own, classes: own.classes.map((x) => (x === `need-${c.cls}` ? `need-${cls}` : x)) }
      const chip = { ...c, cls, what: cls === c.cls ? c.what : `${c.what} as .need-${cls}`, chain: [...c.chain.slice(0, -1), swapped] }
      if (!out.has(shapeOf(chip))) out.set(shapeOf(chip), chip)
    }
  return [...out.values()]
}

// ── the cascade, for one chip ────────────────────────────────────────────────────────────────────────────────────
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The rule that takes the lead chip's tint away on its own Band Activity row (a `.decode-row.need-X .need-chip.need-X`). */
const LEAD = /^\.decode-row\.(need-[a-z]+) \.need-chip\.\1$/
/** The Band Activity rows the sheet tints in a need's colour (`.decode-row.need-X` with a background of its own). */
const TINTED_ROWS = [
  ...new Set(
    RULES.filter((r) => /^\.decode-row\.need-[a-z]+$/.test(r.selector) && r.decls.some((d) => d.prop === 'background' || d.prop === 'background-color')).map(
      (r) => r.selector.slice('.decode-row.'.length),
    ),
  ),
].sort()
/** The tinted Band Activity row a chip sits in, by its need class, or null. */
const rowNeedOf = (w: Chip) => w.chain.find((e) => e.classes.includes('decode-row'))?.classes.find((c) => TINTED_ROWS.includes(c)) ?? null
/** The chip that leads its row: on a row tinted in its own need's colour. */
const isLead = (w: Chip) => rowNeedOf(w) === `need-${w.cls}`
/** The chip as it shipped, lettered in its need colour with its border a mix of it: the rules that lettered it in ink
 *  before this rule was the chip's own (the light themes' and NEW ONE's), the lead chip's outline and the night tint
 *  removed, and its shipped declarations restated after the sheet, so each outranks what the sheet now says in its place. */
const AS_SHIPPED = `
.need-chip { color: var(--need-color, var(--accent)); border: 1px solid color-mix(in srgb, var(--need-color, var(--accent)) 70%, transparent); }
.need-chip.need-dxped { color: #38bdf8; border: 1px solid color-mix(in srgb, #38bdf8 50%, transparent); }
.need-chip.need-state { color: var(--need-state); border: 1px solid color-mix(in srgb, var(--need-state) 48%, transparent); }
.need-pota { --need-color: #16a34a; }
.need-sota { --need-color: #9333ea; }
`
const SHIPPED = [
  ...RULES.filter(
    (r) =>
      !(r.selector.startsWith("[data-theme='light'] ") && /\.need-(chip|pota|sota)\b/.test(r.selector)) &&
      !(r.selector === '.need-chip.need-entity' && r.decls.some((d) => d.prop === 'color' && d.value === 'var(--text)')) &&
      !LEAD.test(r.selector) &&
      !(r.selector.startsWith("[data-night='1'] ") && /\.need-chip\b/.test(r.selector)),
  ),
  ...parseRules(AS_SHIPPED, { n: RULES.length }),
]

const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every built-in light theme: the four light modes, bare and on each light theme, and the standard light theme under each
 *  set of colour-role presets (none of which moves a need colour, the ink or a surface). */
const LIGHT: Mode[] = [
  ...BASE_MODES.filter((b) => baseTheme(b) === 'light').flatMap((b) => skinsOf('light').map((skin): Mode => (skin ? `${b} skin=${skin}` : b))),
  ...PALETTE_SETS.map((set): Mode => `light ${set}`),
]
/** The four dark modes, and the standard one on the dark themes MODES carries as the worst case (SENTINEL_MODES). */
const DARK: Mode[] = [...BASE_MODES.filter((b) => baseTheme(b) === 'dark'), ...SENTINEL_MODES.filter((m) => m.startsWith('dark skin='))]

const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-[\w-]+|data-radix[\w-]*|data-testid)$/.test(k))]))
/** The custom properties a node sets inline (the resolver reads no inline style). */
const inlineVars = (nodes: Element[], upTo: number) =>
  nodes.slice(0, upTo + 1).flatMap((n) => {
    const s = (n as HTMLElement).style
    return s ? [...s].filter((p) => p.startsWith('--')).map((p) => [p, s.getPropertyValue(p).trim()] as [string, string]) : []
  })
const memo = new WeakMap<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
/** A preset declares only custom properties, so the mode without its presets decides a winner. */
const winnerMode = (mode: Mode): Mode => mode.split(' ').filter((p, i) => i === 0 || p.startsWith('skin=')).join(' ') as Mode
const KEYS = new WeakMap<Chip, { shape: string[]; vars: string[] }>()
function keysOf(w: Chip) {
  let k = KEYS.get(w)
  if (!k) {
    const shape = w.chain.map((_, i) => chainKey(w.chain.slice(0, i + 1)))
    const vars = w.chain.map((_, i) => JSON.stringify(inlineVars(w.nodes, i - 1))) // chain has <body> first: node k is chain k+1
    KEYS.set(w, (k = { shape, vars }))
  }
  return k
}
/** Only the rules that declare a property can win it: winnerAt over those alone is the same answer. */
const WITH = new WeakMap<Rule[], Map<string, Rule[]>>()
function rulesWith(rules: Rule[], props: string[]): Rule[] {
  let byProps = WITH.get(rules)
  if (!byProps) WITH.set(rules, (byProps = new Map()))
  const key = props.join()
  let out = byProps.get(key)
  if (!out) byProps.set(key, (out = rules.filter((r) => r.decls.some((d) => props.includes(d.prop)))))
  return out
}
/** The compound a rule puts on the element itself, split off its selector the way reachesChain splits it. */
const SUBJECT = new WeakMap<Rule, string | undefined>()
function subjectOf(rule: Rule): string | undefined {
  if (!SUBJECT.has(rule)) SUBJECT.set(rule, rule.selector.replace(/\s*>\s*/g, ' > ').split(/\s+/).filter(Boolean).pop())
  return SUBJECT.get(rule)
}
/** And of those, only the rules whose subject matches the element can reach it (reachesChain gives up on any other before it
 *  reads an ancestor or the mode). So the cut is made once per element, not once per theme, and the winner is the same. */
const rulesAt = (rules: Rule[], props: string[], el: El): Rule[] =>
  once(rules, `a|${props.join()}|${JSON.stringify([el.tag, el.classes, el.attrs])}`, () =>
    rulesWith(rules, props).filter((r) => {
      const subject = subjectOf(r)
      return !!subject && compoundMatches(subject, el)
    }),
  )
const winAt = (rules: Rule[], mode: Mode, w: Chip, i: number, ...props: string[]) =>
  once(rules, `w|${winnerMode(mode)}|${keysOf(w).shape[i]}|${props.join()}`, () => winnerAt(rulesAt(rules, props, w.chain[i]), winnerMode(mode), w.chain.slice(0, i + 1), ...props))
const TOKEN_DECLS = new WeakMap<Rule[], Rule[]>()
function rulesWithTokens(rules: Rule[]): Rule[] {
  let out = TOKEN_DECLS.get(rules)
  if (!out) TOKEN_DECLS.set(rules, (out = rules.filter((r) => r.decls.some((d) => d.prop.startsWith('--')))))
  return out
}
/** Tokens at chip.chain[0..i], element by element with cssCascade.tokensAt's own semantics (a token an element inherits
 *  arrives computed; one declared on it resolves against its own tokens), then what the node sets inline. */
function tokensAtIndex(rules: Rule[], mode: Mode, w: Chip, i: number): Map<string, string> {
  const k = keysOf(w)
  return once(rules, `t|${mode}|${k.shape[i]}|${k.vars[i]}`, () => {
    let tokens: Map<string, string>
    if (i === 0) {
      const root = rootTokensFrom(rules, mode)
      tokens = new Map([...root].map(([n, v]) => [n, expandWith(root, v)]))
    } else tokens = tokensAtIndex(rules, mode, w, i - 1)
    const at = w.chain.slice(0, i + 1)
    const declared = new Map<string, { rule: Rule; value: string }>()
    for (const rule of rulesWithTokens(rules)) {
      if (!reachesChain(rule.selector, at, mode)) continue
      for (const d of rule.decls) {
        if (!d.prop.startsWith('--')) continue
        const prev = declared.get(d.prop)
        if (!prev || cmpSpec(rule.spec, prev.rule.spec) > 0 || (cmpSpec(rule.spec, prev.rule.spec) === 0 && rule.order > prev.rule.order))
          declared.set(d.prop, { rule, value: d.value })
      }
    }
    const inline = i >= 1 ? inlineVars([w.nodes[i - 1]], 0) : []
    if (declared.size === 0 && inline.length === 0) return tokens
    const own = new Map(tokens)
    for (const [n, { value }] of declared) own.set(n, value)
    for (const [n, v] of inline) own.set(n, v)
    const computed = new Map(tokens)
    for (const n of [...declared.keys(), ...inline.map(([n]) => n)]) computed.set(n, expandWith(own, own.get(n)!))
    return computed
  })
}
const colourAt = (rules: Rule[], mode: Mode, w: Chip, i: number, value: string, under: Rgb): Rgb => {
  const k = keysOf(w)
  return once(rules, `c|${mode}|${k.shape[i]}|${k.vars[i]}|${value}|${under.join()}`, () => {
    const expanded = expandWith(tokensAtIndex(rules, mode, w, i), value)
    const c = toRgb(expanded, under)
    if (!c) throw new Error(`${w.what} ${mode}: not a colour: "${value}" → "${expanded}"`)
    return c
  })
}
const BLANK = /^(inherit|transparent|none|initial|unset)$/i
const important = (rule: Rule, prop: string) => rule.decls.some((d) => d.prop === prop && /!\s*important\s*$/.test(d.value))
const nodeStyle = (w: Chip, i: number): CSSStyleDeclaration | null => (i >= 1 ? ((w.nodes[i - 1] as HTMLElement).style ?? null) : null)
/** The declaration that paints `prop` at chain index i: an !important author rule, else the node's inline style, else the
 *  cascade winner; null when the element leaves it to its parent. */
function declAt(rules: Rule[], mode: Mode, w: Chip, i: number, prop: string, ...props: string[]): string | null {
  const css = winAt(rules, mode, w, i, prop, ...props)
  if (css && important(css.rule, css.prop)) return css.value
  const inline = nodeStyle(w, i)?.getPropertyValue(prop).trim()
  if (inline) return inline
  return css && !BLANK.test(css.value.trim()) ? css.value : null
}
/** What chain index `from` sits on: every background from it outward, down to the first opaque one, composited. */
function surfaceAt(rules: Rule[], mode: Mode, w: Chip, from: number): Rgb {
  const layers: Array<[number, string]> = []
  for (let i = from; i >= 0; i--) {
    const v = declAt(rules, mode, w, i, 'background', 'background-color')
    if (!v || BLANK.test(v)) continue
    layers.push([i, v])
    if (hex(colourAt(rules, mode, w, i, v, [0, 0, 0])) === hex(colourAt(rules, mode, w, i, v, [255, 255, 255]))) break
  }
  return layers.reduceRight((under, [i, v]) => colourAt(rules, mode, w, i, v, under), colourAt(rules, mode, w, 0, 'var(--bg)', [0, 0, 0]))
}
/** The chip's ink (it inherits) and the dimming every ancestor applies to it. */
function inkOf(rules: Rule[], mode: Mode, w: Chip): { value: string; at: number } {
  for (let i = w.chain.length - 1; i >= 0; i--) {
    const v = declAt(rules, mode, w, i, 'color')
    if (v) return { value: v, at: i }
  }
  throw new Error(`${w.what}: nothing paints it`)
}
function opacityOf(rules: Rule[], mode: Mode, w: Chip): number {
  let o = 1
  for (let i = 0; i < w.chain.length; i++) {
    const v = declAt(rules, mode, w, i, 'opacity')
    if (v) o *= Number(v)
  }
  return o
}
const fade = (c: Rgb, under: Rgb, o: number) => [0, 1, 2].map((k) => Math.round(c[k] * o + under[k] * (1 - o))) as unknown as Rgb
/** A border declaration's colour: the value itself, or a `border` shorthand without its width and style. */
const borderColour = (v: string) => v.replace(/^\s*[\d.]+(px|em|rem)\s+/, '').replace(/^(solid|dashed|dotted|double)\s+/, '').trim()

/** The chip's word on what it sits on (through its row's dimming, where the row dims). */
function wordOf(rules: Rule[], mode: Mode, w: Chip) {
  const ink = inkOf(rules, mode, w)
  const bg = surfaceAt(rules, mode, w, w.chain.length - 1)
  const fg = fade(colourAt(rules, mode, w, ink.at, ink.value, bg), bg, opacityOf(rules, mode, w))
  return { fg, bg, ratio: contrast(fg, bg) }
}
/** The chip's border against what the chip sits on (a border is painted over the chip's own background). */
function edgeOf(rules: Rule[], mode: Mode, w: Chip) {
  const i = w.chain.length - 1
  const v = declAt(rules, mode, w, i, 'border-top-color', 'border-color', 'border')
  if (!v) return null
  const under = surfaceAt(rules, mode, w, i - 1)
  const c = fade(colourAt(rules, mode, w, i, borderColour(v), surfaceAt(rules, mode, w, i)), under, opacityOf(rules, mode, w))
  return { c, under, ratio: contrast(c, under), value: v }
}

function unreadable(rules: Rule[], chips: Chip[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const w of chips)
    for (const mode of modes) {
      const { fg, bg, ratio } = wordOf(rules, mode, w)
      if (ratio < 4.5) out.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
    }
  return out
}

describe('a need chip reads in every theme, wherever it sits', () => {
  let found: Chip[] = []
  let natural: Chip[] = []
  let chips: Chip[] = []
  beforeAll(async () => {
    found = await renderHosts()
    natural = [...new Map(found.map((c) => [shapeOf(c), c])).values()]
    chips = everyNeed(natural)
  }, 120_000)

  // THE INVENTORY: every host, with the needs its data here letters. Exact, on purpose: a host that stops rendering its
  // chips here would leave the sweep silently, and a new host is looked at before it joins.
  const ALL = ['band', 'confirm', 'dxped', 'entity', 'grid', 'mode', 'pota', 'sota', 'state', 'watch', 'zone']
  const INVENTORY: Record<string, string[]> = {
    'Operate Band Activity': ALL,
    'Operate Rx Frequency': ['dxped', 'entity', 'pota', 'sota'],
    'Operate roster-layout Band Activity': ALL,
    'Operate Call Roster': ['band', 'confirm', 'entity', 'grid', 'mode', 'pota', 'state', 'watch', 'zone'],
    'Operate Stations pane': ALL,
    'Tempo Stations rail': ALL,
    'Tempo Band Activity rail': ALL,
    'the Needed view': ALL,
    'the Needed pop-out': ALL,
    'Phone Needed pane': ['band', 'entity', 'mode', 'pota', 'watch'],
    'the Spots view': ['watch'],
    'Phone Spots pane': ['watch'],
    'the Satellites view': ['entity', 'grid'],
    'the Satellites pop-out': ['entity', 'grid'],
    'Connect Selection (rail)': ['entity'],
    'Connect Chase (rail)': ['band', 'entity', 'mode', 'state'],
    'Connect Chase tonight (rail)': ['band', 'dxped', 'entity', 'mode', 'state'],
    'Connect Chase tonight (strip)': ['band', 'dxped', 'entity', 'mode', 'state'],
    'Connect Spots box (rail)': ['watch'],
    'Connect Selection (dashboard rail)': ['entity'],
    'Connect Chase (dashboard rail)': ['band', 'entity', 'mode', 'state'],
    'Connect Chase tonight (dashboard rail)': ['band', 'dxped', 'entity', 'mode', 'state'],
    'Connect Spots box (dashboard rail)': ['watch'],
  }
  it('renders need chips on every host, in each need its data letters (the census cannot silently empty out)', () => {
    const seen: Record<string, string[]> = {}
    for (const c of found) (seen[c.host] ??= []).includes(c.cls) || seen[c.host].push(c.cls)
    for (const k of Object.keys(seen)) seen[k].sort()
    expect(seen).toEqual(INVENTORY)
    expect(Object.keys(INVENTORY).sort(), 'every host').toEqual(HOSTS.map(([h]) => h).sort())
    expect(chips.length, 'chips measured').toBeGreaterThan(200)
  })

  it('every chip clears 4.5:1 in every light theme, in each need it can letter, on what it sits on', () => {
    expect(unreadable(RULES, chips, LIGHT)).toEqual([])
  }, 240_000)

  // In the dark themes the same, but in a row that tints or dims (below) 4.3:1: there the night modes' dimmer ink reads 4.35
  // to 4.46:1 on NEW ONE (the operator accepted it: "Take all three"). A worked station's card, dimmed to 72 %, takes a
  // WATCH chip to 4.28:1 on the Lagoon theme (it shipped at 3.51:1), so that card is held to 4.25:1.
  // THE NIGHT MODES (operator, 2026-09-30: "10 % fill"). With the word in the dim night ink, a chip that is not its row's
  // own stacks its tint on a tinted, selected or dimmed row's; at the day's 24 % that took a word down to 3.31:1 (a POTA
  // chip on a watched station's row). At night the tint is 10 % of the need colour, and there the word reads 4.44:1 or
  // better (measured), never worse than it shipped: a chip there is held to 4.4:1 and to what it shipped at. Every other
  // chip clears 4.5:1 at night too.
  const NIGHT_TINTS: Array<[string, (e: El) => boolean]> = [
    ['a Band Activity row calling me', (e) => e.classes.includes('decode-row') && e.classes.includes('directed')],
    ['a selected Call Roster row', (e) => e.classes.includes('or-row') && e.classes.includes('selected')],
    ['a selected station card', (e) => e.classes.includes('station-card') && e.classes.includes('selected')],
  ]
  it('every chip clears 4.5:1 in every dark theme, in each need it can letter, on what it sits on (4.3:1 in a row that tints or dims; at night 4.4:1 in a row that tints, never worse than it shipped)', () => {
    const low: string[] = []
    for (const w of chips)
      for (const mode of DARK) {
        const { fg, bg, ratio } = wordOf(RULES, mode, w)
        const floor = isNight(mode)
          ? tintsOrDims(w) || NIGHT_TINTS.some(([, is]) => w.chain.some(is)) ? 4.4 : 4.5
          : dipOf(w) === "a worked station's card" ? 4.25 : tintsOrDims(w) ? 4.3 : 4.5
        if (ratio < floor) low.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
        if (isNight(mode) && ratio < 4.5) {
          const was = wordOf(SHIPPED, mode, w).ratio
          if (ratio < was - 0.005) low.push(`${w.what} ${mode}: ${ratio.toFixed(2)}:1, worse than it shipped at ${was.toFixed(2)}:1`)
        }
      }
    expect(low).toEqual([])
    expect(NIGHT_TINTS.filter(([, is]) => !chips.some((w) => w.chain.some(is))).map(([name]) => name), 'rows with no chip').toEqual([])
  }, 240_000)

  // THE THREE ROWS A BORDER DIPS IN. Each tints or dims itself, and a need colour at full strength reads 2.7–3.0:1
  // against that in the light themes (measured: the grey LoTW and the amber BAND the lowest): a selected row on the Needed
  // board (the accent's tint), the roster's row for the station being worked (the transmit colour's) and a worked station's
  // card (dimmed to 72 %). The word reads 4.5:1 in them as everywhere, and it names the need. Their borders are held to
  // that floor, and in the dark themes to 2.6:1 (NEW ONE's magenta on a worked card on the Lagoon theme, 2.63:1; it shipped
  // at 1.99:1). In the dark themes, too, a chip on a Band Activity row in another need's colour is held to 2.6:1: NEW ONE's
  // magenta on a watched station's lime row reads 2.62:1 on Slate and Lagoon and 2.93:1 on the standard dark theme (the
  // operator, 2026-09-30, with "Chip drops its tint there": held at 2.6).
  const DIPS: Array<[string, (e: El) => boolean]> = [
    ['a selected Needed-board row', (e) => e.classes.includes('np-row') && e.classes.includes('selected')],
    ["the roster's row for the station being worked", (e) => e.classes.includes('or-row') && e.classes.includes('working')],
    ["a worked station's card", (e) => e.classes.includes('station-card') && e.classes.includes('worked')],
  ]
  const dipOf = (w: Chip) => DIPS.find(([, is]) => w.chain.some(is))?.[0] ?? null
  /** A row that tints itself or dims: the three above, or a Band Activity row in a need's colour. */
  const tintsOrDims = (w: Chip) => dipOf(w) != null || w.chain.some((e) => e.classes.includes('decode-row') && e.classes.some((c) => c.startsWith('need-')))
  it('the need stays on the border at full strength, 3:1 off what the chip sits on in every theme (2.7:1 in the three rows that tint or dim, 2.6:1 in dark there and on another need\'s row)', () => {
    const low: string[] = []
    for (const w of chips)
      for (const mode of [...LIGHT, ...DARK]) {
        const e = edgeOf(RULES, mode, w)
        if (!e) {
          low.push(`${w.what} ${mode}: no border`)
          continue
        }
        if (!e.value.includes('var(--need-color')) low.push(`${w.what} ${mode}: the border is ${e.value}, not the need colour`)
        const dark = DARK.includes(mode)
        const floor = dipOf(w) ? (dark ? 2.6 : 2.7) : dark && rowNeedOf(w) != null && !isLead(w) ? 2.6 : 3
        if (e.ratio < floor) low.push(`${w.what} ${mode}: the border ${hex(e.c)} on ${hex(e.under)} = ${e.ratio.toFixed(2)}:1`)
      }
    expect(low).toEqual([])
    // Each exception is a row the census really has, so the lower floor cannot outlive its row.
    expect(DIPS.filter(([name]) => !chips.some((w) => dipOf(w) === name)).map(([name]) => name), 'rows with no chip').toEqual([])
  }, 240_000)

  // THE LEAD CHIP ON ITS OWN ROW (operator, 2026-09-30: "Chip drops its tint there"). On a Band Activity row tinted in a
  // need's colour, the chip of that need has no tint of its own in any theme (the row carries it), and its border is the
  // need colour at full strength (the border check above); every other chip on a Band Activity row keeps its tint. Every
  // row the sheet tints has its lead chip here, so a row tinted later cannot be left out of the rule.
  it("on a Band Activity row in its own need's colour the chip is an outline in that colour, in every theme; every other chip keeps its tint", () => {
    const wrong: string[] = []
    for (const w of chips) {
      if (!w.chain.some((e) => e.classes.includes('decode-row'))) continue
      const i = w.chain.length - 1
      for (const mode of [...LIGHT, ...DARK]) {
        const tint = declAt(RULES, mode, w, i, 'background', 'background-color')
        if (isLead(w) && tint) wrong.push(`${w.what} ${mode}: on its own row it keeps a tint, ${tint}`)
        if (!isLead(w) && !tint) wrong.push(`${w.what} ${mode}: it has lost its tint`)
      }
    }
    expect(wrong).toEqual([])
    expect(TINTED_ROWS.filter((n) => !chips.some((w) => isLead(w) && rowNeedOf(w) === n)), 'tinted rows with no lead chip').toEqual([])
    expect(TINTED_ROWS, 'the tinted rows').toEqual(['need-band', 'need-confirm', 'need-entity', 'need-grid', 'need-mode', 'need-pota', 'need-state', 'need-watch', 'need-zone'])
  }, 240_000)

  // POTA AND SOTA (operator, 2026-09-30: "SOTA/POTA's fixed colours give way"): a fixed green and purple, set for the
  // Needed board's chips, overrode the theme's own in the dark themes. Each chip now takes the theme's colour in every theme.
  it("the POTA and SOTA chips take the theme's own colours in every theme", () => {
    const wrong: string[] = []
    for (const w of chips.filter((c) => c.cls === 'pota' || c.cls === 'sota'))
      for (const mode of [...LIGHT, ...DARK]) {
        const i = w.chain.length - 1
        const got = hex(colourAt(RULES, mode, w, i, 'var(--need-color)', [0, 0, 0]))
        const want = hex(colourAt(RULES, mode, w, i, `var(--need-${w.cls})`, [0, 0, 0]))
        if (got !== want) wrong.push(`${w.what} ${mode}: ${got}, not the theme's ${want}`)
      }
    expect(wrong).toEqual([])
  }, 120_000)

  it('FIRES: the chip as it shipped is caught in the light theme on every host, at the ratios Chrome measured', () => {
    const shipped = unreadable(SHIPPED, natural, ['light'])
    const hosts = new Set(shipped.map((m) => HOSTS.find(([h]) => m.startsWith(h))?.[0]))
    expect([...hosts].sort()).toEqual(HOSTS.map(([h]) => h).sort())
    // The resolver must agree with what Chrome painted, or it is not measuring the same thing (Chrome, the real app
    // in the standard light theme: the same hex pairs; a ratio a unit apart where the page's mix rounds).
    const has = (re: RegExp) => shipped.some((m) => re.test(m))
    expect(has(/^Operate Band Activity \.decode-row\.directed \.need-dxped "DXPED" light: #38bdf8 on #d3ccde = 1\.37:1$/), 'DXPED calling me').toBe(true)
    expect(has(/^Operate Band Activity \.decode-row\.need-watch \.need-watch "WATCH" light: #3f6212 on #acbb9a = 3\.48:1$/), 'WATCH').toBe(true)
    expect(has(/^the Needed view \.np-row\.need-pota \.need-pota "(NEW PARK|POTA)" light: #16a34a on #b3d9c8 = 2\.15:1$/), 'NEW PARK and POTA').toBe(true)
    expect(has(/^the Needed view \.np-row\.need-entity \.need-dxped "DXPED" light: #38bdf8 on #bfe0f2 = 1\.55:1$/), 'DXPED on the board').toBe(true)
    expect(has(/^Operate Call Roster \.or-row\.need-pota \.need-pota "PARK" light: #16a34a on #c4e7d3 = 2\.4[67]:1$/), 'PARK on the roster').toBe(true)
    expect(has(/^Tempo Stations rail \.station-card\.worked\.needed\.need-confirm \.need-confirm "LoTW" light: #8491a3 on #d7dbe2 = 2\.3[01]:1$/), 'LoTW, dimmed').toBe(true)
    expect(has(/^the Satellites view \.sat-earn \.need-grid "GRID ×5" light: #047857 on #c0dcd6 = 3\.7[78]:1$/), 'GRID on the Next-up strip').toBe(true)
  }, 120_000)

  it('FIRES: the chip as it shipped is caught in the dark theme too, at the ratios Chrome measured', () => {
    const shipped = unreadable(SHIPPED, natural, ['dark'])
    const has = (re: RegExp) => shipped.some((m) => re.test(m))
    // Chrome's figures: the same pairs, the surface a unit apart where its mix rounds, the ratio from its unrounded colours.
    expect(has(/^Operate Band Activity \.decode-row\.need-watch \.need-entity "NEW ONE" dark: #f23ec0 on #62464[bc] = 2\.49:1$/), 'NEW ONE on a watched station').toBe(true)
    expect(has(/^Operate Band Activity \.decode-row\.need-watch \.need-zone "ZONE" dark: #c084fc on #56575a = 2\.7[34]:1$/), 'ZONE on it').toBe(true)
    expect(has(/^Operate Band Activity \.decode-row\.new \.need-sota "SOTA" dark: #9333ea on #322358 = 2\.57:1$/), 'SOTA').toBe(true)
    expect(has(/^the Needed view \.np-row\.need-pota \.need-pota "(NEW PARK|POTA)" dark: #16a34a on #0e3323 = 4\.2[0-3]:1$/), 'NEW PARK').toBe(true)
  }, 120_000)
})

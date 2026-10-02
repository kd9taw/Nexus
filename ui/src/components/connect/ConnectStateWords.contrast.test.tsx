// @vitest-environment jsdom
//
// EVERY STATE-COLOURED WORD ON CONNECT READS IN EVERY LIGHT THEME (operator, 2026-09-30: "Give them the
// treatment the chip got overnight (the word in ink, the state on the border), with one computed guard over
// every state-coloured word on Connect").
//
// A word that states a condition (a band's modelled state, a workability, a space-weather impact, a need, a
// warning) is lettered in that state's colour. The colours were tuned as marks on the dark theme, and in
// every light theme they read 1.7–4.3:1 as lettering on the page colour Connect's panes sit on, measured in
// Chrome (Space Wx "moderate flux" #a27000 on #e5eaf0 = 3.58:1, Band Outlook "Good" #007f35 = 4.25:1, the Kp
// outlook's storm line #f5a524 = 1.69:1, a closed band's "Closed" in the Band Advisor 1.65:1 through its
// row's dimming). In the light themes each takes the provenance chip's treatment (and #382's): the word in the
// theme's ink, the state on a border: its own chip's or row's border where it has one, else an underline (a
// word) or a left bar (a line). Dark is untouched. (The Band Advisor has since taken the Band conditions
// list's pill, the word in the theme's ink on the band colour's tint and edge in every theme, so its word is
// no longer state-coloured and left the census.)
//
// WHAT COUNTS AS A STATE-COLOURED WORD. Every pane in the vocabulary is rendered through the real PaneFrame,
// in a rail and in the strip, and the map's insight card in its map chain, with data that puts each pane's
// words in every state they have. A word is any element with its own letters or digits; it is state-coloured
// when, in the standard DARK theme (which this change does not touch, so the census cannot move with the
// fix), its ink is anything but the theme's text inks and its accent. The census is then held to 4.5:1 on
// what it sits on (through any dimming) in every light theme, and to its state colour in every dark theme.
//
// CONNECT'S BOXES OUTSIDE CONNECT, AND THE NOW BAR. The dashboard rail beside the cockpits is a column of
// Connect's boxes in a chain of its own (`.dash-boxes`), so every pane is rendered there too. The Spots and
// POTA/SOTA boxes are the two boards themselves, rendered with rows that put their words in every state. And
// the NOW bar, above every section and so over Connect, letters its chips' words in their state colours: it
// is rendered in each state as well.
//
// A need chip is no longer one: it letters in the ink in every theme, and NeedChip.contrast.test.tsx holds it on
// Connect and on every other host.
import { describe, it, expect, vi, beforeAll, afterAll, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { PaneFrame } from './PaneFrame'
import type { OtaBoard, PaneContext, SpotsFeed } from './paneContext'
import { MapInsightRail } from '../prop/MapInsightRail'
import { NowBar } from '../NowBar'
import { publishBandConditions } from '../../bandConditions'
import { newWatchFilter, saveWatchlist } from '../../watchlist'
import { PANE_IDS, type PaneId, type SlotId } from '../../features/connectConfig'
import { PALETTE_ROLES } from '../../features/paletteRoles'
import { SKINS } from '../../features/skins'
import {
  BASE_MODES,
  SENTINEL_MODES,
  baseTheme,
  chainOf,
  cmpSpec,
  contrast,
  expandWith,
  parseRules,
  reachesChain,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  tokensAt,
  winnerAt,
  withRoles,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from '../../cssCascade'
import type {
  AmpStatus,
  AppSnapshot,
  BandOutlook,
  FeedHealth,
  FeedStatus,
  NeedAlert,
  OtaSpot,
  PathPrediction,
  PropagationSnapshot,
  SpotRow,
} from '../../types'
import FIXTURE from '../../remote-web/__fixtures__/navigation-connect.json'

const NOW = Math.floor(Date.now() / 1000)
const HOUR = Math.floor(NOW / 3600) * 3600

vi.mock('../../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../api')>()),
  getKpForecast: vi.fn(async () => ({
    points: [
      { timeUnix: HOUR - 6 * 3600, kp: 5.3, kind: 'observed', noaaScale: 'G1' },
      { timeUnix: HOUR, kp: 5.67, kind: 'observed', noaaScale: 'G1' },
      { timeUnix: HOUR + 3 * 3600, kp: 6.3, kind: 'predicted', noaaScale: 'G2' },
      { timeUnix: HOUR + 6 * 3600, kp: 3.2, kind: 'predicted', noaaScale: null },
    ],
  })),
  getSatellites: vi.fn(async () => ({
    tleAgeDays: 20, usableCount: 12, agingCount: 0, heldBackCount: 0, tleFetchedAt: NOW - 20 * 86400, tleSource: 'celestrak',
    birds: [], excluded: [],
    passes: [
      { name: 'ISS', norad: 25544, aosUnix: NOW + 600, losUnix: NOW + 1200, maxElDeg: 45, aosAzDeg: 200, losAzDeg: 40, status: 'active' },
      { name: 'AO-91', norad: 43017, aosUnix: NOW + 3600, losUnix: NOW + 4200, maxElDeg: 30, aosAzDeg: 10, losAzDeg: 170, status: 'dead' },
    ],
  })),
  getSatTrackStatus: vi.fn(async () => ({ name: 'ISS', state: 'tracking', mode: 'rotor-only', dopplerDownlink: false, dopplerUplink: false })),
  readRotatorState: vi.fn(async () => ({ azDeg: 120, reading: 'position', elDeg: null, elRange: null })),
  readRotator: vi.fn(async () => 120),
  getSettings: vi.fn(async () => ({ rotatorModel: 2, rotatorHost: '' })),
  getDeclination: vi.fn(async () => null),
  getOpeningsLog: vi.fn(async () => []),
  getContests: vi.fn(async () => []),
  getDxpedWindows: vi.fn(async () => []),
  // The POTA/SOTA box's board fetches its own rows.
  getOtaSpots: vi.fn(async () => OTA),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  // A watched station on the Spots board: its row leads with the WATCH tile. (The box measures no width
  // here, so it stamps no `data-fit` and shows every column.)
  saveWatchlist([newWatchFilter('call', 'K1CW')])
})
afterAll(() => {
  saveWatchlist([])
  publishBandConditions(null)
})
afterEach(cleanup)

// ── the data: every state each pane can letter ────────────────────────────────────────────────────────────
const hourly = (v: number) => Array.from({ length: 24 }, () => v)
const OUTLOOK_BANDS: BandOutlook[] = [
  { band: '40m', workability: 'Excellent', score: 0.92, window: '0000–2400Z', grayline: false, hourly: hourly(0.9), reliability: 90,
    modeNow: [{ mode: 'FT8', score: 0.8 }, { mode: 'CW', score: 0.4 }, { mode: 'SSB', score: 0.1 }] },
  { band: '30m', workability: 'Good', score: 0.7, window: '0000–2400Z', grayline: false, hourly: hourly(0.7), reliability: 80 },
  { band: '20m', workability: 'Fair', score: 0.4, window: '1200–2000Z', grayline: true, hourly: hourly(0.4), reliability: 60 },
  { band: '17m', workability: 'Marginal', score: 0.2, window: '1400–1600Z', grayline: false, hourly: hourly(0.2), reliability: 40 },
  { band: '15m', workability: 'Closed', score: 0.05, window: '', grayline: false, hourly: hourly(0.05), reliability: 20 },
]
const OUTLOOK: PathPrediction = { engine: 'p533', bands: OUTLOOK_BANDS, mufNow: 14.2, mufHourly: hourly(14) }
const MODELED: Array<[string | undefined, string]> = [
  ['Open', 'Active'], ['Marginal', 'Moderate'], ['Closed', 'Closed'], [undefined, 'Active'], [undefined, 'Moderate'], [undefined, 'Quiet'],
]
const BASE_PROP = FIXTURE.prop as unknown as PropagationSnapshot
const PROP: PropagationSnapshot = {
  ...BASE_PROP,
  source: 'live',
  asOf: NOW,
  advisory: {
    ...BASE_PROP.advisory,
    bands: BASE_PROP.advisory.bands.map((b, i) =>
      i < MODELED.length ? { ...b, modeled: MODELED[i][0] as never, tier: MODELED[i][1] as never } : b,
    ),
  },
  spaceWx: { ...BASE_PROP.spaceWx, sfi: 120, kp: 2, aIndex: 8, xrayClass: 'M2.1', flare: true },
  bestToRegion: [
    { region: 'Europe', octant: 'NE', bearingDeg: 45, band: '20m', tier: 'Active', modeled: 'Open', stations: 12, bidirectional: true, score: 0.8 },
    { region: 'Japan', octant: 'NW', bearingDeg: 330, band: '15m', tier: 'Moderate', modeled: 'Marginal', stations: 3, bidirectional: false, score: 0.4 },
    { region: 'Africa', octant: 'E', bearingDeg: 100, band: '10m', tier: 'Closed', modeled: 'Closed', stations: 0, bidirectional: false, score: 0.05 },
  ],
  openings: [
    { band: '6m', mode: 'FT8', octant: 'S', bearingDeg: 180, maxKm: 2100, probability: 0.8, stations: 7, confidence: 'High', confidenceScore: 0.9,
      reciprocalPairs: 3, anomalyZ: 3.1, onsetSecs: 600, isNew: true, note: 'Es opening' },
  ],
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
const NEEDS: NeedAlert[] = [
  { call: 'VP8XYZ', entity: 'Falkland Islands', band: '40m', zone: 13, tags: ['NewEntity'], priority: 1, headline: 'New DXCC', mode: 'FT8',
    freqMhz: 7.074, admittedAt: NOW - 60, evidence: 'heard by 3 near you' },
  { call: 'JA1ABC', entity: 'Japan', band: '17m', zone: 25, tags: ['NewBand'], priority: 2, headline: 'New band', mode: 'CW', freqMhz: 18.08,
    admittedAt: NOW - 120 },
  { call: 'ZS6XX', entity: 'South Africa', band: '15m', zone: 38, tags: ['NewMode'], priority: 3, headline: 'New mode', mode: 'SSB', freqMhz: 21.3,
    admittedAt: NOW - 180 },
  { call: 'K7ABC', entity: 'United States', band: '20m', zone: 3, tags: ['NewState'], priority: 4, headline: 'New state', mode: 'FT8',
    freqMhz: 14.074, admittedAt: NOW - 240 },
] as NeedAlert[]
const AMP_UP = {
  family: 'spe', model: '1.3K-FA', linked: true, reason: '', operate: true, transmitting: false, outputWatts: 400, bandLabel: '20m', swr: 1.3,
  swrAtu: null, volts: 48.2, amps: 9.5, temp: 44, tempCelsius: true, alarm: 'swrExceedingLimits', alarmRaised: true, warning: 'overheating',
  warningRaised: true, kpaFault: null,
} as unknown as AmpStatus
const AMP_DOWN = { ...AMP_UP, linked: false, reason: 'noAnswer', alarmRaised: false, warningRaised: false } as AmpStatus
// The Spots box's board: a row of each mode's badge, the watched station's (its WATCH tile) and a need.
const spotRow = (call: string, band: string, freqMhz: number, mode: string, over: Partial<SpotRow> = {}): SpotRow =>
  ({
    call, entity: 'Somewhere', zone: 5, state: null, band, freqMhz, mode, submode: null, spotter: 'W3LPL', corroborators: [],
    ageSecs: 30, comment: 'up 2', licensed: true, spotterLocal: true, ...over,
  }) as SpotRow
const SPOTS: SpotRow[] = [
  spotRow('K1CW', '20m', 14.025, 'CW'),
  spotRow('W2SSB', '40m', 7.2, 'Phone'),
  spotRow('JA1FT', '15m', 21.074, 'Digital', { submode: 'FT8' }),
  spotRow('VP8XYZ', '40m', 7.074, 'Digital', { entity: 'Falkland Islands', submode: 'FT8' }),
]
const SPOTS_FEED: SpotsFeed = {
  rows: SPOTS,
  board: { bandPlan: [], selectedCall: null, myGrid: 'EN52', onSelect: () => {}, onWork: () => {}, needAlerts: NEEDS },
}
// The POTA/SOTA box's board: a new park and a band that is open (a spot hunted today is hidden by default).
const otaSpot = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
  program: 'POTA', reference, name: 'Test park', activator, freqKhz: 14_285, mode: 'SSB', spotter: null, comment: null, grid: null,
  newPark: false, bandOpen: false, huntedToday: false, ...over,
})
const OTA: OtaSpot[] = [otaSpot('K1NEW', 'US-0001', { newPark: true }), otaSpot('K2OPN', 'US-0002', { bandOpen: true })]
const OTA_BOARD: OtaBoard = {
  snap: { hunt: null, radio: { dialMhz: 14.285 }, logTick: 1 } as unknown as AppSnapshot,
  onHunt: () => {},
  onSnap: () => {},
}

function ctxOf(over: Partial<PaneContext> = {}): PaneContext {
  return {
    myGrid: 'EN52', entityCentroids: null, theme: 'dark', intent: 'dx',
    prop: PROP, prov: { label: 'LIVE', cls: 'live' },
    needByCall: new Map([['VP8XYZ', 'NewEntity']]), needAlerts: NEEDS,
    selectedCall: null, selStation: null, selSpot: null, selDxped: null, selDxpedWindow: null, dxpedWindows: new Map(), selGrid: null,
    pathPred: OUTLOOK, bandOutlook: OUTLOOK, pathOpen: OUTLOOK_BANDS, outlookOpen: OUTLOOK_BANDS,
    getout: {
      count: 3, maxKm: 4200,
      reports: [
        { call: 'K1ABC', grid: 'FN42', band: '20m', snr: -12, bearingDeg: 70, km: 1400, octant: 'NE', ageSecs: 120 },
        { call: 'G4XYZ', grid: 'IO91', band: '20m', snr: -18, bearingDeg: 50, km: 6200, octant: 'NE', ageSecs: 300 },
      ],
    },
    focusBand: null, amp: AMP_UP,
    scales: { r: 1, s: 0, g: 3, gTomorrow: 2, asOf: NOW },
    alerts: [{ productId: 'K05A', issued: NOW, kind: 'ALERT', message: 'Geomagnetic K-index of 5 reached' }],
    muf: [],
    spotsFeed: SPOTS_FEED, otaBoard: OTA_BOARD,
    onSelectCall: () => {}, toggleFocusBand: () => {},
    ...over,
  } as PaneContext
}

// The NOW bar in each state its chips have: the band Open, Marginal and Closed; heard or not; a need or
// none; the propagation LIVE, PARTIAL, CACHED and OFFLINE; and each feed state.
const feed = (state: FeedStatus['state'], lastEventSecs: number | null = 30): FeedStatus => ({ enabled: true, state, lastEventSecs })
const HEARD: PropagationSnapshot = {
  ...PROP,
  advisory: {
    ...PROP.advisory,
    // 60 m heard; 40 m Marginal with nobody heard (a heard band draws green: propViz bandConditionCell).
    bands: PROP.advisory.bands.map((b) =>
      b.band === '60m' ? { ...b, nHearMe: 8, nIHear: 12 } : b.band === '40m' ? { ...b, tier: 'Quiet' as never } : b,
    ),
  },
}
const NOW_BARS: Array<[string, string, PropagationSnapshot, FeedHealth | null]> = [
  ['open', '60m', HEARD, { cluster: feed('live'), phoneCluster: feed('reconnecting'), phoneClusterHost: 'node', pskr: feed('idle', 900) } as FeedHealth],
  ['marginal', '40m', { ...HEARD, source: 'partial' }, { cluster: feed('connecting', null), phoneCluster: { enabled: false, state: 'off', lastEventSecs: null }, phoneClusterHost: null, pskr: feed('connected', null) } as FeedHealth],
  ['closed', '80m', { ...HEARD, source: 'cached', dxpeditions: { ...HEARD.dxpeditions, workableNow: [] } }, null],
  ['offline', '60m', { ...HEARD, source: 'offline' }, null],
]

// ── rendering: the real frames in the real chains ─────────────────────────────────────────────────────────
interface Word {
  what: string
  kind: string
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
/** The state-word kinds this change is about, by the class that names them (nearest first). */
const KIND_CLASSES = [
  'bbt-band', 'heatmap-band', 'swx-impact', 'cp-work', 'cp-mode', 'swsc-chip', 'go-snr', 'getout-summary', 'chase-open',
  'cfeed-ends', 'opening-band', 'opening-new', 'kp-line', 'sat-stale', 'sat-chip', 'rotor-slewing', 'rotor-stop', 'amp-link',
  'amp-fault', 'prop-prov',
  // The Spots and POTA/SOTA boxes' boards, and the NOW bar.
  'np-mode-col', 'pota-badge', 'nb-chip', 'nb-src',
]
function kindOf(nodes: Element[]): string {
  for (let i = nodes.length - 1; i >= 0; i--) {
    const k = KIND_CLASSES.find((c) => nodes[i].classList.contains(c))
    if (k) return k
  }
  const own = nodes[nodes.length - 1]
  return `${own.tagName.toLowerCase()}.${[...own.classList].join('.')}`
}
/** A state word's identity in the census: the element's classes, how its component flags its ink, and,
 *  where its kind's class sits on an ancestor (a chip around its word), that ancestor's state classes. */
const signature = (w: Word) => {
  const own = w.nodes[w.nodes.length - 1]
  const flag = w.nodes.map((n) => n.getAttribute('data-state-ink')).filter((v) => v != null).pop()
  const cls = [...own.classList].join('.')
  const holder = [...w.nodes].reverse().find((n) => n.classList.contains(w.kind))
  const held = holder && holder !== own ? [...holder.classList].filter((c) => c !== w.kind) : []
  return `${w.kind}: ${cls ? `.${cls}` : own.tagName.toLowerCase()}${held.length ? ` in .${w.kind}.${held.join('.')}` : ''}${flag != null ? ` [${flag || 'set'}]` : ''}`
}
function readWords(root: ParentNode, where: string, out: Word[], outside = 'map') {
  const seen = new Set<Element>()
  const walker = document.createTreeWalker(root as Node, NodeFilter.SHOW_TEXT)
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n.parentElement
    if (!el || seen.has(el) || !/[\p{L}\p{N}]/u.test(n.textContent ?? '')) continue
    seen.add(el)
    if (el.closest('[hidden]')) continue
    const nodes = nodesOf(el)
    const pane = el.closest('.pane-frame')?.getAttribute('data-pane') ?? outside
    out.push({ what: `${where} ${pane} .${[...el.classList].join('.') || el.tagName.toLowerCase()} "${(n.textContent ?? '').trim().slice(0, 18)}"`, kind: kindOf(nodes), nodes, chain: [BODY, ...chainOf(el)] })
  }
}

async function renderAll(): Promise<Word[]> {
  const words: Word[] = []
  const shell = (host: 'rail' | 'strip' | 'dash', paneId: PaneId, ctx: PaneContext) => {
    // The dashboard rail beside a cockpit (components/DashRail): App's shell, the rail, its column.
    if (host === 'dash')
      return (
        <div className="app">
          <div className="shell" data-dash-rail="on">
            <aside className="dash-rail">
              <div className="dash-rail-col dash-boxes">
                <PaneFrame slotId="rail1" slotName="rail1" paneId={paneId} ctx={ctx} onAssign={() => {}} onHide={() => {}} share={1} />
              </div>
            </aside>
          </div>
        </div>
      )
    const slot: SlotId = host === 'rail' ? 'left1' : 'bottom1'
    const frame = <PaneFrame slotId={slot} paneId={paneId} ctx={ctx} onAssign={() => {}} onHide={() => {}} />
    return (
      <div className="app">
        <main className="layout single">
          <div className="connect-shell">
            <div className="connect" data-rails="both">
              {host === 'rail' ? (
                <div className="connect-rail" data-side="left">
                  {frame}
                </div>
              ) : (
                <div className="connect-strip">{frame}</div>
              )}
            </div>
          </div>
        </main>
      </div>
    )
  }
  const variants: Array<[string, PaneId[], PaneContext]> = [
    ['', [...PANE_IDS], ctxOf()],
    // The selection's own chip and the path table (the outlook pane, a station selected).
    ['selected', ['selection', 'outlook'], ctxOf({ selectedCall: 'VP8XYZ', selGrid: 'GD18' })],
    // An amplifier that stopped answering: the link word in its warning colour.
    ['amp down', ['amp'], ctxOf({ amp: AMP_DOWN })],
  ]
  for (const [tag, ids, ctx] of variants)
    for (const host of ['rail', 'strip', 'dash'] as const)
      for (const id of ids) {
        let r!: ReturnType<typeof render>
        await act(async () => {
          r = render(shell(host, id, ctx))
        })
        // The self-fetching panes (Kp outlook, satellite passes, rotor) answer on the next ticks.
        for (let k = 0; k < 4; k++)
          await act(async () => {
            await new Promise((res) => setTimeout(res, 0))
          })
        readWords(r.container, `${host}${tag ? ` (${tag})` : ''}`, words)
        cleanup()
      }
  // The map's insight card (the MUF line and the modelled heatmap), in its map chain.
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(
      <div className="app">
        <main className="layout single">
          <div className="connect-shell">
            <div className="connect" data-rails="both">
              <div className="connect-map">
                <div className="map-view" data-projection="globe">
                  <div className="map-body">
                    <div className="map-canvas-wrap">
                      <MapInsightRail prop={PROP} outlook={OUTLOOK} />
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </main>
      </div>,
    )
  })
  readWords(r.container, 'map', words)
  cleanup()
  // The NOW bar, a child of the app above the section (App.tsx), in each of its states; the band chip reads
  // the published advisory, as every band menu does.
  for (const [tag, band, prop, feeds] of NOW_BARS) {
    publishBandConditions(prop)
    await act(async () => {
      r = render(
        <div className="app">
          <NowBar
            snap={{ radio: { band } } as unknown as AppSnapshot}
            prop={prop}
            feedHealth={feeds}
            connectEnabled
            dxpedEnabled
            onNavigate={() => {}}
            rail={{ on: tag === 'open', onToggle: () => {} }}
          />
        </div>,
      )
    })
    readWords(r.container, `app (${tag})`, words, 'nowBar')
    cleanup()
  }
  publishBandConditions(null)
  // One word per shape: two words with the same chain and the same inline styles resolve alike in every mode.
  const shape = (w: Word) => chainKey(w.chain) + JSON.stringify(w.nodes.map((n) => (n as HTMLElement).style?.cssText ?? ''))
  return [...new Map(words.map((w) => [shape(w), w])).values()]
}

// ── the cascade, for one word ─────────────────────────────────────────────────────────────────────────────
const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
/** The sheet as it shipped before this change: every light-theme rule on a state word, removed. */
const SHIPPED = RULES.filter(
  (r) => !(r.selector.startsWith("[data-theme='light'] ") && KIND_CLASSES.some((k) => r.selector.includes(`.${k}`))),
)

if (RULES.filter((r) => !SHIPPED.includes(r)).some((r) => r.decls.some((d) => d.prop.startsWith('--'))))
  throw new Error('a light-theme state-word rule declares a custom property: the shipped sheet no longer shares the token table')
/** The accent's words as they shipped: the light-theme rules on them, removed (the same token table, like SHIPPED's). */
const ACCENT_SHIPPED = RULES.filter((r) => !(r.selector.startsWith("[data-theme='light'] ") && /\.(cp-muf|sat-when|mini-spectrum-src)\b/.test(r.selector)))
if (RULES.filter((r) => !ACCENT_SHIPPED.includes(r)).some((r) => r.decls.some((d) => d.prop.startsWith('--'))))
  throw new Error('a light-theme accent-word rule declares a custom property: the shipped sheet no longer shares the token table')

const role = (id: string) => PALETTE_ROLES.find((x) => x.id === id)!
const PRESETS: Record<string, string>[] = [
  {},
  ...role('amber').presets.slice(1).map((p) => ({ amber: p.id })),
  ...role('ok').presets.slice(1).map((p) => ({ ok: p.id })),
]
const skinsOf = (theme: 'light' | 'dark') => ['', ...SKINS.filter((s) => s.base === theme).map((s) => s.id)]
/** Every built-in light theme: the four light modes, bare and on each light theme, and the standard light
 *  theme under every OK and amber preset. After the fix a state word letters in the theme's ink, which no
 *  preset moves; the presets move the MARKS and the shipped inks, and those are swept on the standard theme. */
const LIGHT: Mode[] = [
  ...BASE_MODES.filter((b) => baseTheme(b) === 'light').flatMap((b) => skinsOf('light').map((skin) => (skin ? withRoles(b, { skin }) : b))),
  ...PRESETS.slice(1).map((roles) => withRoles('light', roles)),
]
/** The four dark modes, and the standard one on the dark themes MODES carries as the worst case (SENTINEL_MODES). */
const DARK: Mode[] = [
  ...BASE_MODES.filter((b) => baseTheme(b) === 'dark'),
  ...SENTINEL_MODES.filter((m) => m.startsWith('dark skin=')),
]

const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-[\w-]+|data-radix[\w-]*)$/.test(k))]))
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

/** The rules this change added declare no custom property, so the shipped sheet's tokens ARE the sheet's:
 *  one token table serves both (the token walk is the expensive half of every lookup). */
const TOKEN_RULES = (rules: Rule[]) => (rules === SHIPPED || rules === ACCENT_SHIPPED ? RULES : rules)
/** Each prefix of a word's chain, keyed once: its shape, and the custom properties its nodes set inline. */
const KEYS = new WeakMap<Word, { shape: string[]; vars: string[] }>()
function keysOf(w: Word) {
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
const winAt = (rules: Rule[], mode: Mode, w: Word, i: number, ...props: string[]) =>
  once(rules, `w|${winnerMode(mode)}|${keysOf(w).shape[i]}|${props.join()}`, () =>
    winnerAt(rulesWith(rules, props), winnerMode(mode), w.chain.slice(0, i + 1), ...props),
  )
/** Tokens at word.chain[0..i], built element by element with cssCascade.tokensAt's own semantics (a token an
 *  element inherits arrives computed; one declared on it resolves against its own tokens), then what the
 *  node sets inline, which wins over the sheet on that element. Incremental, so each prefix is walked once. */
function tokensAtIndex(rules: Rule[], mode: Mode, w: Word, i: number): Map<string, string> {
  rules = TOKEN_RULES(rules)
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
const TOKEN_DECLS = new WeakMap<Rule[], Rule[]>()
function rulesWithTokens(rules: Rule[]): Rule[] {
  let out = TOKEN_DECLS.get(rules)
  if (!out) TOKEN_DECLS.set(rules, (out = rules.filter((r) => r.decls.some((d) => d.prop.startsWith('--')))))
  return out
}
const colourAt = (rules: Rule[], mode: Mode, w: Word, i: number, value: string, under: Rgb): Rgb => {
  const k = keysOf(w)
  return once(TOKEN_RULES(rules), `c|${mode}|${k.shape[i]}|${k.vars[i]}|${value}|${under.join()}`, () => {
    const expanded = expandWith(tokensAtIndex(rules, mode, w, i), value)
    const c = toRgb(expanded, under)
    if (!c) throw new Error(`${w.what} ${mode}: not a colour: "${value}" → "${expanded}"`)
    return c
  })
}
const BLANK = /^(inherit|transparent|none|initial|unset)$/i
const important = (rule: Rule, prop: string) => rule.decls.some((d) => d.prop === prop && /!\s*important\s*$/.test(d.value))
const nodeStyle = (w: Word, i: number): CSSStyleDeclaration | null => (i >= 1 ? ((w.nodes[i - 1] as HTMLElement).style ?? null) : null)

/** The declaration that paints `prop` at chain index i: an !important author rule, else the node's inline
 *  style, else the cascade winner; null when the element leaves it to its parent. */
function declAt(rules: Rule[], mode: Mode, w: Word, i: number, prop: string, ...props: string[]): string | null {
  const css = winAt(rules, mode, w, i, prop, ...props)
  if (css && important(css.rule, css.prop)) return css.value
  const inline = nodeStyle(w, i)?.getPropertyValue(prop).trim()
  if (inline) return inline
  return css && !BLANK.test(css.value.trim()) ? css.value : null
}
/** What the word sits on: every background from it outward, down to the first opaque one, composited. */
function surfaceOf(rules: Rule[], mode: Mode, w: Word): Rgb {
  const layers: Array<[number, string]> = []
  for (let i = w.chain.length - 1; i >= 0; i--) {
    const v = declAt(rules, mode, w, i, 'background', 'background-color')
    if (!v || BLANK.test(v)) continue
    layers.push([i, v])
    if (hex(colourAt(rules, mode, w, i, v, [0, 0, 0])) === hex(colourAt(rules, mode, w, i, v, [255, 255, 255]))) break
  }
  let under = colourAt(rules, mode, w, 0, 'var(--bg)', [0, 0, 0])
  for (let k = layers.length - 1; k >= 0; k--) under = colourAt(rules, mode, w, layers[k][0], layers[k][1], under)
  return under
}
/** The word's ink (it inherits) and the dimming every ancestor applies to it. */
function inkOf(rules: Rule[], mode: Mode, w: Word): { value: string; at: number } {
  for (let i = w.chain.length - 1; i >= 0; i--) {
    const v = declAt(rules, mode, w, i, 'color')
    if (v) return { value: v, at: i }
  }
  throw new Error(`${w.what}: nothing paints it`)
}
function opacityOf(rules: Rule[], mode: Mode, w: Word): number {
  let o = 1
  for (let i = 0; i < w.chain.length; i++) {
    const v = declAt(rules, mode, w, i, 'opacity')
    if (v) o *= Number(v)
  }
  return o
}
const shown = (rules: Rule[], mode: Mode, w: Word) => w.chain.every((_, i) => declAt(rules, mode, w, i, 'display') !== 'none')

function wordOf(rules: Rule[], mode: Mode, w: Word) {
  const bg = surfaceOf(rules, mode, w)
  const ink = inkOf(rules, mode, w)
  const raw = colourAt(rules, mode, w, ink.at, ink.value, bg)
  const o = opacityOf(rules, mode, w)
  const fg = [0, 1, 2].map((k) => Math.round(raw[k] * o + bg[k] * (1 - o))) as unknown as Rgb
  return { raw, fg, bg, ratio: contrast(fg, bg), ink }
}
/** The theme's accent, resolved at the word. */
const accentAt = (rules: Rule[], mode: Mode, w: Word, at: number) => hex(colourAt(rules, mode, w, at, 'var(--accent)', [0, 0, 0]))
/** The theme's own inks at the word: its text colours and its accent. Anything else is a state colour. */
const NEUTRAL = ['--text', '--text-dim', '--text-faint', '--accent', '--accent-ink', '--readout']
const neutralAt = (rules: Rule[], mode: Mode, w: Word, at: number) =>
  new Set(NEUTRAL.map((t) => hex(colourAt(rules, mode, w, at, `var(${t})`, [0, 0, 0]))))

function unreadable(rules: Rule[], words: Word[], modes: readonly Mode[]): string[] {
  const out: string[] = []
  for (const w of words)
    for (const mode of modes) {
      if (!shown(rules, mode, w)) continue
      const { fg, bg, ratio } = wordOf(rules, mode, w)
      if (ratio < 4.5) out.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
    }
  return out
}

describe('every state-coloured word on Connect reads in every light theme', () => {
  let all: Word[] = []
  let state: Word[] = []
  let accent: Word[] = []
  beforeAll(async () => {
    all = await renderAll()
    state = all.filter((w) => shown(RULES, 'dark', w) && !neutralAt(RULES, 'dark', w, inkOf(RULES, 'dark', w).at).has(hex(wordOf(RULES, 'dark', w).raw)))
    accent = all.filter((w) => shown(RULES, 'dark', w) && hex(wordOf(RULES, 'dark', w).raw) === accentAt(RULES, 'dark', w, inkOf(RULES, 'dark', w).at))
  }, 120_000)

  // THE INVENTORY: every kind of state word on Connect, each in every state the data above puts it in. Exact,
  // on purpose. A word that turns neutral in dark would leave the census (and escape every check below it),
  // and a new state word must be looked at before it joins; either way this list is where it shows.
  const INVENTORY = [
    'amp-fault: .amp-fault.amp-alarm', 'amp-fault: .amp-fault.amp-warn', 'amp-link: .amp-link', 'amp-link: .amp-link.amp-down',
    'bbt-band: .bbt-band [mark]',
    'cfeed-ends: .cfeed-ends', 'chase-open: .chase-open.o-open', 'cp-mode: .cp-mode.fair', 'cp-mode: .cp-mode.good',
    'cp-work: .cp-work.w-excellent', 'cp-work: .cp-work.w-fair', 'cp-work: .cp-work.w-good', 'getout-summary: strong', 'go-snr: .go-snr',
    'heatmap-band: .heatmap-name [mark]', 'kp-line: .kp-line.good', 'kp-line: .kp-line.warn',
    'opening-band: .opening-band', 'opening-new: .opening-new', 'prop-prov: .prop-prov.prov-live',
    'rotor-slewing: .rotor-slewing', 'rotor-stop: .rotor-stop', 'sat-chip: .sat-chip.dead', 'sat-chip: .sat-chip.stale',
    'sat-stale: .sat-stale', 'swsc-chip: .swsc-chip.swsc-major', 'swsc-chip: .swsc-chip.swsc-minor', 'swx-impact: .swx-impact [mark]',
    // The Spots box's board (its mode badges: its WATCH tile is a need chip, NeedChip.contrast.test.tsx's), the
    // POTA/SOTA box's, and the NOW bar.
    'np-mode-col: .np-mode-col.np-mode-cw', 'np-mode-col: .np-mode-col.np-mode-digital', 'np-mode-col: .np-mode-col.np-mode-phone',
    'pota-badge: .pota-badge.pota-badge-open',
    'nb-chip: .nb-v in .nb-chip.good', 'nb-chip: .nb-v in .nb-chip.ok', 'nb-chip: .nb-v in .nb-chip.nb-need.good',
    'nb-chip: .nb-v in .nb-chip.nb-feed.good', 'nb-chip: .nb-v in .nb-chip.nb-feed.ok', 'nb-chip: .nb-v in .nb-chip.nb-feed.bad',
    'nb-src: .nb-src.cached', 'nb-src: .nb-src.live', 'nb-src: .nb-src.partial',
  ]
  it('finds every state word Connect letters, each in every state it has (the census cannot silently empty out)', () => {
    expect([...new Set(state.map(signature))].sort()).toEqual([...INVENTORY].sort())
    expect(new Set(state.map((w) => w.kind)), 'every kind').toEqual(new Set(KIND_CLASSES))
    // The rail's boxes are Connect's: every state word a box letters on Connect, it letters beside a cockpit.
    const inHost = (host: string) => [...new Set(state.filter((w) => w.what.startsWith(`${host} `)).map(signature))].sort()
    expect(inHost('dash'), 'the dashboard rail').toEqual(inHost('rail'))
    expect(all.length, 'words read').toBeGreaterThan(300)
  })

  it('every one clears 4.5:1 on what it sits on, in every light theme and under every OK and amber preset', () => {
    expect(unreadable(RULES, state, LIGHT)).toEqual([])
  }, 240_000)

  // Dark is untouched. The sheet half is by construction (every rule this change adds is scoped to the light
  // theme); what can move is the component half, where an inline colour became `--state-ink`, so each word
  // flagged that way must letter in exactly its `--state-ink`, and nothing may letter in a text ink or grow
  // an underline. Chrome compared every word before and after, byte for byte (the report).
  it('in every dark theme each keeps its state colour, unmarked; a --state-ink word letters in exactly it', () => {
    const moved: string[] = []
    for (const w of state)
      for (const mode of DARK) {
        const now = wordOf(RULES, mode, w)
        if (neutralAt(RULES, mode, w, now.ink.at).has(hex(now.raw))) moved.push(`${w.what} ${mode}: lettered in a text ink (${hex(now.raw)})`)
        let flagged = -1
        w.nodes.forEach((n, k) => n.hasAttribute('data-state-ink') && (flagged = k))
        if (flagged >= 0) {
          const own = colourAt(RULES, mode, w, flagged + 1, 'var(--state-ink)', [0, 0, 0])
          if (hex(own) !== hex(now.raw)) moved.push(`${w.what} ${mode}: ${hex(now.raw)}, not its --state-ink ${hex(own)}`)
        }
        const line = declAt(RULES, mode, w, w.chain.length - 1, 'text-decoration-line', 'text-decoration')
        if (line && line !== 'none' && !/line-through/.test(line)) moved.push(`${w.what} ${mode}: underlined in dark`)
      }
    expect(moved).toEqual([])
  }, 240_000)

  // The words with no border of their own that says their state: each must carry an underline or a bar in the
  // light themes. The rest keep the state on their chip's or row's border, or (Space Wx) on the gauge's bar
  // above; those are measured in the report. (A closed band's grey is no state: it recedes, below.) The
  // Spots board's mode badges letter in the page colour on their mode's fill, which carries the mode, and
  // read 4.5:1 in every light theme as they are.
  const MARKED = ['bbt-band', 'heatmap-band', 'cp-work', 'amp-link', 'kp-line', 'sat-stale', 'rotor-slewing', 'amp-fault']
  it('the state stays on its mark in the light themes: an underline or bar in the state colour, 3:1 off the surface', () => {
    const low: string[] = []
    for (const w of state)
      for (const mode of LIGHT) {
        const i = w.chain.length - 1
        const line = declAt(RULES, mode, w, i, 'text-decoration-line')
        // A bar's colour may arrive in the `border-left` shorthand: the colour is its var() or hex.
        const bar = declAt(RULES, mode, w, i, 'border-left-color', 'border-left')?.match(/var\(--[\w-]+[^)]*\)|#[0-9a-fA-F]{3,8}\b/)?.[0] ?? null
        const mark = line === 'underline' ? declAt(RULES, mode, w, i, 'text-decoration-color') : bar
        if (!mark) {
          if (MARKED.includes(w.kind)) low.push(`${w.what} ${mode}: no underline or bar carries its state`)
          continue
        }
        const bg = surfaceOf(RULES, mode, w)
        const c = colourAt(RULES, mode, w, i, mark, bg)
        if (neutralAt(RULES, mode, w, i).has(hex(c))) low.push(`${w.what} ${mode}: the mark is a text ink`)
        else if (contrast(c, bg) < 3) low.push(`${w.what} ${mode}: the mark ${hex(c)} on ${hex(bg)} = ${contrast(c, bg).toFixed(2)}:1`)
      }
    expect(low).toEqual([])
  }, 60_000)

  // The chips whose own border says their state: with the word in ink, that border must stand 3:1 off what the
  // chip sits on. (Rows that carry it on their left edge, Chase's and the openings', are measured in the report.)
  const CHIPS = ['cp-mode', 'swsc-chip', 'cfeed-ends', 'sat-chip', 'pota-badge', 'nb-chip', 'nb-src']
  /** A border declaration's colour: the value itself, or a `border` shorthand without its width and style. */
  const borderColour = (v: string) => v.replace(/^\s*[\d.]+(px|em|rem)\s+/, '').replace(/^(solid|dashed|dotted|double)\s+/, '').trim()
  /** The chip's index in the word's chain: the word itself, or (the NOW bar's) the chip it sits in. */
  const chipAt = (w: Word) => w.chain.length - 1 - [...w.nodes].reverse().findIndex((n) => n.classList.contains(w.kind))
  /** Each word's chip border that is missing or under 3:1 off what the chip sits on, in the light themes. */
  function lowEdges(words: Word[]): string[] {
    const low: string[] = []
    for (const w of words)
      for (const mode of LIGHT) {
        const i = chipAt(w)
        const v = declAt(RULES, mode, w, i, 'border-top-color', 'border-color', 'border')
        if (!v) {
          low.push(`${w.what} ${mode}: no border`)
          continue
        }
        const under = surfaceOf(RULES, mode, { ...w, chain: w.chain.slice(0, i), nodes: w.nodes.slice(0, i - 1) })
        // A border mixed from `currentColor` (the NOW bar's source chip) is mixed from the chip's own ink.
        const ink = inkOf(RULES, mode, { ...w, chain: w.chain.slice(0, i + 1), nodes: w.nodes.slice(0, i) }).value
        const c = colourAt(RULES, mode, w, i, borderColour(v).replace(/currentColor/gi, ink), under)
        if (contrast(c, under) < 3) low.push(`${w.what} ${mode}: the border ${hex(c)} on ${hex(under)} = ${contrast(c, under).toFixed(2)}:1`)
      }
    return low
  }
  it("a chip that carries its state on its own border keeps it 3:1 off the surface in the light themes", () => {
    expect(lowEdges(state.filter((x) => CHIPS.includes(x.kind)))).toEqual([])
  }, 60_000)

  // The POTA/SOTA box's two badges sit side by side. BAND OPEN is a state word, above. NEW PARK letters in the
  // accent, which the census counts as the theme's own ink (a link's), but on the accent's own tint: in the
  // light theme it read 3.36:1 in Chrome, beside a BAND OPEN that now reads. Both are held here, whatever
  // their ink: 4.5:1, and the colour kept on the badge's edge at 3:1.
  it("the POTA/SOTA box's badges read in every light theme, their colour on the badge's edge", () => {
    const badges = all.filter((w) => w.kind === 'pota-badge')
    expect([...new Set(badges.map(signature))].sort(), 'both badges').toEqual(['pota-badge: .pota-badge.pota-badge-new', 'pota-badge: .pota-badge.pota-badge-open'])
    expect(unreadable(RULES, badges, LIGHT)).toEqual([])
    expect(lowEdges(badges)).toEqual([])
  }, 60_000)

  // THE ACCENT'S WORDS (operator, 2026-09-30: "Ink, accent as the mark"). A word Connect letters in the theme's accent: the
  // MUF, Satellite Passes' next pass time, the scope's source badge and the openings' note. The accent was tuned as a
  // mark, and as lettering on the light page it read 4.25:1 (Chrome), so a word that did takes the theme's ink in the
  // light themes and keeps the accent as its underline. The census is the standard dark theme's again, where their ink is
  // the accent, and it is exact.
  const ACCENT_INVENTORY = [
    '.mini-spectrum-src', '.opening-note', '.sat-when', 'strong',
    // The two boards in the Spots and POTA/SOTA boxes (their screens' own palette).
    '.np-filter-toggle.active', '.np-th.active', '.pota-badge.pota-badge-new', '.pota-hunt-btn', '.pota-spot-ref',
  ].sort()
  // Of the boards' accent words, the Spots board's Filter toggle and sorted heading read as they are. NEW PARK is a
  // chip: its accent went to its edge, held with BAND OPEN by the badges' test above. The POTA/SOTA board's HUNT
  // (3.45:1) and park reference (4.25:1) are that board's on every screen that shows it, and N54's (2026-09-30):
  // named here, and held to the floor once that lands.
  const ACCENT_HELD_ELSEWHERE = ['.pota-badge.pota-badge-new', '.pota-hunt-btn', '.pota-spot-ref']
  /** The light themes, and the standard light theme under each accent preset: the accent is these words' own colour. */
  const ACCENT_LIGHT: Mode[] = [...LIGHT, ...role('accent').presets.slice(1).map((p) => withRoles('light', { accent: p.id }))]
  const ownOf = (w: Word) => {
    const own = w.nodes[w.nodes.length - 1]
    return own.classList.length ? `.${[...own.classList].join('.')}` : own.tagName.toLowerCase()
  }
  it('every word lettered in the accent reads 4.5:1 in every light theme and accent, and one that left it keeps the accent as its underline', () => {
    expect([...new Set(accent.map(ownOf))].sort()).toEqual(ACCENT_INVENTORY)
    const low: string[] = []
    for (const w of accent.filter((x) => !ACCENT_HELD_ELSEWHERE.includes(ownOf(x))))
      for (const mode of ACCENT_LIGHT) {
        if (!shown(RULES, mode, w)) continue
        const { fg, bg, ratio, raw, ink } = wordOf(RULES, mode, w)
        if (ratio < 4.5) low.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
        if (hex(raw) === accentAt(RULES, mode, w, ink.at)) continue
        const i = w.chain.length - 1
        const mark = declAt(RULES, mode, w, i, 'text-decoration-line') === 'underline' ? declAt(RULES, mode, w, i, 'text-decoration-color') : null
        if (mark !== 'var(--accent)') {
          low.push(`${w.what} ${mode}: lettered in ${hex(raw)}, and the accent is not its underline (${mark})`)
          continue
        }
        const c = colourAt(RULES, mode, w, i, mark, bg)
        if (contrast(c, bg) < 3) low.push(`${w.what} ${mode}: the underline ${hex(c)} on ${hex(bg)} = ${contrast(c, bg).toFixed(2)}:1`)
      }
    expect(low).toEqual([])
  }, 120_000)

  it('in every dark theme each word lettered in the accent keeps it, with no underline', () => {
    const moved: string[] = []
    for (const w of accent)
      for (const mode of DARK) {
        const now = wordOf(RULES, mode, w)
        if (hex(now.raw) !== accentAt(RULES, mode, w, now.ink.at)) moved.push(`${w.what} ${mode}: lettered in ${hex(now.raw)}, not the accent`)
        const line = declAt(RULES, mode, w, w.chain.length - 1, 'text-decoration-line', 'text-decoration')
        if (line && line !== 'none') moved.push(`${w.what} ${mode}: underlined in dark`)
      }
    expect(moved).toEqual([])
  }, 60_000)

  it('FIRES: the accent words as they shipped are caught in the light theme, at the ratio Chrome measured', () => {
    const found = unreadable(ACCENT_SHIPPED, accent, ['light'])
    const has = (re: RegExp) => found.some((m) => re.test(m))
    expect(has(/ outlook \.strong "14\.2 MHz" light: #0174ab on #e5eaf0 = 4\.25:1/), 'the MUF').toBe(true)
    expect(has(/ satPasses \.sat-when ".*" light: #0174ab on #e5eaf0 = 4\.25:1/), 'the next pass').toBe(true)
    expect(has(/ scope \.mini-spectrum-src "AUDIO" light: #0174ab on #e5eaf0 = 4\.25:1/), 'the scope badge').toBe(true)
  }, 60_000)

  it('the reader itself: its element-by-element tokens are cssCascade.tokensAt, wherever no node sets one inline', () => {
    const plain = state.filter((w) => keysOf(w).vars.every((v) => v === '[]')).slice(0, 25)
    expect(plain.length, 'words with no inline token').toBeGreaterThan(10)
    for (const w of plain)
      for (const mode of ['light', 'dark skin=lagoon'] as Mode[]) {
        const i = w.chain.length - 1
        expect(Object.fromEntries(tokensAtIndex(RULES, mode, w, i)), `${w.what} ${mode}`).toEqual(Object.fromEntries(tokensAt(RULES, mode, w.chain)))
      }
  }, 240_000)

  it('FIRES: the words as they shipped are caught in the light theme at the ratios Chrome measured', () => {
    const found = unreadable(SHIPPED, state, ['light'])
    const has = (re: RegExp) => found.some((m) => re.test(m))
    expect(has(/ spacewx \.swx-impact ".*" light: #a27000 on #e5eaf0 = 3\.58:1/), 'Space Wx caption').toBe(true)
    expect(has(/ outlook \.cp-work\.w-good "Good" light: #007f35 on #e5eaf0 = 4\.25:1/), 'Band Outlook Good').toBe(true)
    // The storm line's own colour is the theme's warning now that --state-warn is (it was the fixed #f5a524, 1.69:1).
    expect(has(/ kpOutlook \.kp-line\.warn .* light: #a76d00 on #e5eaf0 = 3\.59:1/), 'the Kp storm line').toBe(true)
    // Beside the cockpits, in the boxes' boards and on the NOW bar, before their light rules: the POTA/SOTA box's
    // BAND OPEN (1.65:1 here, 1.66:1 in Chrome: the badge's tint rounds a unit apart) and the bar's PROP CACHED.
    // (A need chip, in the rail as anywhere, is NeedChip.contrast.test.tsx's.)
    expect(has(/ pota \.pota-badge\.pota-badge-open "BAND OPEN" light: #22c55e on #c2e3d6 = 1\.6[56]:1$/), 'BAND OPEN').toBe(true)
    expect(has(/ nowBar \.nb-src\.cached "PROP CACHED" light: #a27000 on #fbfcfe = 4\.21:1$/), "the NOW bar's PROP CACHED").toBe(true)
    // NEW PARK, which the census does not count (above): 3.37:1 here, 3.36:1 in Chrome.
    const badge = unreadable(SHIPPED, all.filter((w) => w.kind === 'pota-badge'), ['light'])
    expect(badge.some((m) => / pota \.pota-badge\.pota-badge-new "NEW PARK" light: #0174ab on #bcd5e4 = 3\.3[67]:1$/.test(m)), 'NEW PARK').toBe(true)
  }, 60_000)

  // A CLOSED BAND RECEDES BY ITS INK, NEVER BY FADING (operator, 2026-09-30: "Closed rows and chips fade by colour instead of
  // transparency"). The Band Advisor's closed rows and Band Outlook's closed mode chips faded by opacity, and a closed
  // band's name in the 24-hour chart and the Best Band table lettered in the closed grey. In the dark theme, in Chrome,
  // the row's "Closed" read 1.98:1, the reason under it 2.26:1, the chip 2.05:1 and the names 4.47:1. Each now recedes by
  // its ink alone, so every word they letter is held to 4.5:1 in every theme, light and dark, with nothing dimming it.
  const isClosed = (w: Word) =>
    w.nodes.some((n) => (n.classList.contains('ba-row') && n.classList.contains('is-closed')) || n.getAttribute('data-state-ink') === 'recede') ||
    w.nodes[w.nodes.length - 1].matches('.cp-mode.closed')
  // The Band Advisor's word is the Band conditions list's pill (`.bc-state`), which dims its own letters when closed.
  const CLOSED_INVENTORY = ['.ba-band', '.ba-modeled.bc-state.is-closed', '.ba-people', '.ba-reason', '.bbt-band', '.cp-mode.closed', '.heatmap-name', '.heatmap-rel']
  it('a closed band recedes by its ink: every word it letters reads 4.5:1 in every theme, and nothing dims it', () => {
    const closed = all.filter(isClosed)
    expect([...new Set(closed.map(ownOf))].sort()).toEqual(CLOSED_INVENTORY)
    const low: string[] = []
    for (const w of closed)
      for (const mode of [...LIGHT, ...DARK]) {
        if (!shown(RULES, mode, w)) continue
        const o = opacityOf(RULES, mode, w)
        if (o !== 1) low.push(`${w.what} ${mode}: dimmed to ${o}`)
        const { fg, bg, ratio } = wordOf(RULES, mode, w)
        if (ratio < 4.5) low.push(`${w.what} ${mode}: ${hex(fg)} on ${hex(bg)} = ${ratio.toFixed(2)}:1`)
      }
    expect(low).toEqual([])
  }, 120_000)
})

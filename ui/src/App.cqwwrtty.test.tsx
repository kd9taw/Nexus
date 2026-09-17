// @vitest-environment jsdom
//
// CQ WW RTTY, END TO END THROUGH APP — the contest this release ships for.
//
// The cockpit suites prove each seam against a fixture I wrote. This one proves the seams line
// up with what the ENGINE actually publishes: `fieldDay` below is the DTO printed by a real
// `cqww_rtty` session (`Engine::snapshot()` with fd_event = cqww_rtty, zone 4, QTH WI), pasted
// verbatim. That is the point — a hand-written `receives` cannot catch the two things most
// likely to go wrong between two agents' work:
//   · the QTH slot's domain is `cqww_rtty_qth`, not `fd_sections`; a grab that only recognised
//     the section list would fill nothing in this contest and say nothing about why;
//   · `sentExchange` is the zone UNPADDED plus the state ("4 WI"), so an F-key that assumed
//     "04" or Field Day's class/section would key the wrong exchange at every station.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor, within } from '@testing-library/react'
import type { AppSnapshot, Settings } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'

/** The transcript the operator is looking at: a CQ, then the answer with its exchange. */
const TEXT = 'CQ TEST W1AW W1AW CQ\r\nW1AW 599 14 WI\r\n'

/** ⭐ VERBATIM from `Engine::snapshot().field_day` for a CQ WW RTTY session — see the header. */
const FIELD_DAY = {
  running: true,
  state: 'CallingCq',
  dxcall: null,
  qsoCount: 0,
  sections: 0,
  workedSections: [],
  points: 0,
  event: 'cqww_rtty',
  poweredPoints: 0,
  bonusPoints: 0,
  totalScore: 0,
  multCount: 0,
  scoreNoteKey: '',
  rulesYear: 2026,
  log: [],
  receives: [
    { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
    { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
    { key: 'QTH', kind: 'enum', required: false, domain: 'cqww_rtty_qth' },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'ZN', raw: '4' },
    { key: 'QTH', raw: 'WI', domain: 'cqww_rtty_qth' },
  ],
  sentExchange: '4 WI',
  bands: ['80m', '40m', '20m', '15m', '10m'],
  role: 'w_ve',
  boards: [
    { id: 'zone', slot: 'ZN', scope: 'perBand', worked: [] },
    { id: 'qth', slot: 'QTH', domain: 'cqww_rtty_qth', scope: 'perBand', worked: [] },
  ],
}

const snapshot = {
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.083,
    band: '20m',
    catOk: true,
    sideband: 'LSB',
    transmitting: false,
    txEnabled: true,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'Ft8', periodSecs: 15, snrDb: 0, dtSec: 0, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: FIELD_DAY,
  recentDecodes: [],
  harqRescues: 0,
  hunt: null,
  b4MatchMode: false,
} as unknown as AppSnapshot

const rttyState = {
  armed: true, afcHz: 0, afcLocked: false, text: TEXT, charConf: [], baud: 45.45, shiftHz: 170,
  markHz: 2125, spaceHz: 2295, sending: false, latched: false, backend: 'afsk', keyerError: null,
  auto: false, seqState: 'idle', peer: null, peerExchange: [], heardCq: null,
}

// `vi.mock` is hoisted above the imports, and the factory object is built when App first
// imports './api' — before any top-level const here has run. `vi.hoisted` is the one place a spy
// can be created early enough to be named in it.
const { rttySend } = vi.hoisted(() => ({ rttySend: vi.fn() }))
const settings: { current: Settings } = { current: defaults as unknown as Settings }

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}),
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => settings.current),
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
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'),
    getRttyState: vi.fn(async () => rttyState),
    rttyAutoArm: vi.fn(async () => rttyState),
    rttySend,
    getLog: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    contestZoneHint: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

import App from './App'
import { frameForAir } from './features/rttyMacros'

const stream = () => document.querySelector('.rtty-host .cw-decode-text') as HTMLElement

/** jsdom has no caret hit-test; answer the offset a browser would, from the real text nodes. */
function caretAt(offset: number) {
  const hit = () => {
    const walker = document.createTreeWalker(stream(), NodeFilter.SHOW_TEXT)
    let left = offset
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const len = (n.nodeValue ?? '').length
      if (left <= len) {
        const r = document.createRange()
        r.setStart(n, left)
        r.collapse(true)
        return r
      }
      left -= len
    }
    return null
  }
  Object.defineProperty(document, 'caretRangeFromPoint', { value: hit, configurable: true })
}

async function grab(word: string, from = 0) {
  caretAt(TEXT.indexOf(word, from) + 1)
  await act(async () => {
    fireEvent.mouseDown(stream(), { detail: 1 })
    fireEvent.mouseUp(stream(), { detail: 1 })
    fireEvent.mouseDown(stream(), { detail: 2 })
    fireEvent.mouseUp(stream(), { detail: 2 })
    fireEvent.doubleClick(stream(), { detail: 2 })
  })
}

/** ⚠️ SCOPED TO THE RTTY HOST. The keep-alive host mounts other cockpits with their own log
 *  strips, so a bare `screen.getByLabelText('Zone')` finds several. */
const box = (caption: string) =>
  within(document.querySelector('.rtty-host') as HTMLElement).getByLabelText(caption) as HTMLInputElement

beforeEach(() => {
  rttySend.mockReset().mockImplementation(async () => rttyState)
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { rtty: true } }))
  localStorage.setItem('nexus.workspace', 'dx')
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
  window.location.hash = '#rtty'
  settings.current = {
    ...(defaults as unknown as Settings),
    macros: { ...(defaults as unknown as Settings).macros, activeRttyProfile: 'contest' },
  }
})
afterEach(() => {
  Reflect.deleteProperty(document, 'caretRangeFromPoint')
  cleanup()
})

describe('a CQ WW RTTY session, through App', () => {
  it('grabs the call and the exchange the session asks for, and keys the session’s own exchange', async () => {
    render(<App />)
    await waitFor(() => expect(stream()?.textContent).toBe(TEXT))

    // The call: both fields, as ever.
    await grab('W1AW')
    expect((document.querySelector('.rtty-host .rtty-hiscall') as HTMLInputElement).value).toBe('W1AW')
    await waitFor(() =>
      expect((document.querySelector('.rtty-host .le-fd-input-call') as HTMLInputElement).value).toBe('W1AW'),
    )

    // The zone, by the slot's own bounds — and the QTH, by a domain that is NOT the section list.
    await grab('14')
    await waitFor(() => expect(box('Zone').value).toBe('14'))
    await grab('WI')
    await waitFor(() => expect(box('QTH').value).toBe('WI'))
    // The report was never touched: 599 is a report, not an exchange value.
    expect(box('RST').value).toBe('599')

    // F2 in the Contest set is `{CALL} 599 {EXCH} {EXCH}` — the exchange is the SESSION's.
    await act(async () => {
      fireEvent.keyDown(window, { key: 'F2' })
    })
    await waitFor(() => expect(rttySend).toHaveBeenCalledWith(frameForAir('W1AW 599 4 WI 4 WI')))
  })
})

// @vitest-environment jsdom
//
// SPOTS AND NEEDED ON THE CW SCREEN (plan piece H8) — Phone's two feeds (#345), in CW.
//
// The operator's pick: "CW gets Phone's Spots/Needed panes", hidden by default exactly as in
// Phone — default-removed, one click in ⊞ Panels — and an existing CW layout loads unchanged.
// Everything runs against the REAL panel record (`usePanelLayout(CW_PANELS)`, as App builds it),
// the REAL ⊞ menu and the REAL boards, for Phone's reason: the default is a property of the
// record, and a stub answering 'docked' for every id would prove nothing about who sees them.
//
// What it pins:
//   · both hidden on a fresh screen AND on a record that predates them; the ⊞ button reads as it
//     did; a tick docks the real board as a fill pane, stored; the ✕ is the same act; Reset hides
//     them again and Undo brings them back; nothing here keys;
//   · the Spots pane opens on CW spots on the radio's band (a human spot in a CW segment and a
//     skimmer's CW decode; never a skimmer's RTTY or FT8 decode there), follows the band, and its
//     chips widen it in a filter copy of its own;
//   · the Needed pane opens on the CW needs in a record of its own;
//   · a row's Work is its view's own handler;
//   · the places: Spots at the foot of Decode's column, Needed at the foot of the middle column at
//     three tracks, both in the one merged column below that; the divider between them only where
//     the operator's arrangement makes them a pair, committing to the CW record;
//   · on Remote, a pane whose data the page does not carry says so with the existing status.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { SpotsPanel } from './SpotsPanel'
import { CW_PANELS, panelStorageKey, seamShares, usePanelLayout } from '../features/panelState'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import type { AppSnapshot, BandChannel, NeedAlert, SpotRow } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "a tick docks the real board as a fill feed with its own…", takes
// 0.61 s and 0.38 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const decodeState = {
  text: '',
  wpm: 0,
  sent: [] as string[],
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
const api = vi.hoisted(() => ({
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, rigModel: 0 })),
  getCatCwUnprovenRigModels: vi.fn(async () => []),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (text: string) => text),
  pointRotatorAtCall: vi.fn(async () => 0),
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => {}),
  openQrzPage: vi.fn(async () => {}),
  readRotator: vi.fn(async () => null),
  pointRotator: vi.fn(async () => {}),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The header hosts the ⊞ menu, which the cockpit hands it as `actions`; the rest is stubbed.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: unknown }) => <header className="cockpit-header">{actions as never}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('../remote-web/RemoteRecall', () => ({ RemoteRecallEntry: () => <div data-testid="recall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** Fire a resize to EVERY live observer (the region's and the column dividers'). */
let fire: (() => void) | null = null
beforeEach(() => {
  decodeState.sent = []
  api.cwDecode.mockImplementation(async () => decodeState)
  const live = new Set<() => void>()
  fire = () => [...live].forEach((cb) => cb())
  globalThis.ResizeObserver = class {
    cb: () => void
    constructor(cb: () => void) {
      this.cb = cb
      live.add(cb)
    }
    observe() {}
    disconnect() {
      live.delete(this.cb)
    }
    unobserve() {}
  } as unknown as typeof ResizeObserver
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
afterEach(() => {
  cleanup()
  localStorage.clear()
  sessionStorage.clear()
  for (const f of Object.values(api)) f.mockClear()
})

// With an NR level and an AGC the Rig controls frame renders, as on a real CAT radio.
function makeSnap(band = '20m', dialMhz = 14.05): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz, band, catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false, txEnabled: true,
      txAllowed: true, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', nb: null, nr: null, notch: null,
      filterWidthHz: 500, splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot
}

const spot = (call: string, band: string, freqMhz: number, mode: string, submode: string | null = null): SpotRow => ({
  call, entity: 'United States', zone: 5, band, freqMhz, mode, submode, spotter: 'W3LPL', corroborators: [],
  ageSecs: 30, comment: '', licensed: true, spotterLocal: true, workedAgoSecs: null, workedTodayUtc: false,
})
// `mode` is the backend's frequency-derived CLASS, `submode` only ever a skimmer's token.
const SPOTS: SpotRow[] = [
  spot('K1CW', '20m', 14.025, 'CW', 'CW'), // a skimmer's CW decode
  spot('W1HUMAN', '20m', 14.03, 'CW'), // a human's spot in the CW segment
  spot('W1AW', '20m', 14.25, 'Phone'),
  spot('JA1FT', '20m', 14.074, 'Digital', 'FT8'),
  spot('N2FORTY', '40m', 7.03, 'CW', 'CW'),
  // A skimmer's RTTY decode INSIDE the CW segment: the class says CW, the wire says RTTY.
  spot('W9RTTY', '20m', 14.04, 'CW', 'RTTY'),
]
const BAND_PLAN: BandChannel[] = [
  { band: '20m', group: 'HF', dialMhz: 14.074, mode: 'USB' as never, label: '20m', note: '' },
  { band: '40m', group: 'HF', dialMhz: 7.074, mode: 'USB' as never, label: '40m', note: '' },
]
const alert = (call: string, band: string, mode: string, freqMhz: number | null): NeedAlert => ({
  call, entity: 'Japan', band, zone: 25, tags: ['NewBand'] as never, priority: 50, headline: `${call} needed`, mode, freqMhz,
})
const ALERTS: NeedAlert[] = [
  alert('JA1CW', '20m', 'CW', 14.025),
  alert('JA2CW', '40m', 'CW', 7.03),
  alert('K1PHONE', '20m', 'Phone', 14.25),
  alert('VK9FT8', '20m', 'FT8', 14.074),
]

function boards() {
  return {
    spotsBoard: { bandPlan: BAND_PLAN, selectedCall: null, myGrid: 'EN52', onSelect: vi.fn(), onWork: vi.fn(), needAlerts: [] as NeedAlert[] },
    neededBoard: {
      alerts: ALERTS, bandPlan: BAND_PLAN, selectedCall: null, myGrid: 'EN52', onQsy: vi.fn(), onSelect: vi.fn(),
      onWork: vi.fn(), phoneSource: null,
    },
  }
}

/** The cockpit with the real panel record and the board wiring App passes. */
function Live({ band = '20m', wiring = boards() }: { band?: string; wiring?: ReturnType<typeof boards> }) {
  const panels = usePanelLayout(CW_PANELS)
  return (
    <CwCockpit
      snap={makeSnap(band, band === '40m' ? 7.03 : 14.05)}
      theme="dark"
      onWorkSpot={() => {}}
      spots={SPOTS}
      panels={panels}
      spotsBoard={wiring.spotsBoard}
      neededBoard={wiring.neededBoard}
    />
  )
}

const CW_KEY = panelStorageKey('cw')
const record = () => JSON.parse(localStorage.getItem(CW_KEY) ?? '{"state":{},"share":{}}')
/** Render, then let the mount-time reads (settings, the decode poll) settle. */
async function mount(el: React.ReactElement) {
  const r = render(el)
  await act(async () => {
    for (let i = 0; i < 4; i++) await Promise.resolve()
  })
  return r
}
/** Open ⊞ Panels (idempotent — the trigger TOGGLES) and return the popover. */
function openMenu() {
  const btn = screen.getByRole('button', { name: /Panels/ })
  if (btn.getAttribute('aria-expanded') !== 'true') fireEvent.click(btn)
  return screen.getByRole('group', { name: /panels on this screen/i })
}
const box = (label: RegExp) => within(openMenu()).getByRole('checkbox', { name: label }) as HTMLInputElement
const pane = (id: 'spots' | 'needed') => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)
/** The calls shown in one pane's board. */
const callsIn = (el: HTMLElement | null) =>
  [...(el?.querySelectorAll('.np-row:not(.np-header) .np-call') ?? [])].map((c) => c.textContent?.replace(/↗/g, ''))
const KEYING = [api.sendCw, api.setTune, api.setPtt, api.stopCw, api.haltTx]

describe('CW ships Phone’s Spots and Needed hidden, and ⊞ Panels is where they come from', () => {
  it('a fresh CW screen shows neither, ⊞ offers both unticked with no note, and the ⊞ button reads as it did', async () => {
    await mount(<Live />)
    expect(pane('spots'), 'Spots appeared on a stock CW screen').toBeNull()
    expect(pane('needed'), 'Needed appeared on a stock CW screen').toBeNull()
    expect(screen.getByRole('button', { name: /Panels/ }).textContent, 'the stock ⊞ button counts the feeds as hidden').toBe('⊞ Panels')
    expect(box(/^Spots$/).checked).toBe(false)
    expect(box(/^Needed$/).checked).toBe(false)
    expect(box(/^Spots$/).getAttribute('aria-describedby'), 'a hide that ends nothing carries no note').toBeNull()
    expect(box(/^Needed$/).getAttribute('aria-describedby')).toBeNull()
    expect(box(/CW Decode/).checked, 'a pane that shipped before them moved').toBe(true)
  })

  it('a CW record saved before this release loads unchanged, and shows neither', async () => {
    localStorage.setItem(CW_KEY, JSON.stringify({ v: 1, state: { copilot: 'removed' }, share: {} }))
    await mount(<Live />)
    expect(pane('spots')).toBeNull()
    expect(pane('needed')).toBeNull()
    expect(document.querySelector('[data-pane="copilot"]'), 'the stored choice was lost').toBeNull()
    expect(document.querySelector('[data-pane="decode"]')).not.toBeNull()
  })

  it('a tick docks the real board as a fill feed with its own ✕; Reset hides both, Undo brings them back; nothing keys', async () => {
    const r = await mount(<Live />)
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    for (const id of ['spots', 'needed'] as const) {
      expect(pane(id), `ticking ${id} did not dock it`).not.toBeNull()
      expect(pane(id)!.dataset.fit, 'a feed is a fill pane').toBe('fill')
      expect(pane(id)!.querySelector('.np-board'), 'the pane holds no board').not.toBeNull()
    }
    expect(record().state).toMatchObject({ spots: 'docked', needed: 'docked' })
    fireEvent.click(within(pane('needed')!).getByRole('button', { name: 'Hide Needed' }))
    expect(pane('needed'), 'the ✕ is not the same act as unticking').toBeNull()
    expect(record().state.needed).toBe('removed')
    fireEvent.click(box(/^Needed$/))
    r.unmount()
    await mount(<Live />)
    expect(pane('spots'), 'the tick did not survive a restart').not.toBeNull()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /reset layout/i }))
    expect(pane('spots'), 'Reset left Spots on screen').toBeNull()
    expect(pane('needed'), 'Reset left Needed on screen').toBeNull()
    expect(document.querySelector('[data-pane="decode"]'), 'Reset moved a stock pane').not.toBeNull()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /undo last change/i }))
    expect(pane('spots')).not.toBeNull()
    expect(pane('needed')).not.toBeNull()
    for (const wire of KEYING) expect(wire).not.toHaveBeenCalled()
  })
})

describe('the Spots pane: CW spots on the radio’s band', () => {
  it('opens on CW spots on the radio’s band — a human’s spot and a skimmer’s CW decode, never a skimmer’s RTTY or FT8 decode', async () => {
    await mount(<Live />)
    fireEvent.click(box(/^Spots$/))
    expect(callsIn(pane('spots')).sort()).toEqual(['K1CW', 'W1HUMAN'])
  })

  it('follows the band when the radio changes band', async () => {
    const r = await mount(<Live />)
    fireEvent.click(box(/^Spots$/))
    r.rerender(<Live band="40m" />)
    expect(callsIn(pane('spots'))).toEqual(['N2FORTY'])
  })

  it('its chips widen it, in a filter copy of its own that neither the Spots view nor Phone’s pane shares', async () => {
    const wiring = boards()
    const view = render(<SpotsPanel spots={SPOTS} {...wiring.spotsBoard} />)
    const viewCalls = () => [...view.container.querySelectorAll('.np-row:not(.np-header) .np-call')].map((c) => c.textContent)
    const viewRows = viewCalls().sort()
    await mount(<Live wiring={wiring} />)
    fireEvent.click(box(/^Spots$/))
    const spots = within(pane('spots')!)
    fireEvent.click(spots.getByRole('button', { name: /^Filter/ }))
    fireEvent.click(spots.getByRole('button', { name: 'RTTY' }))
    expect(callsIn(pane('spots')).sort()).toEqual(['K1CW', 'W1HUMAN', 'W9RTTY'])
    expect(JSON.parse(sessionStorage.getItem('nexus.spots.shownModes.cw')!)).toEqual(['CW', 'RTTY'])
    expect(sessionStorage.getItem('nexus.spots.shownModes.phone'), 'CW’s chips wrote Phone’s pane').toBeNull()
    expect(viewCalls().sort(), 'the pane’s chip moved the Spots view').toEqual(viewRows)
  })

  it('a row’s Work is the Spots view’s own handler, with the row it was given — and nothing keys', async () => {
    const wiring = boards()
    await mount(<Live wiring={wiring} />)
    fireEvent.click(box(/^Spots$/))
    const row = [...pane('spots')!.querySelectorAll<HTMLElement>('.np-row:not(.np-header)')].find(
      (r) => r.querySelector('.np-call')?.textContent === 'K1CW',
    )!
    fireEvent.click(row)
    expect(wiring.spotsBoard.onSelect).toHaveBeenCalledWith('K1CW')
    expect(wiring.spotsBoard.onWork).toHaveBeenCalledTimes(1)
    expect(wiring.spotsBoard.onWork).toHaveBeenCalledWith(SPOTS[0])
    for (const wire of [...KEYING, api.setFrequency]) expect(wire, 'the pane touched the rig itself').not.toHaveBeenCalled()
  })
})

describe('the Needed pane', () => {
  it('opens on the CW needs, in a filter record of its own; its chips widen it', async () => {
    await mount(<Live />)
    fireEvent.click(box(/^Needed$/))
    expect(callsIn(pane('needed')).sort()).toEqual(['JA1CW', 'JA2CW'])
    const needed = within(pane('needed')!)
    fireEvent.click(needed.getByRole('button', { name: /^Filter/ }))
    fireEvent.click(needed.getByRole('button', { name: 'Phone' }))
    expect(callsIn(pane('needed')).sort()).toEqual(['JA1CW', 'JA2CW', 'K1PHONE'])
    expect(JSON.parse(localStorage.getItem('nexus.cw.neededFilters')!).modes).toEqual({ Digital: false, CW: true, Phone: true })
    expect(localStorage.getItem('neededFilters'), 'the pane wrote the Needed view’s record').toBeNull()
    expect(localStorage.getItem('nexus.phone.neededFilters'), 'the pane wrote Phone’s record').toBeNull()
  })

  it('a row goes through the Needed view’s own handler', async () => {
    const wiring = boards()
    await mount(<Live wiring={wiring} />)
    fireEvent.click(box(/^Needed$/))
    const row = [...pane('needed')!.querySelectorAll<HTMLElement>('.np-row:not(.np-header)')][0]
    fireEvent.click(row)
    expect(wiring.neededBoard.onWork).toHaveBeenCalledWith(ALERTS[0])
    for (const wire of KEYING) expect(wire).not.toHaveBeenCalled()
  })
})

describe('the feeds’ places', () => {
  const region = () => document.querySelector('.cockpit-panes')!
  const cols = () => [...region().querySelectorAll(':scope > .cockpit-col')] as HTMLElement[]
  const framesIn = (col: Element) => [...col.querySelectorAll(':scope > .pane-frame')].map((f) => f.getAttribute('data-pane'))
  const sep = () => screen.queryByRole('separator', { name: /Spots \/ Needed/ })
  async function tier(width: number) {
    Object.defineProperty(region(), 'clientWidth', { configurable: true, get: () => width })
    act(() => fire!())
    await act(async () => {
      await new Promise((r) => requestAnimationFrame(() => r(null)))
    })
  }

  it('Spots at the foot of Decode’s column and Needed of the middle one at three tracks; below that both after every strip', async () => {
    decodeState.sent = ['CQ CQ DE KD9TAW K']
    localStorage.setItem(CW_KEY, JSON.stringify({ v: 1, state: { spots: 'docked', needed: 'docked' }, share: {} }))
    await mount(<Live />)
    await tier(1800)
    expect(cols().map(framesIn)).toEqual([['decode', 'sent', 'spots'], ['rigctl', 'bandActivity', 'copilot', 'needed'], ['log']])
    expect(sep(), 'a divider between feeds in different columns').toBeNull()
    // One merged column: the feeds come last, so ticking one never pushes the Rig controls, Band
    // Activity or the copilot down it (Phone's rule), and there they are a pair, with a divider.
    await tier(1200)
    expect(cols().map(framesIn)).toEqual([['decode', 'sent', 'rigctl', 'bandActivity', 'copilot', 'spots', 'needed'], ['log']])
    expect(sep()!.previousElementSibling).toBe(pane('spots'))
    expect(sep()!.nextElementSibling).toBe(pane('needed'))
    await tier(900)
    expect(cols().map(framesIn), 'one track stacks the same order').toEqual([
      ['decode', 'sent', 'rigctl', 'bandActivity', 'copilot', 'spots', 'needed'],
      ['log'],
    ])
  })

  it('with Band Activity and the copilot hidden, the Rig controls still stand ahead of the feeds', async () => {
    const state = { spots: 'docked', needed: 'docked', bandActivity: 'removed', copilot: 'removed' }
    localStorage.setItem(CW_KEY, JSON.stringify({ v: 1, state, share: {} }))
    await mount(<Live />)
    await tier(1200)
    expect(cols().map(framesIn)).toEqual([['decode', 'rigctl', 'spots', 'needed'], ['log']])
    expect(sep()!.previousElementSibling).toBe(pane('spots'))
  })

  it('arranged into a pair, a divider stands between them and commits the split to the CW record on release', async () => {
    // ⊞ Arrange moved both feeds to the head of the middle column (a placed pane leads its column).
    const place = { spots: { col: 'b', order: 0 }, needed: { col: 'b', order: 1 } }
    localStorage.setItem(CW_KEY, JSON.stringify({ v: 2, state: { spots: 'docked', needed: 'docked' }, share: {}, place }))
    await mount(<Live />)
    await tier(1800)
    expect(framesIn(cols()[1])).toEqual(['rigctl', 'spots', 'needed', 'bandActivity', 'copilot'])
    expect(sep()!.previousElementSibling).toBe(pane('spots'))
    expect(sep()!.nextElementSibling).toBe(pane('needed'))
    const stub = (el: HTMLElement, top: number, bottom: number) => {
      el.getBoundingClientRect = () => ({ top, bottom, left: 0, right: 600, width: 600, height: bottom - top, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
    }
    stub(pane('spots')!, 100, 300)
    stub(pane('needed')!, 310, 500)
    fireEvent.pointerDown(sep()!, { clientY: 205, pointerId: 1 })
    fireEvent.pointerMove(window, { clientY: 300, pointerId: 1 })
    expect(record().share.spots, 'committed before release').toBeUndefined()
    fireEvent.pointerUp(window, { clientY: 340, pointerId: 1 })
    const [a, b] = seamShares((340 - 100) / 400)
    expect(record().share).toEqual({ spots: a, needed: b })
    fireEvent.click(within(pane('needed')!).getByRole('button', { name: 'Hide Needed' }))
    expect(sep(), 'a divider with one feed').toBeNull()
  })
})

describe('on the Remote page', () => {
  // ONE state object per source, as the real store keeps them (see PhoneCockpit.boards.test.tsx).
  const source = (phase: 'unavailable' | 'ready') => {
    const state = { phase, total: 0, retained: 0, at: 0 }
    return { subscribe: () => () => {}, state: () => state } as unknown as RemoteCollections
  }
  const observe = (phase: 'unavailable' | 'ready') => (
    <StationControlContext.Provider value={false}>
      <RemoteCollectionsContext.Provider value={source(phase)}>
        <Live />
      </RemoteCollectionsContext.Provider>
    </StationControlContext.Provider>
  )

  it('a pane whose data the page does not carry shows the existing unavailable status; with it, the board', async () => {
    const r = await mount(observe('unavailable'))
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    expect(pane('spots')!.querySelector('.np-board'), 'a board drawn with no station data').toBeNull()
    expect(pane('spots')!.textContent).toContain('Remote spot data unavailable.')
    expect(pane('needed')!.querySelector('.np-board')).toBeNull()
    expect(pane('needed')!.textContent).toMatch(/unavailable/i)
    r.unmount()
    await mount(observe('ready'))
    expect(callsIn(pane('spots')).sort()).toEqual(['K1CW', 'W1HUMAN'])
    expect(pane('needed')!.querySelector('.np-board')).not.toBeNull()
  })
})

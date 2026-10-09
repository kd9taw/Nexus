// @vitest-environment jsdom
//
// SPOTS AND NEEDED ON THE PHONE SCREEN (#345) — the two boards as Phone panes.
//
// The tester's ask: "Dockable panels on the phone interface that would allow us to bring in
// things like spot and needed … the way I use phone right now leaves me with much empty real
// estate on my screen." The operator's picks, verbatim:
//   · "Hidden, add via ⊞ Panels (Recommended)" — nobody's Phone screen changes on update;
//   · "Phone spots, this band (Recommended)" — SSB/AM/FM spots on the band the radio is on,
//     like a phone operator's band map, and its filter chips can widen it.
//
// Everything here runs against the REAL panel record (`usePanelLayout(PHONE_PANELS)`, exactly as
// App builds it), the REAL ⊞ menu and the REAL boards. A fake record cannot serve: the default is
// a property of the record, and a stub whose `stateOf` answers 'docked' for every id would show
// both panes and prove nothing about who sees them on update.
//
// What it pins:
//   · both panes are hidden on a fresh screen AND on a screen whose record predates them, a tick
//     docks one and is stored, ⊞ Reset hides them again, Undo undoes, the ✕ is the same act;
//   · the Spots pane opens on phone spots on the radio's band, follows the band, and its chips
//     widen it — with its filters in keys of its own, so neither it nor the Spots view can move
//     the other's;
//   · the Needed pane opens on Phone needs in a record of its own, and its chips widen it;
//   · the divider between them commits the split to the Phone record;
//   · working a spot from the pane is the Spots view's own handler, and nothing here keys;
//   · on Remote, a pane whose data the page does not have says so with the existing status.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { SpotsPanel } from './SpotsPanel'
import { NeededPanel } from './NeededPanel'
import { PHONE_PANELS, panelStorageKey, seamShares, usePanelLayout } from '../features/panelState'
import { StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import type { AppSnapshot, BandChannel, NeedAlert, SpotRow } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "a fresh Phone screen shows neither, and ⊞ Panels offers…", takes
// 0.39 s and 0.35 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => ({
  getSettings: vi.fn(async () => ({})),
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
  setTxEnabled: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  openQrzPage: vi.fn(async () => {}),
  readRotator: vi.fn(async () => null),
  pointRotator: vi.fn(async () => {}),
  stopVoice: vi.fn(async () => ({})),
  playVoiceMessage: vi.fn(async () => ({})),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The header hosts the ⊞ menu, which the cockpit hands it as `actions`; the rest is stubbed.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ actions }: { actions?: unknown }) => (
    <header className="cockpit-header">{actions as never}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('../remote-web/RemoteRecall', () => ({ RemoteRecallEntry: () => <div data-testid="recall-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

/** Fire a resize the way the browser does: to EVERY live observer. The region's (useRegionCols)
 *  is no longer the only one — with the real panel record the log column's width divider
 *  observes too (panes/RegionColumnSeams), and a harness that kept the last one constructed
 *  fired the divider's instead of the region's. */
let fire: (() => void) | null = null
beforeEach(() => {
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

function makeSnap(band = '20m', dialMhz = 14.25): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz,
      band,
      catOk: true,
      sideband: 'USB',
      sidebandOverride: null,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      qsoRecording: false,
      rfPower: null,
      micGain: null,
      nrLevel: null,
      agc: null,
      nb: null,
      nr: null,
      notch: null,
      comp: null,
      vox: null,
      filterWidthHz: null,
      splitTxMhz: null,
      smeterDb: null,
      rxLevel: 0,
      phoneSegLo: null,
      phoneSegHi: null,
    },
  } as unknown as AppSnapshot
}

const spot = (call: string, band: string, freqMhz: number, mode: string, submode: string | null = null): SpotRow => ({
  call,
  entity: 'United States',
  zone: 5,
  band,
  freqMhz,
  mode,
  submode,
  spotter: 'W3LPL',
  corroborators: [],
  ageSecs: 30,
  comment: '',
  licensed: true,
  spotterLocal: true,
  workedAgoSecs: null,
  workedTodayUtc: false,
})
// `mode` is the backend's frequency-derived CLASS (propagation::classify_spot_mode: CW | Phone |
// Digital) and `submode` is only ever an RBN skimmer's token (CW, FT8, RTTY…) — never SSB/AM/FM.
const SPOTS: SpotRow[] = [
  spot('W1AW', '20m', 14.25, 'Phone'),
  spot('K1CW', '20m', 14.025, 'CW', 'CW'),
  spot('JA1FT', '20m', 14.074, 'Digital', 'FT8'),
  spot('N2FORTY', '40m', 7.2, 'Phone'),
  // A CW skimmer's decode INSIDE the 20 m phone segment: the class says Phone, the wire says CW.
  // It is a CW signal, and a phone operator's band map must not show it as a voice station.
  spot('W9SKIM', '20m', 14.2, 'Phone', 'CW'),
]

const BAND_PLAN: BandChannel[] = [
  { band: '20m', group: 'HF', dialMhz: 14.074, mode: 'USB' as never, label: '20m', note: '' },
  { band: '40m', group: 'HF', dialMhz: 7.074, mode: 'USB' as never, label: '40m', note: '' },
]

const alert = (call: string, band: string, mode: string, freqMhz: number | null): NeedAlert => ({
  call,
  entity: 'Japan',
  band,
  zone: 25,
  tags: ['NewBand'] as never,
  priority: 50,
  headline: `${call} needed`,
  mode,
  freqMhz,
})
// Two phone needs, then a CW one and a digital one (FT8 is a Digital submode to the board's filter),
// so the Phone pane's opening default has something to leave out.
const ALERTS: NeedAlert[] = [
  alert('JA1AAA', '20m', 'Phone', 14.21),
  alert('JA2BBB', '40m', 'Phone', 7.18),
  alert('K1NEED', '20m', 'CW', 14.025),
  alert('VK9FT8', '20m', 'FT8', 14.074),
]

function boards() {
  return {
    spotsBoard: {
      bandPlan: BAND_PLAN,
      selectedCall: null,
      myGrid: 'EN52',
      onSelect: vi.fn(),
      onWork: vi.fn(),
      needAlerts: [] as NeedAlert[],
    },
    neededBoard: {
      alerts: ALERTS,
      bandPlan: BAND_PLAN,
      selectedCall: null,
      myGrid: 'EN52',
      onQsy: vi.fn(),
      onSelect: vi.fn(),
      onWork: vi.fn(),
      phoneSource: null,
    },
  }
}

/** The cockpit with the real panel record and the board wiring App passes. */
function Live({ band = '20m', wiring = boards() }: { band?: string; wiring?: ReturnType<typeof boards> }) {
  const panels = usePanelLayout(PHONE_PANELS)
  return (
    <PhoneCockpit
      snap={makeSnap(band, band === '40m' ? 7.2 : 14.25)}
      theme="dark"
      onWorkSpot={() => {}}
      spots={SPOTS}
      panels={panels}
      spotsBoard={wiring.spotsBoard}
      neededBoard={wiring.neededBoard}
    />
  )
}

const PHONE_KEY = panelStorageKey('phone')
const record = () => JSON.parse(localStorage.getItem(PHONE_KEY) ?? '{"state":{},"share":{}}')

/** Open ⊞ Panels (idempotent — the trigger TOGGLES) and return the popover. */
function openMenu() {
  const btn = screen.getByRole('button', { name: /Panels/ })
  if (btn.getAttribute('aria-expanded') !== 'true') fireEvent.click(btn)
  return screen.getByRole('group', { name: /panels on this screen/i })
}
function box(label: RegExp) {
  return within(openMenu()).getByRole('checkbox', { name: label }) as HTMLInputElement
}
const pane = (id: 'spots' | 'needed') => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)
/** The calls shown in one pane's board, in order. */
const callsIn = (el: HTMLElement | null) =>
  [...(el?.querySelectorAll('.np-row:not(.np-header) .np-call') ?? [])].map((c) => c.textContent)

describe('Spots and Needed ship hidden, and the ⊞ menu is where they come from', () => {
  it('a fresh Phone screen shows neither, and ⊞ Panels offers both, unticked, with no warning', () => {
    render(<Live />)
    expect(pane('spots'), 'the Spots pane appeared on a stock screen').toBeNull()
    expect(pane('needed'), 'the Needed pane appeared on a stock screen').toBeNull()
    expect(box(/^Spots$/).checked).toBe(false)
    expect(box(/^Needed$/).checked).toBe(false)
    // Every pane that shipped before them is exactly where it was.
    expect(box(/Voice Keyer/).checked).toBe(true)
    expect(box(/Band Activity/).checked).toBe(true)
    // Their hide ends nothing, so their entries carry no note (THE PRACTICE).
    expect(box(/^Spots$/).getAttribute('aria-describedby')).toBeNull()
    expect(box(/^Needed$/).getAttribute('aria-describedby')).toBeNull()
  })

  it('the ⊞ button reads exactly as it did: a pane that ships hidden is not one the operator hid', () => {
    // Found by the pixel diff against the previous build, and invisible to every assertion above:
    // the button counts unticked entries, so a stock screen read "⊞ Panels · 2 hidden", lit up, on
    // every operator's Phone screen — a change on update, and a false one.
    render(<Live />)
    const btn = screen.getByRole('button', { name: /Panels/ })
    expect(btn.textContent).toBe('⊞ Panels')
    expect(btn.classList.contains('active'), 'the stock ⊞ button is lit').toBe(false)
    // An operator's own hide still counts…
    fireEvent.click(box(/Voice Keyer/))
    expect(btn.textContent).toBe('⊞ Panels · 1 hidden')
    // …and a ticked feed counts nothing, ticked or unticked again: that is its stock state.
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Spots$/))
    expect(btn.textContent).toBe('⊞ Panels · 1 hidden')
  })

  it('a Phone record saved before this release (no entry for either) still shows neither', () => {
    localStorage.setItem(PHONE_KEY, JSON.stringify({ v: 1, state: { receiver: 'removed' }, share: {} }))
    render(<Live />)
    expect(pane('spots')).toBeNull()
    expect(pane('needed')).toBeNull()
    expect(document.querySelector('[data-pane="receiver"]'), 'the stored choice was lost').toBeNull()
  })

  it('a tick docks the pane as a fill feed with its own ✕, is stored, and survives a restart', () => {
    const r = render(<Live />)
    fireEvent.click(box(/^Spots$/))
    const frame = pane('spots')
    expect(frame, 'ticking Spots did not dock it').not.toBeNull()
    expect(frame!.dataset.fit, 'a feed is a fill pane, never a content strip').toBe('fill')
    expect(frame!.querySelector('.np-board'), 'the pane holds no board').not.toBeNull()
    expect(within(frame!).getByRole('button', { name: 'Hide Spots' })).toBeTruthy()
    expect(record().state.spots).toBe('docked')
    r.unmount()
    render(<Live />)
    expect(pane('spots'), 'the tick did not survive a restart').not.toBeNull()
    expect(pane('needed')).toBeNull()
  })

  it('the pane’s ✕ is the same act as unticking it', () => {
    render(<Live />)
    fireEvent.click(box(/^Needed$/))
    expect(pane('needed')).not.toBeNull()
    fireEvent.click(within(pane('needed')!).getByRole('button', { name: 'Hide Needed' }))
    expect(pane('needed')).toBeNull()
    expect(record().state.needed).toBe('removed')
  })

  it('⊞ Reset hides them again, Undo brings them back, and neither touches the transmitter', () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    expect(pane('spots')).not.toBeNull()
    expect(pane('needed')).not.toBeNull()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /reset layout/i }))
    expect(pane('spots'), 'Reset left Spots on screen').toBeNull()
    expect(pane('needed'), 'Reset left Needed on screen').toBeNull()
    // The keyer is docked by default, so Reset leaves it exactly where it was.
    expect(document.querySelector('[data-pane="voiceKeyer"]')).not.toBeNull()
    fireEvent.click(within(openMenu()).getByRole('button', { name: /undo last change/i }))
    expect(pane('spots')).not.toBeNull()
    expect(pane('needed')).not.toBeNull()
    for (const wire of [api.setPtt, api.haltTx, api.setTune, api.stopVoice, api.playVoiceMessage]) {
      expect(wire).not.toHaveBeenCalled()
    }
  })

  it('an Undo that would hide one of them says nothing about ending a transmission', () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    const undo = within(openMenu()).getByRole('button', { name: /undo last change/i })
    expect((undo as HTMLButtonElement).disabled).toBe(false)
    expect(undo.getAttribute('aria-describedby'), 'a warning on a hide that ends nothing').toBeNull()
  })
})

describe('the Spots pane: phone spots on the radio’s band', () => {
  it('opens on phone spots on the band the radio is on — no CW, digital or skimmer rows', () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    expect(callsIn(pane('spots'))).toEqual(['W1AW'])
  })

  it('follows the band when the radio changes band', () => {
    const r = render(<Live />)
    fireEvent.click(box(/^Spots$/))
    expect(callsIn(pane('spots'))).toEqual(['W1AW'])
    r.rerender(<Live band="40m" />)
    expect(callsIn(pane('spots'))).toEqual(['N2FORTY'])
  })

  it('its chips widen it: another mode, another band', () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    const spots = within(pane('spots')!)
    fireEvent.click(spots.getByRole('button', { name: /^Filter/ }))
    fireEvent.click(spots.getByRole('button', { name: 'CW' }))
    expect(callsIn(pane('spots')).sort()).toEqual(['K1CW', 'W1AW', 'W9SKIM'])
    fireEvent.click(spots.getByRole('button', { name: '40m' }))
    expect(callsIn(pane('spots')).sort()).toEqual(['K1CW', 'N2FORTY', 'W1AW', 'W9SKIM'])
  })

  it('unticking the radio’s own band chip stops following it and shows every band', () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    const spots = within(pane('spots')!)
    fireEvent.click(spots.getByRole('button', { name: /^Filter/ }))
    const rigBand = spots.getByRole('button', { name: '20m' })
    expect(rigBand.getAttribute('title'), 'the chip that follows the radio does not say so').toMatch(/20m/)
    fireEvent.click(rigBand)
    expect(callsIn(pane('spots')).sort()).toEqual(['N2FORTY', 'W1AW'])
  })

  it('the pane and the Spots view keep separate filters — neither moves the other', () => {
    const wiring = boards()
    const view = render(<SpotsPanel spots={SPOTS} {...wiring.spotsBoard} />)
    const viewCalls = () => [...view.container.querySelectorAll('.np-row:not(.np-header) .np-call')].map((c) => c.textContent)
    render(<Live wiring={wiring} />)
    fireEvent.click(box(/^Spots$/))
    // The view's own keys are `nexus.spots.<filter>`; the pane's copies carry its scope suffix.
    const viewKeys = () =>
      Object.fromEntries(
        Object.keys(sessionStorage)
          .filter((k) => /^nexus\.spots\.[A-Za-z]+$/.test(k))
          .map((k) => [k, sessionStorage.getItem(k)]),
      )
    const before = viewKeys()
    expect(Object.keys(before).length, 'fixture: the view stored its filters').toBeGreaterThan(0)
    const viewRows = viewCalls()
    expect(viewRows.sort(), 'fixture: the view shows every spot').toEqual(['JA1FT', 'K1CW', 'N2FORTY', 'W1AW', 'W9SKIM'])

    // The pane's chips move the pane only…
    const spots = within(pane('spots')!)
    fireEvent.click(spots.getByRole('button', { name: /^Filter/ }))
    fireEvent.click(spots.getByRole('button', { name: 'CW' }))
    fireEvent.click(spots.getByRole('button', { name: '40m' }))
    expect(viewKeys(), 'the pane wrote the Spots view’s filters').toEqual(before)
    expect(viewCalls().sort()).toEqual(viewRows.sort())
    // …into its own copies (so "untouched" above is not just "nothing was written").
    expect(JSON.parse(sessionStorage.getItem('nexus.spots.shownModes.phone')!)).toEqual(['Phone', 'CW'])
    expect(JSON.parse(sessionStorage.getItem('nexus.spots.bands.phone')!)).toEqual(['40m'])

    // …and the view's chips move the view only.
    const paneBefore = callsIn(pane('spots')).sort()
    fireEvent.click(within(view.container).getByRole('button', { name: 'Phone' }))
    expect(viewCalls()).not.toContain('W1AW')
    expect(callsIn(pane('spots')).sort(), 'the view’s chip moved the pane').toEqual(paneBefore)
  })
})

describe('the Needed pane keeps a filter record of its own', () => {
  // The operator's pick for it (2026-09-27): "Phone needs only — matches the Spots pane (phone
  // spots, this band). Its chips still widen it." A FIRST-RUN default of the pane's own record:
  // the Needed view keeps the board's defaults, every mode, and neither moves the other.
  it('opens on Phone needs only, its chips widen it, and neither it nor the Needed view moves the other', () => {
    const wiring = boards()
    const view = render(<NeededPanel {...wiring.neededBoard} />)
    const viewCalls = () => [...view.container.querySelectorAll('.np-row:not(.np-header) .np-call button')].map((c) => c.textContent)
    const paneCalls = () => callsIn(pane('needed')).map((c) => c?.replace(/↗/g, '')).sort()
    const r = render(<Live wiring={wiring} />)
    fireEvent.click(box(/^Needed$/))
    expect(paneCalls(), 'a fresh Needed pane opened on more than the Phone needs').toEqual(['JA1AAA', 'JA2BBB'])
    // …while the view, on its own record, still shows every mode.
    expect(viewCalls().sort()).toEqual(['JA1AAA', 'JA2BBB', 'K1NEED', 'VK9FT8'])
    const viewKey = localStorage.getItem('neededFilters')

    // A chip widens it, and the widening is the pane's own record from then on.
    let needed = within(pane('needed')!)
    fireEvent.click(needed.getByRole('button', { name: /^Filter/ }))
    fireEvent.click(needed.getByRole('button', { name: 'CW' }))
    expect(paneCalls()).toEqual(['JA1AAA', 'JA2BBB', 'K1NEED'])
    expect(JSON.parse(localStorage.getItem('nexus.phone.neededFilters')!).modes).toEqual({ Digital: false, CW: true, Phone: true })
    r.unmount()
    render(<Live wiring={wiring} />)
    expect(paneCalls(), 'a restart put the opening default back over the operator’s own pick').toEqual(['JA1AAA', 'JA2BBB', 'K1NEED'])

    needed = within(pane('needed')!)
    fireEvent.click(needed.getByRole('button', { name: /^Filter/ }))
    fireEvent.click(needed.getByRole('button', { name: '40m' }))
    expect(paneCalls()).toEqual(['JA2BBB'])
    expect(localStorage.getItem('neededFilters'), 'the pane wrote the Needed view’s filters').toBe(viewKey)
    expect(JSON.parse(localStorage.getItem('nexus.phone.neededFilters')!).bands).toEqual(['40m'])
    expect(viewCalls().sort()).toEqual(['JA1AAA', 'JA2BBB', 'K1NEED', 'VK9FT8'])

    fireEvent.click(within(view.container).getByRole('button', { name: /^Filter/ }))
    fireEvent.click(within(view.container).getByRole('button', { name: '20m' }))
    expect(viewCalls().sort()).toEqual(['JA1AAA', 'K1NEED', 'VK9FT8'])
    expect(paneCalls(), 'the view’s chip moved the pane').toEqual(['JA2BBB'])
  })
})

describe('the divider between the two panes', () => {
  /** Stub a frame's box — jsdom lays nothing out. */
  function box2(el: HTMLElement, top: number, bottom: number) {
    el.getBoundingClientRect = () => ({ top, bottom, left: 0, right: 600, width: 600, height: bottom - top, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
  }

  it('is there only while both share a column in the bounded flow — tier 2', async () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    const sep = () => screen.queryByRole('separator', { name: /Spots \/ Needed/ })
    // The region starts at tier 1 (jsdom: width 0): the stacking flow, where a share is inert.
    expect(sep()).toBeNull()
    const region = document.querySelector('.cockpit-panes')!
    const width = async (w: number) => {
      Object.defineProperty(region, 'clientWidth', { configurable: true, get: () => w })
      act(() => fire!())
      await act(async () => {
        await new Promise((r) => requestAnimationFrame(() => r(null)))
      })
    }
    await width(1200)
    expect(region.getAttribute('data-cols')).toBe('2')
    // Between the two, in the same column — and marked as a column's divider, the class the sheet
    // keys its in-gap margins on (styles.css `.in-column`; cockpit-shells.test.ts computes the net).
    expect(sep()!.previousElementSibling).toBe(pane('spots'))
    expect(sep()!.nextElementSibling).toBe(pane('needed'))
    expect(sep()!.parentElement!.classList.contains('cockpit-col')).toBe(true)
    expect(sep()!.classList.contains('in-column'), 'the divider takes a 12 px gap of its own').toBe(true)
    // Tier 3 puts Needed under the strips in the middle column: nothing to split, no divider.
    await width(1800)
    expect(region.getAttribute('data-cols')).toBe('3')
    expect(sep()).toBeNull()
    const cols = region.querySelectorAll(':scope > .cockpit-col')
    expect(pane('spots')!.parentElement).toBe(cols[0])
    expect(pane('needed')!.parentElement).toBe(cols[1])
    await width(1200)
    expect(sep()).not.toBeNull()
    fireEvent.click(within(pane('needed')!).getByRole('button', { name: 'Hide Needed' }))
    expect(sep()).toBeNull()
  })

  it('paints the split live and commits it to the Phone record on release', async () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    const region = document.querySelector('.cockpit-panes')!
    Object.defineProperty(region, 'clientWidth', { configurable: true, get: () => 1200 })
    act(() => fire!())
    await act(async () => {
      await new Promise((r) => requestAnimationFrame(() => r(null)))
    })
    const spots = pane('spots')!
    const needed = pane('needed')!
    box2(spots, 100, 300)
    box2(needed, 310, 500)
    const sep = screen.getByRole('separator', { name: /Spots \/ Needed/ })
    fireEvent.pointerDown(sep, { clientY: 205, pointerId: 1 })
    fireEvent.pointerMove(window, { clientY: 300, pointerId: 1 })
    const [a1] = seamShares((300 - 100) / 400)
    expect(spots.style.getPropertyValue('--pane-share'), 'the drag did not repaint the pane').toBe(String(a1))
    expect(record().share.spots, 'committed before release').toBeUndefined()
    fireEvent.pointerUp(window, { clientY: 340, pointerId: 1 })
    const [a, b] = seamShares((340 - 100) / 400)
    expect(record().share).toEqual({ spots: a, needed: b })
    // …and the record's value is what the frames carry from then on.
    expect(pane('spots')!.style.getPropertyValue('--pane-share')).toBe(String(a))
    expect(pane('needed')!.style.getPropertyValue('--pane-share')).toBe(String(b))
  })

  it('while the divider is there the feeds’ floors follow their shares; a lone feed keeps the stock floor', async () => {
    // L1 left this divider inert at 1024×768 and 1366×768 (measured in Chrome): the leading
    // column is too short for both feeds' floors, so both sat on them and a share moved nothing.
    // The floor now follows the share — but only for the PAIR the divider splits: at tier 3 each
    // feed is alone in its column, and hiding one leaves the other alone too.
    localStorage.setItem(PHONE_KEY, JSON.stringify({ v: 2, state: { spots: 'docked', needed: 'docked' }, share: { spots: 1.6, needed: 0.4 } }))
    render(<Live />)
    const region = document.querySelector('.cockpit-panes')!
    const width = async (w: number) => {
      Object.defineProperty(region, 'clientWidth', { configurable: true, get: () => w })
      act(() => fire!())
      await act(async () => {
        await new Promise((r) => requestAnimationFrame(() => r(null)))
      })
    }
    const SPLIT_FLOOR = 'min(calc(var(--cockpit-fill-min, 0px) * var(--pane-share, 1) / 1), 100%)'
    const STOCK_FLOOR = 'var(--cockpit-fill-min, 0)'
    await width(1200)
    expect(pane('spots')!.style.minHeight).toBe(SPLIT_FLOOR)
    expect(pane('needed')!.style.minHeight).toBe(SPLIT_FLOOR)
    expect(pane('spots')!.style.getPropertyValue('--pane-share')).toBe('1.6')
    await width(1800)
    expect(pane('spots')!.style.minHeight, 'tier 3: Spots is alone in its column').toBe(STOCK_FLOOR)
    expect(pane('spots')!.style.getPropertyValue('--pane-share')).toBe('')
    expect(pane('needed')!.style.minHeight).toBe(STOCK_FLOOR)
    await width(1200)
    fireEvent.click(within(pane('needed')!).getByRole('button', { name: 'Hide Needed' }))
    expect(pane('spots')!.style.minHeight, 'Needed hidden: Spots is alone').toBe(STOCK_FLOOR)
  })

  it('answers the keyboard from where the panes are, and Backspace puts the stock split back (PaneSeam)', async () => {
    render(<Live />)
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    const region = document.querySelector('.cockpit-panes')!
    Object.defineProperty(region, 'clientWidth', { configurable: true, get: () => 1200 })
    act(() => fire!())
    await act(async () => {
      await new Promise((r) => requestAnimationFrame(() => r(null)))
    })
    box2(pane('spots')!, 100, 300)
    box2(pane('needed')!, 310, 500)
    const sep = screen.getByRole('separator', { name: /Spots \/ Needed/ })
    expect(sep.tabIndex, 'a divider only a mouse can reach').toBe(0)
    // 200 : 190 on screen, so the step starts from there — one arrow, one committed step.
    fireEvent.keyDown(sep, { key: 'ArrowDown' })
    const [a, b] = seamShares(200 / 390 + 0.05)
    expect(record().share).toEqual({ spots: a, needed: b })
    fireEvent.keyDown(sep, { key: 'Backspace' })
    expect(record().share, 'reset leaves no share of its own behind').toEqual({})
  })
})

describe('working a spot from the pane', () => {
  it('is the Spots view’s own handler, with the row it was given — and keys nothing', () => {
    const wiring = boards()
    render(<Live wiring={wiring} />)
    fireEvent.click(box(/^Spots$/))
    const row = [...pane('spots')!.querySelectorAll<HTMLElement>('.np-row:not(.np-header)')][0]
    fireEvent.click(row)
    expect(wiring.spotsBoard.onSelect).toHaveBeenCalledWith('W1AW')
    expect(wiring.spotsBoard.onWork).toHaveBeenCalledTimes(1)
    expect(wiring.spotsBoard.onWork).toHaveBeenCalledWith(SPOTS[0])
    for (const wire of [api.setPtt, api.haltTx, api.setTune, api.setTxEnabled, api.playVoiceMessage, api.setFrequency]) {
      expect(wire, 'working a spot from the pane touched the rig directly').not.toHaveBeenCalled()
    }
  })

  it('a Needed row goes through the Needed view’s own handler', () => {
    const wiring = boards()
    render(<Live wiring={wiring} />)
    fireEvent.click(box(/^Needed$/))
    const row = [...pane('needed')!.querySelectorAll<HTMLElement>('.np-row:not(.np-header)')][0]
    fireEvent.click(row)
    expect(wiring.neededBoard.onWork).toHaveBeenCalledWith(ALERTS[0])
    expect(api.setPtt).not.toHaveBeenCalled()
  })
})

describe('on the Remote page', () => {
  // ONE state object per source, as the real store keeps them in a Map: useSyncExternalStore
  // reads a fresh object on every call as a change, and re-renders until React gives up.
  const source = (phase: 'loading' | 'ready' | 'unavailable') => {
    const state = { phase, total: 0, retained: 0, at: 0 }
    return { subscribe: () => () => {}, state: () => state } as unknown as RemoteCollections
  }

  it('a pane whose data the page does not carry shows the existing unavailable status', () => {
    render(
      <StationControlContext.Provider value={false}>
        <RemoteCollectionsContext.Provider value={source('unavailable')}>
          <Live />
        </RemoteCollectionsContext.Provider>
      </StationControlContext.Provider>,
    )
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    expect(pane('spots')!.querySelector('.np-board'), 'a board drawn with no station data').toBeNull()
    expect(pane('spots')!.textContent).toContain('Remote spot data unavailable.')
    expect(pane('needed')!.querySelector('.np-board')).toBeNull()
    expect(pane('needed')!.textContent).toMatch(/unavailable/i)
  })

  it('with the collections ready, the same panes draw the boards from what the page already has', () => {
    render(
      <StationControlContext.Provider value={false}>
        <RemoteCollectionsContext.Provider value={source('ready')}>
          <Live />
        </RemoteCollectionsContext.Provider>
      </StationControlContext.Provider>,
    )
    fireEvent.click(box(/^Spots$/))
    fireEvent.click(box(/^Needed$/))
    expect(callsIn(pane('spots'))).toEqual(['W1AW'])
    expect(pane('needed')!.querySelector('.np-board')).not.toBeNull()
  })
})

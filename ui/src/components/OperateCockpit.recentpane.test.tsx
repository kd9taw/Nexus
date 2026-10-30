// @vitest-environment jsdom
//
// THE RECENT CONTACTS PANE IN FT. Phone, CW, JS8, RTTY and PSK list the last few contacts in a pane under
// their log entry form; FT logs through its confirm dialog and has no form, so its pane stands at the foot of
// the side rail, at its content height. Ticking it keeps FT's stock layout (Classic's Rx/Tx column and its
// Tx messages divider untouched); the rail it joins floors its feeds and scrolls instead of squashing them.
// These pin:
//  1. it ships hidden, and a record that never mentions it draws the stock rail exactly as before;
//  2. ticked, it stands last in the STOCK rail in both layouts, through CockpitPaneFrame, content-fit, and FT
//     is not arranged: Classic's Rx/Tx column keeps its divider;
//  3. ticked, the rail's feeds take the yielding floor and the card keeps its size; unticked, neither;
//  4. in the arranged columns (arranged for another reason) it stands last in the rail column;
//  5. a row opens that contact in the Logbook's editor, as in every other cockpit;
//  6. it stands the rail by itself when nothing else in the rail is shown.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QrzLookup } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'
import { installRecentLog, recentRows, resetRecentLog } from './RecentQsosPane.testkit'

// THE BUDGET (2026-10-09). The slowest case here, "is listed in ⊞ Panels in both layouts", takes 0.34 s and 0.74 s on
// one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than one core, past
// vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const resolved: QrzLookup = {
  call: 'W1ABC',
  name: 'Alice Example',
  nickname: null,
  qth: 'Hartford, CT',
  grid: 'FN31',
  state: 'CT',
  country: 'United States',
  dxcc: 291,
  cqZone: 5,
  ituZone: 8,
  image: null,
}

const startSearchAndPounce = vi.fn(async () => null)
vi.mock('../api', () => ({
  startSearchAndPounce: () => startSearchAndPounce(),
  qrzLookup: vi.fn(async () => resolved),
  resolveEntity: vi.fn(async () => 'United States'),
  getSettings: vi.fn(() => Promise.resolve({})),
  setSettings: vi.fn(async () => null),
  openPanelWindow: vi.fn(async () => null),
  notifyErase: vi.fn(async () => null),
  pointRotatorAtCall: vi.fn(async () => null),
  redecode: vi.fn(async () => null),
  startCq: vi.fn(async () => null),
  startQsoRecording: vi.fn(async () => null),
  stopQsoRecording: vi.fn(async () => null),
  setSkipTx1: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => null),
  postSpot: vi.fn(async () => null),
  setFrequency: vi.fn(async () => null),
  setRit: vi.fn(async () => null),
  setXit: vi.fn(async () => null),
  setVfo: vi.fn(async () => null),
  getSpectrumRow: vi.fn(async () => null),
  setDecodeDepth: vi.fn(async () => null),
  atuTune: vi.fn(async () => null),
  setMsk144Period: vi.fn(async () => null),
  getCallStats: vi.fn(async () => null),
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall-stub" /> }))
vi.mock('./OperateDecodes', async (importOriginal) => {
  const real = await importOriginal<typeof import('./OperateDecodes')>()
  return { ...real, OperateDecodes: () => <div data-testid="od-pane" /> }
})

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    stations: [
      {
        call: 'W1ABC',
        grid: 'FN42',
        snr: -7,
        lastHeardSlot: 0,
        heardCount: 3,
        presence: 'live',
        worked: true,
        country: 'United States',
        // W1ABC's last frame was addressed to K9XYZ — the station being called.
        calling: 'K9XYZ',
      },
    ],
    recentDecodes: [],
    conversations: [],
    highlights: [],
    harqRescues: 0,
    clearTick: 0,
    qso: null,
    link: { tier: 'FT8' },
    radio: {
      dialMhz: 14.074,
      band: '20m',
      sideband: 'USB',
      slot: 0,
      source: 'native',
      sourceLabel: 'Native',
      nextSlotMs: 5000,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      txEven: true,
      txCycleAuto: true,
      txEnabled: false,
      txAllowed: true,
      transmitting: false,
      tuning: false,
      atu: true,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

function panelsApi(
  initial: Partial<Record<OperatePanelId, PanelState>> = {},
  extras?: Record<string, OperatePanelId[]>,
): PanelLayoutApi<OperatePanelId> {
  const state: Partial<Record<string, PanelState>> = { ...initial }
  return {
    layout: { v: 1, state, share: {}, ...(extras ? { extras } : {}) },
    stateOf: (id: string) => panelStateIn(OPERATE_PANELS, { v: 1, state, share: {} }, id as OperatePanelId),
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  } as unknown as PanelLayoutApi<OperatePanelId>
}

function renderCockpit(opts: {
  selectedCall?: string | null
  panelState?: Record<string, PanelState>
  layout?: 'classic' | 'roster'
  onSetMode?: (...a: unknown[]) => void
  onClearSelection?: () => void
  onOpenLogbook?: (call: string, id?: string | null) => void
  extras?: Record<string, OperatePanelId[]>
} = {}) {
  const noop = () => {}
  return render(
    <OperateCockpit
      snap={makeSnap()}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={noop}
      onSetTxLevel={noop}
      onSetMode={opts.onSetMode ?? noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={opts.selectedCall === undefined ? 'W1ABC' : opts.selectedCall}
      onSelect={noop}
      onClearSelection={opts.onClearSelection ?? noop}
      onOpenLogbook={opts.onOpenLogbook}
      layoutMode={opts.layout ?? 'classic'}
      onLayoutMode={noop}
      panels={panelsApi(opts.panelState as Partial<Record<OperatePanelId, PanelState>>, opts.extras)}
      active
    />,
  )
}

const cardEl = () => document.querySelector('.recall-card')
const rail = () => document.querySelector('aside.cockpit-side')

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const recentEl = () => document.querySelector('[data-pane="recent"]')
const railKids = () => [...(rail()?.children ?? [])].map((c) => c.getAttribute('data-pane') ?? c.className)
const arrangedRail = () => document.querySelector('aside.op-stack')

describe('the Recent contacts pane in FT', () => {
  afterEach(resetRecentLog)

  it('is listed in ⊞ Panels in both layouts, unticked', () => {
    for (const layout of ['classic', 'roster'] as const) {
      renderCockpit({ layout })
      fireEvent.click(screen.getByRole('button', { name: /panels/i }))
      const box = screen.getByLabelText('Recent contacts') as HTMLInputElement
      expect(box.checked, layout).toBe(false)
      cleanup()
    }
  })

  it('ships hidden: a stored record that never names it draws the stock rail as an explicit untick does', () => {
    for (const layout of ['classic', 'roster'] as const) {
      renderCockpit({ layout, panelState: { stations: 'docked' } })
      expect(recentEl(), `${layout}: on screen without a tick`).toBeNull()
      expect(rail(), `${layout}: control: the stock rail stands`).not.toBeNull()
      expect(arrangedRail(), `${layout}: arranged without a tick`).toBeNull()
      const stored = railKids()
      cleanup()
      renderCockpit({ layout, panelState: { stations: 'docked', recent: 'removed' } })
      expect(railKids(), layout).toEqual(stored)
      cleanup()
    }
  })

  for (const [layout, above] of [['classic', 'cockpit-roster'], ['roster', 'cockpit-rxfreq']] as const) {
    it(`${layout}: ticked, it stands last in the stock rail, content-fit, under the rail's own panes; FT is not arranged`, () => {
      installRecentLog()
      renderCockpit({ layout, panelState: { recent: 'docked' } })
      expect(arrangedRail(), 'ticked, FT switched to its arranged columns').toBeNull()
      expect(document.querySelector('.cockpit-lower')!.hasAttribute('data-arranged')).toBe(false)
      const col = rail()
      expect(col, 'ticked, the stock rail is gone').not.toBeNull()
      const recent = recentEl()
      expect(recent, 'ticked, the Recent contacts pane is not on screen').not.toBeNull()
      expect(recent!.parentElement, 'not a direct member of the stock rail').toBe(col)
      expect(recent!.getAttribute('data-fit')).toBe('content')
      expect(recent!.nextElementSibling, 'something stands under it in the rail').toBeNull()
      expect(recent!.previousElementSibling?.classList.contains(above), `not directly under .${above}`).toBe(true)
      // The callsign card keeps the top of the rail.
      expect(col!.firstElementChild).toBe(cardEl())
    })
  }

  it("classic: ticked, the Rx/Tx column and its Tx messages divider stand exactly as unticked", () => {
    const qsocol = () => {
      const c = document.querySelector('.cockpit-qsocol')
      return c ? [...c.children].map((k) => k.getAttribute('aria-label') ?? k.className) : null
    }
    renderCockpit({ panelState: { stations: 'docked' } })
    const stock = qsocol()
    expect(stock, 'control: the stock Rx/Tx column stands').not.toBeNull()
    expect(stock, 'control: the stock column has the divider').toContain('Tx messages height')
    cleanup()
    renderCockpit({ panelState: { stations: 'docked', recent: 'docked' } })
    expect(qsocol()).toEqual(stock)
  })

  for (const layout of ['classic', 'roster'] as const) {
    it(`${layout}: ticked, the rail floors its feeds and the card keeps its size; unticked, neither`, () => {
      const feeds = () => [...rail()!.children].filter((k) => /cockpit-(roster|decodes-side|rxfreq)\b/.test(k.className))
      const floors = () => feeds().map((f) => (f as HTMLElement).style.minHeight)
      installRecentLog()
      renderCockpit({ layout, panelState: { recent: 'docked' } })
      expect(feeds().length, 'no feeds in the rail').toBe(layout === 'classic' ? 1 : 2)
      expect(rail()!.classList.contains('op-rail-floor'), 'the rail sets no floor').toBe(true)
      expect(floors(), 'a feed takes no floor').toEqual(feeds().map(() => 'var(--cockpit-fill-min, 0)'))
      expect(cardEl()!.classList.contains('cockpit-recall-kept'), 'the card gives up its height to the floors').toBe(true)
      cleanup()
      renderCockpit({ layout, panelState: { recent: 'removed' } })
      expect(rail()!.classList.contains('op-rail-floor')).toBe(false)
      expect(floors()).toEqual(feeds().map(() => ''))
      expect(cardEl()!.classList.contains('cockpit-recall-kept')).toBe(false)
    })
  }

  // The other layout's pane added to this one arranges FT (Classic: the Call Roster; Roster: Stations).
  for (const [layout, added] of [['classic', 'callRoster'], ['roster', 'stations']] as const) {
    it(`${layout}: arranged for another reason, it stands last in the rail column`, () => {
      installRecentLog()
      renderCockpit({ layout, panelState: { recent: 'docked' }, extras: { [layout]: [added] } })
      const col = arrangedRail()
      expect(col, 'control: the seed arranges FT').not.toBeNull()
      const recent = recentEl()
      expect(recent!.parentElement).toBe(col)
      expect(recent!.getAttribute('data-fit')).toBe('content')
      expect(recent!.nextElementSibling).toBeNull()
      expect(recent!.previousElementSibling?.getAttribute('data-pane')).toBe('stations')
    })
  }

  it('a row opens that contact in the Logbook editor, by its id', async () => {
    installRecentLog()
    const onOpenLogbook = vi.fn()
    renderCockpit({ panelState: { recent: 'docked' }, onOpenLogbook })
    await waitFor(() => expect(recentRows()).toHaveLength(2))
    fireEvent.click(recentRows()[0])
    expect(onOpenLogbook).toHaveBeenCalledWith('W1AW', 'id-newer')
  })

  it('stands the rail by itself when nothing else in it is shown', () => {
    installRecentLog()
    renderCockpit({ selectedCall: null, panelState: { recent: 'docked', stations: 'removed' } })
    expect(rail(), 'the rail does not stand for the pane').not.toBeNull()
    expect(rail()!.contains(recentEl())).toBe(true)
    cleanup()
    // Control: unticked, the same screen has no rail at all.
    renderCockpit({ selectedCall: null, panelState: { stations: 'removed' } })
    expect(rail()).toBeNull()
    expect(arrangedRail()).toBeNull()
  })
})

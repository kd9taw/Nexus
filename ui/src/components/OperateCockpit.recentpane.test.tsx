// @vitest-environment jsdom
//
// THE RECENT CONTACTS PANE IN FT. Phone, CW, JS8, RTTY and PSK list the last few contacts in a pane under
// their log entry form; FT logs through its confirm dialog and has no form, so its pane stands at the foot of
// the side rail, at its content height. Like a box it stands only in FT's arranged columns, whose feeds keep
// a floor and whose column scrolls (the stock rail's feeds have none). These pin:
//  1. it ships hidden, and a record that never mentions it draws the stock rail exactly as before;
//  2. ticked, it stands last in the arranged rail column in both layouts, through CockpitPaneFrame, content-fit;
//  3. a row opens that contact in the Logbook's editor, as in every other cockpit;
//  4. it stands the rail by itself when nothing else in the rail is shown.
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

function panelsApi(initial: Partial<Record<OperatePanelId, PanelState>> = {}): PanelLayoutApi<OperatePanelId> {
  const state: Partial<Record<string, PanelState>> = { ...initial }
  return {
    layout: { v: 1, state, share: {} },
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
      panels={panelsApi(opts.panelState as Partial<Record<OperatePanelId, PanelState>>)}
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

  for (const [layout, above] of [['classic', 'stations'], ['roster', 'rxfreq']] as const) {
    it(`${layout}: ticked, it stands last in the arranged rail column, content-fit, under the rail's own panes`, () => {
      installRecentLog()
      renderCockpit({ layout, panelState: { recent: 'docked' } })
      const col = arrangedRail()
      expect(col, 'ticked, FT is not drawn in its arranged columns').not.toBeNull()
      expect(rail(), 'the stock rail stands beside the arranged one').toBeNull()
      const recent = recentEl()
      expect(recent, 'ticked, the Recent contacts pane is not on screen').not.toBeNull()
      expect(recent!.parentElement, 'not a direct member of the rail column').toBe(col)
      expect(recent!.getAttribute('data-fit')).toBe('content')
      expect(recent!.nextElementSibling, 'something stands under it in the rail column').toBeNull()
      expect(recent!.previousElementSibling?.getAttribute('data-pane'), `not directly under ${above}`).toBe(above)
      // The callsign card keeps the top of the rail.
      expect(col!.firstElementChild).toBe(cardEl())
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
    expect(arrangedRail(), 'the rail does not stand for the pane').not.toBeNull()
    expect(arrangedRail()!.contains(recentEl())).toBe(true)
    cleanup()
    // Control: unticked, the same screen has no rail at all.
    renderCockpit({ selectedCall: null, panelState: { stations: 'removed' } })
    expect(rail()).toBeNull()
    expect(arrangedRail()).toBeNull()
  })
})

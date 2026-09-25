// @vitest-environment jsdom
//
// #204 (KR4FQG) — THE CALLSIGN CARD AS ITS OWN PANEL, CLEARED ON S&P, AND THE CALLED STATION.
//
// What the reporter asked for, and what each test below pins:
//  1. "Can the Callsign block be a separate Panel?" / "added to the Panels selection" — the card
//     is a ⊞ entry of its own. Removed, it is gone even with a station selected; and it no longer
//     rides on the Stations roster, so hiding Stations does not take the card with it.
//  2. "Can it automatically clear when S&P is selected in FT?" — S&P dismisses the card (and the
//     click selection behind it), exactly as F4 does, and still switches the sequencer role.
//  3. "Click the station being called, and display its Callsign block" — the card of a station
//     that is calling someone offers that station's card, one click away.
//
// And one layout guarantee the new panel must not break: with no card up and Stations hidden,
// the side rail does not mount empty (the idle layout costs nothing, as the card always did).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, QrzLookup } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

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

vi.mock('../api', () => ({
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
    stateOf: (id: string) => state[id] ?? 'docked',
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

describe('#204 — the callsign card is a panel of its own', () => {
  it('removed from ⊞ Panels, the card is gone even with a station selected', async () => {
    renderCockpit({ panelState: { recall: 'removed' } })
    // Positive control: the roster in the same rail rendered, so an absent card is the panel
    // state and not a broken render.
    expect(await screen.findByTestId('stations-roster')).toBeTruthy()
    expect(cardEl()).toBeNull()
  })

  it('docked, the card shows for the selected station', async () => {
    renderCockpit()
    await waitFor(() => expect(cardEl()).not.toBeNull())
  })

  it('hiding Stations no longer takes the card with it', async () => {
    renderCockpit({ panelState: { stations: 'removed' } })
    await waitFor(() => expect(cardEl()).not.toBeNull())
    expect(screen.queryByTestId('stations-roster')).toBeNull()
    expect(rail()?.contains(cardEl())).toBe(true)
  })

  it('with no card up and Stations hidden, the side rail does not mount empty', () => {
    renderCockpit({ selectedCall: null, panelState: { stations: 'removed' } })
    expect(rail()).toBeNull()
  })

  it('is listed in ⊞ Panels in both layouts', () => {
    for (const layout of ['classic', 'roster'] as const) {
      renderCockpit({ layout })
      fireEvent.click(screen.getByRole('button', { name: /panels/i }))
      expect(screen.getByLabelText('Callsign card'), layout).toBeTruthy()
      cleanup()
    }
  })
})

describe('#204 — S&P clears the card', () => {
  it('dismisses the card and the selection, and still switches to S&P', async () => {
    const onSetMode = vi.fn()
    const onClearSelection = vi.fn()
    renderCockpit({ onSetMode, onClearSelection })
    await waitFor(() => expect(cardEl()).not.toBeNull())

    fireEvent.click(screen.getByRole('button', { name: 'S&P' }))

    await waitFor(() => expect(cardEl()).toBeNull())
    expect(onClearSelection).toHaveBeenCalledTimes(1)
    expect(onSetMode).toHaveBeenCalledWith('qso-monitor')
  })
})

describe('#204 — the station being called', () => {
  it("the card of a station calling someone opens that station's card", async () => {
    renderCockpit()
    await waitFor(() => expect(cardEl()).not.toBeNull())
    const card = cardEl() as HTMLElement

    fireEvent.click(within(card).getByRole('button', { name: 'Calling K9XYZ' }))

    await waitFor(() =>
      expect(within(cardEl() as HTMLElement).getByText('K9XYZ')).toBeTruthy(),
    )
  })
})

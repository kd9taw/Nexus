// @vitest-environment jsdom
//
// THE BAND ACTIVITY SEARCH BOX INSIDE THE FT COCKPIT. The component's own suite
// (OperateDecodes.search.test.tsx) pins what the box matches; this one pins how it lives beside
// the cockpit's keys and panes, with the real Band Activity and Rx Frequency panes mounted:
//
//   · typing in the box never fires an FT key: F6 (re-decode) and Alt+1…6 (the Tx buttons) stay
//     behind the typing guard, and plain letters and digits do nothing at all;
//   · Esc in the box empties it AND is still the stop: Esc halts TX on every operating screen,
//     heard in the capture phase before any control, and a search box is not allowed to be the
//     one place it does not. Outside the box Esc halts as it always did;
//   · the search survives Classic ↔ Roster (the cockpit owns it, as it owns the sort);
//   · the Rx Frequency pane never applies it, and a double-click on a filtered row works the
//     station exactly as on the unfiltered row.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, DecodeRow } from '../types'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'

vi.setConfig({ testTimeout: 15_000 })

const redecode = vi.fn(async () => null)

vi.mock('./Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall-canvas" />,
}))

vi.mock('../api', () => {
  const nothing = () => Promise.resolve(null)
  return {
    getSettings: vi.fn(() => Promise.resolve({})),
    setSettings: vi.fn(nothing),
    openPanelWindow: vi.fn(nothing),
    closePanelWindow: vi.fn(nothing),
    notifyErase: vi.fn(nothing),
    pointRotatorAtCall: vi.fn(nothing),
    redecode: (...a: unknown[]) => redecode(...(a as [])),
    startCq: vi.fn(nothing),
    startQsoRecording: vi.fn(nothing),
    stopQsoRecording: vi.fn(nothing),
    setSkipTx1: vi.fn(nothing),
    getDeclination: vi.fn(nothing),
    getSatTrackStatus: vi.fn(nothing),
    getSatTransponder: vi.fn(nothing),
    setSatTransponder: vi.fn(nothing),
    readRotator: vi.fn(nothing),
    stopRotator: vi.fn(nothing),
    stopSatTrack: vi.fn(nothing),
    openQrzPage: vi.fn(nothing),
    postSpot: vi.fn(nothing),
    setFrequency: vi.fn(nothing),
    setRit: vi.fn(nothing),
    setXit: vi.fn(nothing),
    setVfo: vi.fn(nothing),
    getSpectrumRow: vi.fn(nothing),
  }
})

const decode = (over: Partial<DecodeRow>): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

// W1AW sits on the RX offset (1500 Hz), so the Rx Frequency pane shows it too.
const DECODES: DecodeRow[] = [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31', freqHz: 1500, country: 'United States' }),
  decode({ from: 'JA1XYZ', message: 'CQ DX JA1XYZ PM95', freqHz: 900, snr: -18, country: 'Japan' }),
  decode({ from: 'K1ABC', message: 'K1ABC DL1QQ R-07', freqHz: 2100, isCq: false, country: 'Germany' }),
]

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  stations: [],
  recentDecodes: DECODES,
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
    slot: 10,
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
    qsoRecording: false,
    catOk: true,
    splitTxMhz: null,
  },
} as unknown as AppSnapshot

function panelsApi(
  state: Partial<Record<OperatePanelId, PanelState>>,
  places?: PanelLayoutApi<OperatePanelId>['layout']['places'],
): PanelLayoutApi<OperatePanelId> {
  const layout = { v: 1 as const, state, share: {}, ...(places ? { places } : {}) }
  return {
    layout,
    stateOf: (id) => panelStateIn(OPERATE_PANELS, layout, id),
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    setCols: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  }
}

/** ⊞ Arrange's record for Classic: Band Activity placed, so the region draws the arranged columns. */
const ARRANGED = { classic: { bandActivity: { col: 'a' as const, order: 0 } } }

function renderCockpit(layoutMode: 'classic' | 'roster' = 'classic') {
  const noop = () => {}
  const spies = { onHaltTx: vi.fn(), onOverrideTx: vi.fn(), onCall: vi.fn() }
  let panels = panelsApi({})
  const el = (mode: 'classic' | 'roster') => (
    <OperateCockpit
      snap={snap}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={spies.onCall}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={spies.onOverrideTx}
      onHaltTx={spies.onHaltTx}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      layoutMode={mode}
      onLayoutMode={noop}
      panels={panels}
      active
    />
  )
  const view = render(el(layoutMode))
  /** Arrange (or un-arrange) Classic on the SAME mounted cockpit: the region swaps branches, which
   *  remounts every pane in it. */
  const arrange = (on: boolean) => {
    panels = panelsApi({}, on ? ARRANGED : undefined)
    view.rerender(el('classic'))
  }
  return { ...view, spies, arrange, switchTo: (mode: 'classic' | 'roster') => view.rerender(el(mode)) }
}

const box = () => screen.getByRole('searchbox', { name: 'Search Band Activity' })
const type = (text: string) => fireEvent.change(box(), { target: { value: text } })
/** The Band Activity pane: the section that holds the search box. */
const bandPane = () => box().closest('section') as HTMLElement
const rxPane = () =>
  Array.from(document.querySelectorAll('section.operate-decodes')).find((s) => s !== bandPane()) as HTMLElement
const callsIn = (pane: HTMLElement) =>
  Array.from(pane.querySelectorAll('.decode-row')).map((r) => (r.getAttribute('aria-label') ?? '').split(',')[0])

beforeEach(() => {
  localStorage.clear()
  redecode.mockClear()
})
afterEach(cleanup)

describe('typing in the Band Activity search never fires an FT key', () => {
  it('F6 typed in the box does not re-decode; on the page it does (control)', () => {
    renderCockpit()
    fireEvent.keyDown(box(), { key: 'F6' })
    expect(redecode).not.toHaveBeenCalled()
    fireEvent.keyDown(document.body, { key: 'F6' })
    expect(redecode).toHaveBeenCalledTimes(1)
  })

  it('Alt+1 typed in the box sends no Tx message; on the page it does (control)', () => {
    const { spies } = renderCockpit()
    // A DX call in the Tx panel, so Alt+1 has something to send: select a decode.
    fireEvent.click(within(bandPane()).getByText('CQ DX JA1XYZ PM95'))
    fireEvent.keyDown(box(), { key: '1', code: 'Digit1', altKey: true })
    expect(spies.onOverrideTx).not.toHaveBeenCalled()
    fireEvent.keyDown(document.body, { key: '1', code: 'Digit1', altKey: true })
    expect(spies.onOverrideTx).toHaveBeenCalledTimes(1)
  })

  it('plain letters, digits and Enter typed in the box do nothing but type', () => {
    const { spies } = renderCockpit()
    for (const key of ['k', '1', 'A', '6', 'Enter', ' ']) fireEvent.keyDown(box(), { key })
    expect(redecode).not.toHaveBeenCalled()
    expect(spies.onOverrideTx).not.toHaveBeenCalled()
    expect(spies.onHaltTx).not.toHaveBeenCalled()
    expect(spies.onCall).not.toHaveBeenCalled()
  })
})

describe('Esc and the search box', () => {
  it('Esc in the box empties it and is still the stop', () => {
    const { spies } = renderCockpit()
    type('japan')
    fireEvent.keyDown(box(), { key: 'Escape' })
    expect(box()).toHaveProperty('value', '')
    expect(spies.onHaltTx).toHaveBeenCalledTimes(1)
  })

  it('Esc outside the box halts TX and leaves the search alone', () => {
    const { spies } = renderCockpit()
    type('japan')
    fireEvent.keyDown(document.body, { key: 'Escape' })
    expect(spies.onHaltTx).toHaveBeenCalledTimes(1)
    expect(box()).toHaveProperty('value', 'japan')
  })
})

describe('the search in the cockpit', () => {
  it('filters Band Activity and never the Rx Frequency pane', () => {
    renderCockpit()
    expect(callsIn(rxPane())).toEqual(['W1AW'])
    type('japan')
    expect(callsIn(bandPane())).toEqual(['JA1XYZ'])
    expect(callsIn(rxPane())).toEqual(['W1AW'])
  })

  it('survives a switch to Roster and back', () => {
    const { switchTo } = renderCockpit('classic')
    type('japan')
    switchTo('roster')
    expect(box()).toHaveProperty('value', 'japan')
    expect(callsIn(bandPane())).toEqual(['JA1XYZ'])
    switchTo('classic')
    expect(box()).toHaveProperty('value', 'japan')
  })

  it('survives ⊞ Arrange moving Band Activity into the arranged columns and back', () => {
    const { arrange, container } = renderCockpit('classic')
    type('japan')
    arrange(true)
    expect(container.querySelector('[data-arranged]'), 'the arranged columns did not draw').not.toBeNull()
    expect(box()).toHaveProperty('value', 'japan')
    expect(callsIn(bandPane())).toEqual(['JA1XYZ'])
    arrange(false)
    expect(container.querySelector('[data-arranged]')).toBeNull()
    expect(box()).toHaveProperty('value', 'japan')
  })

  it('is in Band Activity in Roster too', () => {
    renderCockpit('roster')
    type('dl1qq')
    expect(callsIn(bandPane())).toEqual(['K1ABC'])
  })

  it('double-clicking a filtered row works the station exactly as unfiltered', () => {
    const a = renderCockpit()
    fireEvent.doubleClick(within(bandPane()).getByText('K1ABC DL1QQ R-07'))
    const plain = a.spies.onCall.mock.calls[0]
    cleanup()
    const b = renderCockpit()
    type('dl1qq')
    fireEvent.doubleClick(within(bandPane()).getByText('K1ABC DL1QQ R-07'))
    expect(b.spies.onCall).toHaveBeenCalledTimes(1)
    expect(b.spies.onCall.mock.calls[0]).toEqual(plain)
  })
})

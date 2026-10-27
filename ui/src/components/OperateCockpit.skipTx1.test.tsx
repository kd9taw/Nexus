// @vitest-environment jsdom
//
// Skip Tx1 is the station's own setting. The box draws `radio.skipTx1` from the snapshot, so on a
// computer driving a station it shows the station's value (the snapshot there is the station's),
// a click sends the change on and draws the station's answer, and a refusal is said, not dropped.
// At the shack it behaves as before: off at launch, and a click turns it on.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { OperateCockpit } from './OperateCockpit'
import { setSkipTx1 } from '../api'
import { subscribeToasts, type Toast } from '../toast'
import type { AppSnapshot } from '../types'
import { OPERATE_PANELS, panelStateIn } from '../features/panelState'
import type { OperatePanelId, PanelLayoutApi, PanelState } from '../features/panelState'

vi.mock('./Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall-canvas" />,
}))

vi.mock('../api', () => {
  const nothing = () => Promise.resolve(null)
  return {
    getSettings: vi.fn(() => Promise.resolve({})),
    setSettings: vi.fn(nothing),
    openPanelWindow: vi.fn(nothing),
    notifyErase: vi.fn(nothing),
    pointRotatorAtCall: vi.fn(nothing),
    redecode: vi.fn(nothing),
    startCq: vi.fn(nothing),
    startQsoRecording: vi.fn(nothing),
    stopQsoRecording: vi.fn(nothing),
    setSkipTx1: vi.fn(nothing),
    getDeclination: vi.fn(nothing),
    getSatTrackStatus: vi.fn(nothing),
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
    setDecodeDepth: vi.fn(nothing),
  }
})

function makeSnap(skipTx1?: boolean): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN61',
    stations: [],
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
      atu: null,
      qsoRecording: false,
      catOk: true,
      splitTxMhz: null,
      ...(skipTx1 === undefined ? {} : { skipTx1 }),
    },
  } as unknown as AppSnapshot
}

function panelsApi(state: Partial<Record<OperatePanelId, PanelState>>): PanelLayoutApi<OperatePanelId> {
  return {
    layout: { v: 1, state, share: {} },
    stateOf: (id) => panelStateIn(OPERATE_PANELS, { v: 1, state, share: {} }, id),
    setPanelState: vi.fn(),
    shareOf: () => 1,
    setShare: vi.fn(),
    setShares: vi.fn(),
    undo: vi.fn(),
    canUndo: false,
    undoRemoves: [],
    reset: vi.fn(),
  }
}

function cockpit(snap: AppSnapshot, onSnap = vi.fn()) {
  const noop = () => {}
  return (
    <OperateCockpit
      snap={snap}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={noop}
      onCall={noop}
      onSetTxLevel={noop}
      onSetMode={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onOverrideTx={noop}
      onHaltTx={noop}
      roster={<div data-testid="stations-roster" />}
      needByCall={new Map()}
      selectedCall={null}
      onSelect={noop}
      layoutMode="classic"
      onLayoutMode={noop}
      panels={panelsApi({})}
      active={false}
      onSnap={onSnap}
    />
  )
}

const box = () => screen.getByRole('button', { name: /skip tx1/i })
const pressed = () => box().getAttribute('aria-pressed')

afterEach(() => {
  cleanup()
  vi.mocked(setSkipTx1).mockReset()
})

describe("Skip Tx1 follows the station's own setting", () => {
  it("shows the station's value, and follows it when the station changes", () => {
    const { rerender } = render(cockpit(makeSnap(true)))
    expect(pressed()).toBe('true')
    rerender(cockpit(makeSnap(false)))
    expect(pressed()).toBe('false')
  })

  it("sends a change on, and draws the station's answer", async () => {
    const answer = makeSnap(true)
    vi.mocked(setSkipTx1).mockResolvedValue(answer)
    const onSnap = vi.fn()
    render(cockpit(makeSnap(false), onSnap))
    fireEvent.click(box())
    expect(setSkipTx1).toHaveBeenCalledWith(true)
    await waitFor(() => expect(onSnap).toHaveBeenCalledWith(answer))
  })

  it('says a refusal, and keeps showing the station\'s value', async () => {
    vi.mocked(setSkipTx1).mockRejectedValue('refused: not driving')
    const seen: Toast[][] = []
    const stop = subscribeToasts((ts) => seen.push(ts))
    const onSnap = vi.fn()
    render(cockpit(makeSnap(false), onSnap))
    fireEvent.click(box())
    await waitFor(() =>
      expect(seen[seen.length - 1]?.some((x) => x.kind === 'error' && x.message.includes('refused: not driving'))).toBe(true),
    )
    stop()
    expect(onSnap).not.toHaveBeenCalled()
    expect(pressed()).toBe('false')
  })

  it('at the shack: off at launch, and a click turns it on', async () => {
    vi.mocked(setSkipTx1).mockResolvedValue(makeSnap(true))
    const onSnap = vi.fn()
    // A snapshot without the field (an older station) reads off, the launch value.
    const { rerender } = render(cockpit(makeSnap(), onSnap))
    expect(pressed()).toBe('false')
    fireEvent.click(box())
    expect(setSkipTx1).toHaveBeenCalledWith(true)
    await waitFor(() => expect(onSnap).toHaveBeenCalledTimes(1))
    rerender(cockpit(onSnap.mock.calls[0][0] as AppSnapshot, onSnap))
    expect(pressed()).toBe('true')
  })
})

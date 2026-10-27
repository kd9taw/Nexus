// @vitest-environment jsdom
//
// THE FT WATERFALL'S CALLSIGN TAGS, IN THE COCKPIT (#312). Each decode of the last period that names
// a sender is tagged over its own audio frequency, never our own transmission. A station calling us
// leads a crowd, then the strongest. A click on a tag moves the RX offset there, as a click on the
// picture does, and the Calls button in the waterfall's header turns the tags off and is kept.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import type { ReactNode } from 'react'
import { OperateCockpit } from './OperateCockpit'
import type { AppSnapshot, DecodeRow } from '../types'
import { OPERATE_PANELS, usePanelLayout } from '../features/panelState'
import type { WaterfallTag } from './Waterfall'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module, as the other FT suites do: a hand-kept list omits whatever is
  // added later and the cockpit throws on mount.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => null) : actual[k]
  }
  return {
    ...auto,
    getSettings: vi.fn(async () => ({})),
    getLicensedBandPlan: vi.fn(async () => []),
    getDeclination: vi.fn(async () => 0),
    resolveEntity: vi.fn(async () => 'United States'),
  }
})
// The waterfall is a canvas: the stub keeps what the cockpit hands it, and draws its header control.
const seen = vi.hoisted(() => ({ props: null as null | { tags?: WaterfallTag[]; onTag?: (hz: number) => void; headerExtra?: ReactNode } }))
vi.mock('./Waterfall', () => ({
  Waterfall: (p: { tags?: WaterfallTag[]; onTag?: (hz: number) => void; headerExtra?: ReactNode }) => {
    seen.props = p
    return <div data-testid="waterfall-stub">{p.headerExtra}</div>
  },
}))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

const decode = (over: Partial<DecodeRow>): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: 1500,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

// The last period: two CQs, a station calling us (weaker, but it leads), our own transmission and
// a decode with no sender, which are never tagged.
const DECODES: DecodeRow[] = [
  decode({}),
  decode({ from: 'K1ABC', message: 'CQ K1ABC FN42', freqHz: 800, snr: -3 }),
  decode({ from: 'N0CALL', message: 'KD9TAW N0CALL EM12', freqHz: 2100, snr: -20, directedToMe: true }),
  decode({ from: 'KD9TAW', message: 'CQ KD9TAW EN61', freqHz: 1500, mine: true }),
  decode({ from: undefined, message: 'TNX 73 GL', freqHz: 1700 }),
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
  rxFreqClearTick: 0,
  qso: null,
  link: { tier: 'FT8', periodSecs: 15 },
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
    decodeDepth: 2,
  },
} as unknown as AppSnapshot

beforeEach(() => {
  localStorage.clear()
  seen.props = null
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const noop = () => {}

function Live({ onTune = noop }: { onTune?: (hz: number, target: 'rx' | 'tx' | 'both') => void }) {
  const panels = usePanelLayout(OPERATE_PANELS)
  return (
    <OperateCockpit
      snap={snap}
      theme="dark"
      tier="FT8"
      onTierChange={noop}
      bandPlan={[]}
      onSetFrequency={noop}
      onSourceChange={noop}
      onTune={onTune}
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
      panels={panels}
      active
    />
  )
}

const tags = () => (seen.props?.tags ?? []).map((t) => [t.label, t.hz, t.rank])

describe('callsign tags on the FT waterfall', () => {
  it('tags the last period by sender, a station calling us first, then the strongest', () => {
    render(<Live />)
    expect(tags()).toEqual([
      ['N0CALL', 2100, 0],
      ['K1ABC', 800, 1],
      ['W1AW', 1500, 2],
    ])
  })

  it('a click on a tag moves the RX offset there', () => {
    const onTune = vi.fn()
    render(<Live onTune={onTune} />)
    seen.props!.onTag!(800)
    expect(onTune).toHaveBeenCalledWith(800, 'rx')
  })

  it('the Calls button turns the tags off, and the choice is kept', () => {
    const view = render(<Live />)
    const calls = screen.getByRole('button', { name: 'Calls' })
    expect(calls.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(calls)
    expect(tags()).toEqual([])
    view.unmount()
    render(<Live />)
    expect(screen.getByRole('button', { name: 'Calls' }).getAttribute('aria-pressed')).toBe('false')
    expect(tags()).toEqual([])
  })
})

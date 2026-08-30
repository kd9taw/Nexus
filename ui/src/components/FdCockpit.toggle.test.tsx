// @vitest-environment jsdom
//
// THE COCKPIT ↔ DASHBOARD TOGGLE — the control that makes one nav slot hold two screens.
//
// Field Day did not get a second View, a second registry entry or a second ModeNav row. It got
// ONE slot with two faces, and the only thing standing between the operator and a dead end is
// that each face draws the button back to the other. That button is App's wiring (it owns the
// persisted `nexus.fdLayout` choice and hands each screen a callback), so it is pinned here
// rather than inside either component's own suite.
//
// BOTH DIRECTIONS, for both faces, because the prop is OPTIONAL on purpose: passing it must
// draw a working button, and NOT passing it must draw nothing at all. The second half is not
// tidiness — FieldDayView has five shipped suites that render it without the prop, and a
// button that appeared unconditionally would change what every one of them sees.
//
// Also pinned: the DIG note's Open-Operate button, App's third callback into this cockpit. It
// is a NAVIGATION affordance and nothing else — the cockpit's whole relationship with the FT
// path is read-only (the FT hard gate), so the click is asserted to move the operator and to
// key nothing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { FdCockpit } from './FdCockpit'
import { FieldDayView } from './FieldDayView'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { AppSnapshot, FieldDayStatus } from '../types'

// ⭐ DERIVED FROM THE REAL MODULE (the stop-line sweep's lesson): a hand-kept mock omits any
// export added after it was written, and the component then THROWS ON MOUNT — a red at a seam
// nothing in the diff explains. Only the calls these assertions depend on are overridden.
const { setPtt, sendCw, haltTx } = vi.hoisted(() => ({
  setPtt: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
}))
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    setPtt,
    sendCw,
    haltTx,
    getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
    exportLog: vi.fn(async () => ''),
    getLicensedBandPlan: vi.fn(async () => []),
    cwDecode: vi.fn(async () => ({ text: '', sent: [] })),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The dock's entry row and the two heavy mode panes: none of them draws a toggle.
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./OperateDecodes', () => ({ OperateDecodes: () => <div data-testid="decode-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="keyer-stub" /> }))

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const fieldDay = {
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: 'running',
  qsoCount: 3,
  sections: 2,
  points: 6,
  workedSections: ['WI'],
  log: [],
} as unknown as FieldDayStatus

function snapWith(rigMode: string): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    harqRescues: 0,
    recentDecodes: [],
    radio: {
      dialMhz: 14.25,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode,
      operatingMode: 'phone',
      transmitting: false,
      tuning: false,
      txEnabled: true,
      txAllowed: true,
      slot: 0,
      rxOffsetHz: 1500,
      cwWpm: 22,
      rfPower: null,
      smeterDb: null,
      splitTxMhz: null,
    },
  } as unknown as AppSnapshot
}

async function settle() {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const button = (name: RegExp) => screen.queryAllByRole('button', { name })

describe('the dashboard offers the cockpit', () => {
  it('draws a Cockpit button that calls back, when App passes the callback', async () => {
    const open = vi.fn()
    render(<FieldDayView fieldDay={fieldDay} onSetMode={() => {}} onOpenCockpit={open} />)
    await settle()
    const btns = button(/^cockpit$/i)
    expect(btns).toHaveLength(1)
    fireEvent.click(btns[0])
    expect(open).toHaveBeenCalledTimes(1)
  })

  it('draws nothing at all without it — every shipped dashboard suite sees what it always saw', async () => {
    render(<FieldDayView fieldDay={fieldDay} onSetMode={() => {}} />)
    await settle()
    expect(button(/^cockpit$/i)).toHaveLength(0)
  })
})

describe('the cockpit offers the dashboard', () => {
  it('draws a Dashboard button that calls back, when App passes the callback', async () => {
    const open = vi.fn()
    render(
      <FdCockpit
        snap={snapWith('USB')}
        fieldDay={fieldDay}
        onSetMode={() => {}}
        onOpenDashboard={open}
      />,
    )
    await settle()
    const btns = button(/^dashboard$/i)
    expect(btns).toHaveLength(1)
    fireEvent.click(btns[0])
    expect(open).toHaveBeenCalledTimes(1)
  })

  it('draws nothing at all without it', async () => {
    render(<FdCockpit snap={snapWith('USB')} fieldDay={fieldDay} onSetMode={() => {}} />)
    await settle()
    expect(button(/^dashboard$/i)).toHaveLength(0)
  })
})

describe('the digital position is pointed at Operate, and never keys from here', () => {
  it('opens Operate on click and transmits nothing', async () => {
    const open = vi.fn()
    render(
      <FdCockpit
        snap={snapWith('PKTUSB')}
        fieldDay={fieldDay}
        onSetMode={() => {}}
        onOpenOperate={open}
      />,
    )
    await settle()
    const btns = button(/^open operate$/i)
    expect(btns).toHaveLength(1)
    fireEvent.click(btns[0])
    expect(open).toHaveBeenCalledTimes(1)
    // The FT hard gate, stated as an assertion: this button navigates. It does not key, it
    // does not send, and it does not touch the sequencer.
    expect(setPtt).not.toHaveBeenCalled()
    expect(sendCw).not.toHaveBeenCalled()
  })

  it('draws nothing at all without the callback', async () => {
    render(<FdCockpit snap={snapWith('PKTUSB')} fieldDay={fieldDay} onSetMode={() => {}} />)
    await settle()
    expect(button(/^open operate$/i)).toHaveLength(0)
  })
})

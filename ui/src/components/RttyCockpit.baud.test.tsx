// @vitest-environment jsdom
//
// #488, wired: the RTTY screen's receive pane carries the baud picker in its head (beside Arm RX;
// not in the cockpit header, whose width moves the TX dock at 1366×768), it saves
// `rttyBaud` through the settings patch seam (only that field), and it is disabled while an over
// or the TX latch is up. The picker itself is RttyBaudPicker.test.tsx.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, RttyState } from '../types'

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

const state = vi.hoisted(() => ({ current: null as unknown }))
vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => state.current),
  rttyArm: vi.fn(async () => state.current),
  rttyAutoArm: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  haltTx: vi.fn(),
}))
const patch = vi.hoisted(() => ({ fields: [] as unknown[] }))
vi.mock('../settings/patch', () => ({
  patchSettings: vi.fn(async (edit: (cur: unknown) => unknown) => {
    patch.fields.push(edit({ rttyBaud: 45.45, rttyShiftHz: 170 }))
    return null
  }),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

const snap = {
  mycall: 'KD9TAW',
  radio: { dialMhz: 14.08, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true },
} as unknown as AppSnapshot
const IDLE = {
  armed: true, afcHz: 0, afcLocked: false, text: '', charConf: [], baud: 45.45, shiftHz: 170, backend: 'afsk',
  sending: false, latched: false, keyerError: null, markHz: 2125, spaceHz: 2295, auto: false, seqState: 'idle',
  peer: null, peerExchange: [], heardCq: null,
} as unknown as RttyState

beforeEach(() => {
  state.current = IDLE
  patch.fields = []
})
afterEach(cleanup)

const picker = () => screen.getByRole('combobox', { name: 'RTTY baud rate' }) as HTMLSelectElement

describe('the RTTY header’s baud picker (#488)', () => {
  it('sits in the receive pane’s head, not the header, and saves only the baud', async () => {
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('RTTY 45.45 · 170 Hz')
    await waitFor(() => expect(picker().value).toBe('45.45'))
    expect(picker().closest('.cw-decode-head')).not.toBeNull()
    expect(picker().closest('.cockpit-header')).toBeNull()
    fireEvent.change(picker(), { target: { value: '50' } })
    await waitFor(() => expect(patch.fields).toEqual([{ rttyBaud: 50 }]))
  })

  it.each([
    ['sending', { sending: true }],
    ['latched', { sending: true, latched: true }],
  ])('disabled while %s', async (_what, over) => {
    state.current = { ...IDLE, ...over }
    render(<RttyCockpit snap={snap} />)
    await waitFor(() => expect(picker().disabled).toBe(true))
  })

  it('CONTROL: enabled on receive', async () => {
    render(<RttyCockpit snap={snap} />)
    await waitFor(() => expect(picker().disabled).toBe(false))
  })
})

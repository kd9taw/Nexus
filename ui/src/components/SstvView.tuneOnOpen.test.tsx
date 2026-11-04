// @vitest-environment jsdom
//
// #451 — AN OPTION TO TUNE TO THE BAND'S SSTV FREQUENCY WHEN THE SSTV SCREEN OPENS.
//
// Opening SSTV leaves the dial where the last screen had it (FT8 at 14.074, CW at 14.030), and
// the screen says where images appear and offers the channel in its band picker. With Settings ▸
// Digital ▸ SSTV ▸ "Tune to the band's SSTV frequency when SSTV opens" on (it is OFF by default),
// each entry to the screen also tunes there, through the same call a pick in the band picker
// makes: the band's own SSTV channel, or 20 m's when the band has none and the radio is on HF.
//
// What it must NOT do: move the dial with the option off; move a dial already on an SSTV
// channel (an overflow, the ISS downlink the pass auto-arm parked on); move it while the radio is
// transmitting or tuning; move it while the screen is hidden; or move it again on every render
// or poll of one visit. A VHF/UHF dial on a band with no SSTV channel is left alone, because a
// jump to HF from there is not "this band's" frequency and the radio may not cover it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { SstvView, sstvChannelOnOpen } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, BandChannel, SstvState } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))
vi.mock('../announce', () => ({ announce: vi.fn() }))

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>
const sstvAutoArm = api.sstvAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>

const IDLE = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: null,
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
} as unknown as SstvState

const ch = (band: string, dialMhz: number, mode: string): BandChannel =>
  ({ band, group: dialMhz >= 30 ? 'VHF' : 'HF', dialMhz, mode, label: band, note: '' }) as BandChannel
/** The built-in plan's shape: a band's primary channel is the row named for the band alone. */
const PLAN: BandChannel[] = [
  ch('80m', 3.845, 'LSB'),
  ch('80m-eu', 3.73, 'LSB'),
  ch('40m', 7.171, 'LSB'),
  ch('40m-eu', 7.165, 'LSB'),
  ch('20m', 14.23, 'USB'),
  ch('20m-alt', 14.233, 'USB'),
  ch('20m-alt2', 14.236, 'USB'),
  ch('15m', 21.34, 'USB'),
  ch('2m', 145.8, 'FM'),
  ch('2m-call', 144.5, 'FM'),
]

const snapAt = (dialMhz: number, radio: Record<string, unknown> = {}) =>
  ({
    radio: {
      dialMhz,
      band: '',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      tuning: false,
      txAllowed: true,
      ...radio,
    },
  }) as unknown as AppSnapshot

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvAutoArm.mockReset().mockResolvedValue(IDLE)
  getLicensedBandPlan.mockReset().mockResolvedValue(PLAN)
})
afterEach(cleanup)

/** Let the band-plan read and the entry effects land. */
const settle = () =>
  act(async () => {
    for (let i = 0; i < 5; i++) await Promise.resolve()
  })

describe('which channel the screen tunes to on opening', () => {
  it("the band's own SSTV channel: 20 m's 14.230 from FT8's 14.074, 40 m's from CW at 7.030", () => {
    expect(sstvChannelOnOpen(PLAN, 14.074)?.dialMhz).toBe(14.23)
    expect(sstvChannelOnOpen(PLAN, 7.03)?.dialMhz).toBe(7.171)
    expect(sstvChannelOnOpen(PLAN, 21.074)?.dialMhz).toBe(21.34)
  })

  it('20 m when the HF band has no SSTV channel (30 m), or the dial is off the bands', () => {
    expect(sstvChannelOnOpen(PLAN, 10.136)?.dialMhz).toBe(14.23)
    expect(sstvChannelOnOpen(PLAN, 9.5)?.dialMhz).toBe(14.23)
  })

  it('nothing when the dial is already on an SSTV channel — an overflow, or the ISS downlink', () => {
    expect(sstvChannelOnOpen(PLAN, 14.236)).toBeNull()
    expect(sstvChannelOnOpen(PLAN, 14.23)).toBeNull()
    expect(sstvChannelOnOpen(PLAN, 145.8)).toBeNull()
  })

  it('nothing on a VHF/UHF band with no SSTV channel, and nothing with no plan', () => {
    expect(sstvChannelOnOpen(PLAN, 432.1)).toBeNull()
    expect(sstvChannelOnOpen([], 14.074)).toBeNull()
  })
})

describe('the SSTV screen opening with the option', () => {
  it('tunes once on entry, through the band picker’s own call, when the option is on', async () => {
    const onSetFrequency = vi.fn()
    const { rerender } = render(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).toHaveBeenCalledTimes(1)
    expect(onSetFrequency).toHaveBeenCalledWith(14.23, '20m', 'USB')
    // The same visit: a poll, a re-render, the dial not yet reported moved — no second tune.
    rerender(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).toHaveBeenCalledTimes(1)
  })

  it('tunes again on the next entry', async () => {
    const onSetFrequency = vi.fn()
    const { rerender } = render(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    rerender(<SstvView snap={snapAt(7.074)} active={false} tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency, 'not while hidden').toHaveBeenCalledTimes(1)
    rerender(<SstvView snap={snapAt(7.074)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).toHaveBeenCalledTimes(2)
    expect(onSetFrequency).toHaveBeenLastCalledWith(7.171, '40m', 'LSB')
  })

  it('does nothing with the option off — the default', async () => {
    const onSetFrequency = vi.fn()
    render(<SstvView snap={snapAt(14.074)} active onSetFrequency={onSetFrequency} />)
    await settle()
    render(<SstvView snap={snapAt(14.074)} active tuneOnOpen={false} onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).not.toHaveBeenCalled()
  })

  it('waits for the settings at startup, then tunes; switching it on during a visit waits for the next entry', async () => {
    const onSetFrequency = vi.fn()
    const first = render(<SstvView snap={snapAt(14.074)} active tuneOnOpen={undefined} onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency, 'the answer is not in yet').not.toHaveBeenCalled()
    first.rerender(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).toHaveBeenCalledWith(14.23, '20m', 'USB')
    first.unmount()

    const later = vi.fn()
    const { rerender } = render(<SstvView snap={snapAt(14.074)} active tuneOnOpen={false} onSetFrequency={later} />)
    await settle()
    rerender(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={later} />)
    await settle()
    expect(later, 'this visit opened with it off').not.toHaveBeenCalled()
    rerender(<SstvView snap={snapAt(14.074)} active={false} tuneOnOpen onSetFrequency={later} />)
    rerender(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={later} />)
    await settle()
    expect(later).toHaveBeenCalledTimes(1)
  })

  it('does nothing while the screen is hidden', async () => {
    const onSetFrequency = vi.fn()
    render(<SstvView snap={snapAt(14.074)} active={false} tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).not.toHaveBeenCalled()
  })

  it('does not move a radio that is transmitting or sending a tune carrier, and does not come back for it', async () => {
    const onSetFrequency = vi.fn()
    const { rerender } = render(
      <SstvView snap={snapAt(14.074, { transmitting: true })} active tuneOnOpen onSetFrequency={onSetFrequency} />,
    )
    await settle()
    rerender(<SstvView snap={snapAt(14.074)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency, 'the entry passed while the radio was keyed').not.toHaveBeenCalled()
    rerender(<SstvView snap={snapAt(14.074, { tuning: true })} active={false} tuneOnOpen onSetFrequency={onSetFrequency} />)
    rerender(<SstvView snap={snapAt(14.074, { tuning: true })} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).not.toHaveBeenCalled()
  })

  it('does not move a dial already on an SSTV channel', async () => {
    const onSetFrequency = vi.fn()
    render(<SstvView snap={snapAt(14.236)} active tuneOnOpen onSetFrequency={onSetFrequency} />)
    await settle()
    expect(onSetFrequency).not.toHaveBeenCalled()
  })
})

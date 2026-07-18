// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { RttyCockpit, confidenceRuns } from './RttyCockpit'
import * as api from '../api'
import type { AppSnapshot, RttyState } from '../types'

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  rttyArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyArm = api.rttyArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>

const snap = {
  radio: {
    dialMhz: 14.08,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const IDLE: RttyState = { armed: false, afcHz: 0, afcLocked: false, text: '', charConf: [] }

beforeEach(() => {
  getRttyState.mockReset().mockResolvedValue(IDLE)
  rttyArm.mockReset().mockResolvedValue({ ...IDLE, armed: true })
  getLicensedBandPlan.mockReset().mockResolvedValue([])
})
afterEach(cleanup)

describe('RttyCockpit RX wiring', () => {
  it('renders without a snapshot (stream + macros + compose, no header)', async () => {
    render(<RttyCockpit snap={null} />)
    expect(screen.getByText('Arm RX to decode RTTY from the receive audio')).toBeTruthy()
    // No snapshot → no CockpitHeader (it needs live radio state).
    expect(document.querySelector('.cockpit-header')).toBeNull()
    // The macro row + compose line stay disabled — TX is a later, safety-reviewed wave.
    for (const label of ['CQ', 'Answer', 'Exchange', '73']) {
      const btn = screen.getByText(label).closest('button')
      expect(btn?.disabled, label).toBe(true)
    }
    expect(screen.getByLabelText('RTTY compose (disabled — TX not wired yet)')).toBeTruthy()
    await waitFor(() => expect(getRttyState).toHaveBeenCalled())
  })

  it('renders the mode badge + keying-backend pill with a snapshot', async () => {
    render(<RttyCockpit snap={snap} />)
    expect(screen.getByText('RTTY 45.45 · 170 Hz')).toBeTruthy()
    expect(screen.getByText('AFSK')).toBeTruthy()
    // No onSetFrequency handler → the display-only band pill.
    expect(screen.getByText('20m')).toBeTruthy()
    await waitFor(() => expect(getRttyState).toHaveBeenCalled())
  })

  it('offers the licensed RTTY band plan and QSYs through onSetFrequency', async () => {
    getLicensedBandPlan.mockResolvedValue([
      {
        band: '20m',
        group: 'HF',
        dialMhz: 14.083,
        mode: 'LSB',
        label: '20 m · RTTY',
        note: 'the 14.080–14.090 RTTY window',
      },
    ])
    const onSetFrequency = vi.fn()
    render(<RttyCockpit snap={snap} onSetFrequency={onSetFrequency} />)
    expect(getLicensedBandPlan).toHaveBeenCalledWith('rtty')
    const select = (await screen.findByLabelText('Band channel preset')) as HTMLSelectElement
    await waitFor(() => expect(select.querySelectorAll('option').length).toBeGreaterThan(1))
    fireEvent.change(select, { target: { value: '20m' } })
    // Lands on the watering hole with the channel's own sideband (RTTY = LSB).
    expect(onSetFrequency).toHaveBeenCalledWith(14.083, '20m', 'LSB')
  })

  it('polls the decoder and renders confidence-faded text + the locked AFC pill', async () => {
    getRttyState.mockResolvedValue({
      armed: true,
      afcHz: 12.4,
      afcLocked: true,
      text: 'CQ TEST',
      // "CQ TE" solid, "ST" low-confidence → faint tail run.
      charConf: [95, 95, 95, 90, 90, 20, 20],
    })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('RX armed')
    expect(screen.getByText('+12 Hz 🔒')).toBeTruthy()
    const faint = screen.getByText('ST')
    expect(faint.style.opacity).toBe('0.3')
    expect(screen.getByText('CQ TE').style.opacity).toBe('')
  })

  it('shows the unlocked AFC offset plain (no lock glyph)', async () => {
    getRttyState.mockResolvedValue({ ...IDLE, armed: true, afcHz: -8.2 })
    render(<RttyCockpit snap={snap} />)
    await screen.findByText('-8 Hz')
    expect(screen.queryByText(/🔒/)).toBeNull()
  })

  it('arms the RX decoder through rtty_arm and reflects the returned state', async () => {
    render(<RttyCockpit snap={snap} />)
    const arm = await screen.findByText('Arm RX')
    fireEvent.click(arm)
    expect(rttyArm).toHaveBeenCalledWith(true)
    await screen.findByText('RX armed')
  })

  it('does not poll while inactive (hidden keep-alive host)', () => {
    render(<RttyCockpit snap={snap} active={false} />)
    expect(getRttyState).not.toHaveBeenCalled()
  })
})

describe('confidenceRuns', () => {
  it('groups equal-confidence chars into runs and fades the low ones', () => {
    expect(confidenceRuns('ABCD', [90, 90, 20, 20])).toEqual([
      { text: 'AB', opacity: 1 },
      { text: 'CD', opacity: 0.3 },
    ])
  })

  it('treats missing confidence as solid — decoded text is never hidden', () => {
    expect(confidenceRuns('AB', [])).toEqual([{ text: 'AB', opacity: 1 }])
  })
})

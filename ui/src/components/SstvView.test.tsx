// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvState } from '../types'

vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>
const sstvArm = api.sstvArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>

const snap = {
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const IDLE: SstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  gallery: [],
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvArm.mockReset().mockResolvedValue({ ...IDLE, armed: true })
  getLicensedBandPlan.mockReset().mockResolvedValue([])
})
afterEach(cleanup)

describe('SstvView RX wiring', () => {
  it('renders without a snapshot (canvas empty-state + gallery, no header)', async () => {
    render(<SstvView snap={null} />)
    expect(screen.getByText('Tune 14.230 / 145.800 — images decode here')).toBeTruthy()
    expect(screen.getByText('Gallery')).toBeTruthy()
    // No snapshot → no CockpitHeader (it needs live radio state).
    expect(document.querySelector('.cockpit-header')).toBeNull()
    await waitFor(() => expect(getSstvState).toHaveBeenCalled())
  })

  it('arms the receiver through sstv_arm and shows the armed waiting state', async () => {
    render(<SstvView snap={snap} />)
    // RX-first: txState=false — no TX/RX pill in the header.
    expect(document.querySelector('.cockpit-txstate')).toBeNull()
    const arm = await screen.findByText('Arm')
    fireEvent.click(arm)
    expect(sstvArm).toHaveBeenCalledWith(true)
    await screen.findByText('Armed')
    expect(screen.getByText('Armed — waiting for a VIS header…')).toBeTruthy()
    // Slant trim is decoder-automatic; the manual control stays disabled.
    const slant = screen.getByLabelText(
      'SSTV slant trim (disabled — decoder not wired yet)',
    ) as HTMLInputElement
    expect(slant.disabled).toBe(true)
  })

  it('presents an in-flight VIS-detected image honestly ("decoding…" before lines land)', async () => {
    getSstvState.mockResolvedValue({
      ...IDLE,
      armed: true,
      mode: 'Scottie 1',
      linesDone: 0,
      linesTotal: 256,
    })
    render(<SstvView snap={snap} />)
    // Two-pass core: mode + total show immediately, lines land at completion —
    // never a fake progress count.
    await screen.findByText('decoding Scottie 1…')
    expect(screen.getByText('SSTV · Scottie 1')).toBeTruthy()
  })

  it('shows the line count once lines actually land', async () => {
    getSstvState.mockResolvedValue({
      ...IDLE,
      armed: true,
      mode: 'Robot 36',
      linesDone: 240,
      linesTotal: 240,
      previewRgbBase64: btoa('\x01\x02\x03\x04\x05\x06'),
      previewWidth: 2,
      previewHeight: 1,
    })
    render(<SstvView snap={snap} />)
    await screen.findByText('Robot 36 — 240/240 lines')
    expect(document.querySelector('.sstv-live-canvas')).toBeTruthy()
  })

  it('renders the gallery newest-first with mode / UTC / frequency captions', async () => {
    getSstvState.mockResolvedValue({
      ...IDLE,
      gallery: [
        {
          path: '/data/sstv-gallery/20260717T150000Z_scottie1.bmp',
          mode: 'Scottie 1',
          finishedUtc: '2026-07-17T15:00:00Z',
          freqMhz: 14.23,
          lines: 256,
        },
        {
          path: '/data/sstv-gallery/20260717T153000Z_pd120.bmp',
          mode: 'PD120',
          finishedUtc: '2026-07-17T15:30:00Z',
          freqMhz: 145.8,
          lines: 496,
        },
      ],
    })
    render(<SstvView snap={snap} />)
    await screen.findByText('PD120')
    const modes = Array.from(document.querySelectorAll('.sstv-thumb-mode')).map(
      (el) => el.textContent,
    )
    // Backend list is oldest-first; the gallery shows newest first.
    expect(modes).toEqual(['PD120', 'Scottie 1'])
    expect(screen.getByText('2026-07-17 15:30Z · 145.800 MHz')).toBeTruthy()
    expect(screen.getByText('2026-07-17 15:00Z · 14.230 MHz')).toBeTruthy()
  })

  it('offers the licensed SSTV band plan (ISS 145.800 star) and QSYs through onSetFrequency', async () => {
    getLicensedBandPlan.mockResolvedValue([
      {
        band: '2m',
        group: 'VHF',
        dialMhz: 145.8,
        mode: 'FM',
        label: '2 m · ISS downlink',
        note: 'ARISS events transmit PD120 images here',
      },
    ])
    const onSetFrequency = vi.fn()
    render(<SstvView snap={snap} onSetFrequency={onSetFrequency} />)
    expect(getLicensedBandPlan).toHaveBeenCalledWith('sstv')
    const select = (await screen.findByLabelText('Band channel preset')) as HTMLSelectElement
    await waitFor(() => expect(select.querySelectorAll('option').length).toBeGreaterThan(1))
    fireEvent.change(select, { target: { value: '2m' } })
    expect(onSetFrequency).toHaveBeenCalledWith(145.8, '2m', 'FM')
  })

  it('does not poll while inactive (hidden keep-alive host)', () => {
    render(<SstvView snap={snap} active={false} />)
    expect(getSstvState).not.toHaveBeenCalled()
  })
})

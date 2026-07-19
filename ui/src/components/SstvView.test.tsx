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
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
}))
// withErrorToast passes through to its action so the Send path exercises the real
// setOperatingMode → sstvSend sequence (returns null on reject, like the real one).
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>
const sstvArm = api.sstvArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const sstvSend = api.sstvSend as ReturnType<typeof vi.fn>
const sstvStop = api.sstvStop as ReturnType<typeof vi.fn>
const setOperatingMode = api.setOperatingMode as ReturnType<typeof vi.fn>

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
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvArm.mockReset().mockResolvedValue({ ...IDLE, armed: true })
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  sstvSend.mockReset().mockResolvedValue({ ...IDLE, sending: true, txMode: 'Scottie 1' })
  sstvStop.mockReset().mockResolvedValue(IDLE)
  setOperatingMode.mockReset().mockResolvedValue(snap)
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
    // The header now shows the TX-state pill (display-only here — the arm handler is
    // supplied by App in the real app; the dedicated arm test below covers the toggle).
    expect(document.querySelector('.cockpit-txstate')).not.toBeNull()
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

  it('exposes the Enable-Tx arm in the header and toggles it (the SSTV cockpit has no other TX arm)', async () => {
    const onSetTxEnabled = vi.fn()
    render(
      <SstvView
        snap={{ ...snap, radio: { ...snap.radio, txEnabled: false } } as AppSnapshot}
        onSetTxEnabled={onSetTxEnabled}
      />,
    )
    // TX disarmed → the pill is a clickable "TX Off" arm; clicking it enables transmit.
    const arm = await screen.findByRole('button', { name: /tx off/i })
    fireEvent.click(arm)
    expect(onSetTxEnabled).toHaveBeenCalledWith(true)
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
          fskId: 'KD9TAW',
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
    // The FSK-ID badge renders only for the entry that carries one — the
    // Scottie 1 entry (no fskId) shows no callsign badge.
    expect(screen.getByText('KD9TAW')).toBeTruthy()
    expect(document.querySelectorAll('.sstv-thumb-call').length).toBe(1)
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

// jsdom lacks a real 2D canvas, so we stub getContext (drawImage no-op +
// getImageData returning a zero-filled buffer of the requested size) and Image
// (fires onload with a synthetic 120×90 source) so the cover-crop + RGB-pack path
// runs. The packed base64's LENGTH tracks the mode's pixel count, which is what
// the (width, height) passed to sstvSend proves.
class MockImage {
  onload: (() => void) | null = null
  onerror: (() => void) | null = null
  naturalWidth = 120
  naturalHeight = 90
  width = 120
  height = 90
  set src(_v: string) {
    queueMicrotask(() => this.onload?.())
  }
}

function installCanvasStubs() {
  const ctx = {
    clearRect: vi.fn(),
    drawImage: vi.fn(),
    getImageData: (_x: number, _y: number, w: number, h: number) => ({
      data: new Uint8ClampedArray(w * h * 4),
      width: w,
      height: h,
    }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
    ctx as unknown as CanvasRenderingContext2D,
  )
  vi.stubGlobal('Image', MockImage)
  URL.createObjectURL = vi.fn(() => 'blob:mock')
  URL.revokeObjectURL = vi.fn()
  return ctx
}

/** Load a fake image through the file picker and wait for Send to enable. */
async function loadPicture() {
  const input = document.querySelector('input[type=file]') as HTMLInputElement
  const file = new File([new Uint8Array([1, 2, 3])], 'photo.png', { type: 'image/png' })
  fireEvent.change(input, { target: { files: [file] } })
  const send = (await screen.findByText('Send')) as HTMLButtonElement
  await waitFor(() => expect(send.disabled).toBe(false))
  return send
}

describe('SstvView TX panel', () => {
  beforeEach(() => {
    installCanvasStubs()
  })
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.restoreAllMocks()
  })

  it('Send is disabled until an image is loaded', async () => {
    render(<SstvView snap={snap} />)
    const send = (await screen.findByText('Send')) as HTMLButtonElement
    expect(send.disabled).toBe(true)
    await loadPicture()
  })

  it('Send preflights Phone (followFreq false) then calls sstvSend with the mode dims + slug', async () => {
    render(<SstvView snap={snap} />)
    const send = await loadPicture()
    fireEvent.click(send)
    // 14.23 MHz → HF default is Scottie 1 (320×256).
    await waitFor(() => expect(sstvSend).toHaveBeenCalled())
    expect(setOperatingMode).toHaveBeenCalledWith('phone', false)
    expect(sstvSend).toHaveBeenCalledWith(expect.any(String), 320, 256, 'scottie1')
    // Phone preflight runs before the send.
    expect(setOperatingMode.mock.invocationCallOrder[0]).toBeLessThan(
      sstvSend.mock.invocationCallOrder[0],
    )
  })

  it('changing the mode re-crops to the new dimensions', async () => {
    render(<SstvView snap={snap} />)
    const send = await loadPicture()
    const modeSelect = screen.getByLabelText('SSTV transmit mode') as HTMLSelectElement
    fireEvent.change(modeSelect, { target: { value: 'pd120' } })
    fireEvent.click(send)
    // PD-120 is 640×496 — the re-crop packed the new size.
    await waitFor(() =>
      expect(sstvSend).toHaveBeenCalledWith(expect.any(String), 640, 496, 'pd120'),
    )
  })

  it('Send is disabled and Stop enabled while sending; Stop calls sstvStop', async () => {
    getSstvState.mockResolvedValue({ ...IDLE, sending: true, txMode: 'Scottie 1' })
    render(<SstvView snap={snap} />)
    const send = (await screen.findByText('Send')) as HTMLButtonElement
    const stop = screen.getByText('Stop') as HTMLButtonElement
    await waitFor(() => expect(stop.disabled).toBe(false))
    // No image loaded AND sending → Send stays disabled.
    expect(send.disabled).toBe(true)
    fireEvent.click(stop)
    expect(sstvStop).toHaveBeenCalled()
  })

  it('renders the TX progress bar from txProgress / elapsed / total', async () => {
    getSstvState.mockResolvedValue({
      ...IDLE,
      sending: true,
      txMode: 'Scottie 1',
      txProgress: 0.37,
      txElapsedSecs: 68,
      txTotalSecs: 180,
    })
    render(<SstvView snap={snap} />)
    const bar = await waitFor(() => {
      const el = document.querySelector('[role=progressbar]')
      if (!el) throw new Error('no progressbar yet')
      return el
    })
    expect(bar.getAttribute('aria-valuenow')).toBe('37')
    // 180 − 68 = 112 s = 1:52 remaining.
    expect(screen.getByText('TX — Scottie 1 · 1:52 remaining')).toBeTruthy()
  })
})

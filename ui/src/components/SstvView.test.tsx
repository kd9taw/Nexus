// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'

// The idle band view mounts the real Waterfall, which needs `window.matchMedia`
// and a working canvas 2D context — jsdom provides neither. These tests are about
// the SSTV panel, so the waterfall is stubbed rather than propped up.
vi.mock('./Waterfall', () => ({ Waterfall: () => null }))

vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
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
const sstvAutoArm = api.sstvAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const sstvSend = api.sstvSend as ReturnType<typeof vi.fn>
const sstvStop = api.sstvStop as ReturnType<typeof vi.fn>
const setOperatingMode = api.setOperatingMode as ReturnType<typeof vi.fn>

const snap = {
  // ⚠️ REQUIRED, and not decoration: SSTV identifies the station by burning this call
  // into the transmitted picture, so Send refuses without it (see the guard below).
  mycall: 'KD9TAW',
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

const NO_HEALTH: SstvHealth = {
  armed: false,
  audioPeak: 0,
  lastAudioUnix: null,
  drains: 0,
  visSeen: 0,
  lastVisUnix: null,
  unknownVis: 0,
  lastUnknownVisCode: null,
  lastUnknownVisUnix: null,
  images: 0,
  lastImageUnix: null,
}

const IDLE: SstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: NO_HEALTH,
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvArm.mockReset().mockResolvedValue({ ...IDLE, armed: true })
  // Opening the view starts the receiver; the default here keeps the tests that are
  // NOT about arming on the pre-existing disarmed shape.
  sstvAutoArm.mockReset().mockResolvedValue(IDLE)
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  sstvSend.mockReset().mockResolvedValue({ ...IDLE, sending: true, txMode: 'Scottie 1' })
  sstvStop.mockReset().mockResolvedValue(IDLE)
  setOperatingMode.mockReset().mockResolvedValue(snap)
})
afterEach(cleanup)

describe('SstvView RX wiring', () => {
  it('renders without a snapshot (canvas empty-state + gallery, no header)', async () => {
    render(<SstvView snap={null} />)
    // The idle caption says what the RECEIVER is doing. With no state polled yet it
    // reads as stopped — never the old fixed "Tune 14.230 / 145.800" hint, which said
    // the same thing whether the app was deaf, stopped, or fine.
    expect(screen.getByText(/receiver is stopped/i)).toBeTruthy()
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
    await screen.findByText(/decoding Scottie 1…/)
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
    // The composer burns the station-ID plate with fillRect on every render, through a
    // save/setTransform/restore so a stale transform or alpha cannot weaken it.
    save: vi.fn(),
    restore: vi.fn(),
    setTransform: vi.fn(),
    fillRect: vi.fn(),
    fillStyle: '#000',
    globalAlpha: 1,
    imageSmoothingEnabled: false,
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

  // ---------------------------------------------------------------------------
  // ⭐ STATION IDENTIFICATION. SSTV transmit used to carry NONE — no burned-in
  // overlay, no CW ident, and the FSK ID is decode-only. The remedy is a callsign
  // burned into the picture (§97.119(b)(4): the call may go out "by an image
  // emission … when all or part of the communications are transmitted in the same
  // image emission"), which makes the callsign a TX GATE rather than a preference.
  //
  // These guards cover the VIEW's half — that the gate exists and that the ident is
  // stated. That the burned-in call is actually LEGIBLE after the mode's demodulator
  // has had it is a different question and is proved where the pixels are:
  // `crates/tempo-sstv/tests/id_legibility.rs` encodes the plate, decodes it with the
  // real decoder and reads the callsign back, for all 15 modes at three SNRs.
  // ---------------------------------------------------------------------------

  it('⭐ refuses to transmit with no callsign set — the picture IS the identification', async () => {
    const noCall = { ...snap, mycall: '' } as unknown as AppSnapshot
    render(<SstvView snap={noCall} />)
    const input = document.querySelector('input[type=file]') as HTMLInputElement
    const file = new File([new Uint8Array([1, 2, 3])], 'photo.png', { type: 'image/png' })
    fireEvent.change(input, { target: { files: [file] } })
    const send = (await screen.findByText('Send')) as HTMLButtonElement
    // A picture IS loaded — the composer says so — and Send is still refused.
    await waitFor(() => expect(screen.getByText(/photo\.png/)).toBeTruthy())
    expect(send.disabled).toBe(true)
    expect(send.title).toMatch(/callsign/i)
    fireEvent.click(send)
    expect(sstvSend).not.toHaveBeenCalled()
    // And it says why, where the operator is looking, rather than only in a tooltip.
    expect(screen.getByText(/No callsign set/i)).toBeTruthy()
  })

  it('names the callsign it is burning in, and where', async () => {
    render(<SstvView snap={snap} />)
    await loadPicture()
    expect(screen.getByText(/KD9TAW burned in/)).toBeTruthy()
  })

  it('tells the operator how long the rig will be keyed before they key it', async () => {
    // A 290 s PTT hold is an expensive place to discover a bad crop, and the airtime
    // is the exact encoder figure (sstv-modes.test.ts pins it against tx_duration_secs).
    render(<SstvView snap={snap} />)
    await loadPicture()
    expect(screen.getByText(/1:51 key-down/)).toBeTruthy()
    const modeSelect = screen.getByLabelText('SSTV transmit mode') as HTMLSelectElement
    fireEvent.change(modeSelect, { target: { value: 'pd290' } })
    await waitFor(() => expect(screen.getByText(/4:50 key-down/)).toBeTruthy())
  })

  it('⭐ refuses an iPhone HEIC BY NAME, and keeps the picture already loaded', async () => {
    render(<SstvView snap={snap} />)
    const send = await loadPicture()
    const input = document.querySelector('input[type=file]') as HTMLInputElement
    // ISO-BMFF `ftyp` box with the `heic` brand — recognised from the bytes, so a HEIC
    // renamed .jpg is caught too.
    const heic = new Uint8Array(16)
    heic.set([0, 0, 0, 0x10], 0)
    heic.set([...'ftyp'].map((c) => c.charCodeAt(0)), 4)
    heic.set([...'heic'].map((c) => c.charCodeAt(0)), 8)
    fireEvent.change(input, {
      target: { files: [new File([heic], 'IMG_0001.HEIC', { type: 'image/heic' })] },
    })
    // The message names the format AND the operator's fix, not "could not load".
    await waitFor(() => expect(screen.getByText(/Most Compatible/)).toBeTruthy())
    // The previously loaded picture and its crop survive the refusal.
    expect(send.disabled).toBe(false)
    expect(screen.getByText(/photo\.png/)).toBeTruthy()
  })

  it('warns, but does not refuse, when the picture is smaller than the raster', async () => {
    // The stubbed source is 120×90; Scottie 1 is 320×256. A webcam grab is a legitimate
    // thing to send and every other SSTV program enlarges it — so this is a badge that
    // survives to Send time, not a refusal.
    render(<SstvView snap={snap} />)
    const send = await loadPicture()
    expect(screen.getByText(/enlarged and look soft/)).toBeTruthy()
    expect(send.disabled).toBe(false)
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

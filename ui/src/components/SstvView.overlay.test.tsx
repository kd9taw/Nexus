// @vitest-environment jsdom
//
// TEXT OVERLAYS ON THE TRANSMIT PICTURE (operator request, 2026-08-16 — the
// MMSSTV-style compose). These tests pin the three contracts the feature must keep:
//
//  1. THE IDENT WINS. Overlays draw BEFORE the ID plate inside renderTx, so operator
//     text can cover artwork but structurally never the callsign. Asserted on the
//     recorded draw order, not on hope.
//  2. TEXT IS NOT AN AFFIRMATION. Adding overlays never touches idInImage (#50) — the
//     Send payload still says the plate is wanted.
//  3. LIVE QSO DATA COSTS NOTHING. The Reply preset reads the newest FSK ID from the
//     gallery; with no station heard it is disabled with the reason in its tooltip.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'
import { overlayRect, presetCq } from '../sstvOverlay'
import { plateFor } from '../sstvIdOverlay'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
  setRfPower: vi.fn(async () => {}),
}))
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
const setOperatingMode = api.setOperatingMode as ReturnType<typeof vi.fn>
const setRfPower = api.setRfPower as ReturnType<typeof vi.fn>

const snap = {
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

type Op = { style: string; args: [number, number, number, number] }

/** Canvas stub that RECORDS fillRect order with the fillStyle at call time — the draw
 *  order is the contract under test. */
function installRecordingCanvas() {
  const ops: Op[] = []
  const ctx = {
    clearRect: vi.fn(),
    drawImage: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    setTransform: vi.fn(),
    fillStyle: '#000',
    globalAlpha: 1,
    imageSmoothingEnabled: false,
    fillRect(x: number, y: number, w: number, h: number) {
      ops.push({ style: String(this.fillStyle), args: [x, y, w, h] })
    },
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
  return ops
}

async function loadPicture() {
  const input = document.querySelector('input[type=file]') as HTMLInputElement
  const file = new File([new Uint8Array([1, 2, 3])], 'photo.png', { type: 'image/png' })
  fireEvent.change(input, { target: { files: [file] } })
  const send = (await screen.findByText('Send')) as HTMLButtonElement
  await waitFor(() => expect(send.disabled).toBe(false))
  return send
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvArm.mockReset().mockResolvedValue({ ...IDLE, armed: true })
  sstvAutoArm.mockReset().mockResolvedValue(IDLE)
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  sstvSend.mockReset().mockResolvedValue({ ...IDLE, sending: true, txMode: 'Scottie 1' })
  setOperatingMode.mockReset().mockResolvedValue(snap)
  setRfPower.mockReset().mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('SSTV text overlays', () => {
  it('draws operator text UNDER the ID plate — the ident always paints last', async () => {
    const ops = installRecordingCanvas()
    render(<SstvView snap={snap} />)
    await loadPicture()

    ops.length = 0
    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    await waitFor(() => expect(ops.length).toBeGreaterThan(0))

    const canvas = document.querySelector('.sstv-tx-preview') as HTMLCanvasElement
    const W = canvas.width
    const H = canvas.height
    // Expected geometry from the SAME pure modules the component draws with, so the
    // assertion is about ORDER, not about re-deriving arithmetic.
    const cq = overlayRect(presetCq('KD9TAW'), W, H, (t, px) => t.length * px * 0.6)
    const plate = plateFor(W, H, 'KD9TAW')
    expect(plate).not.toBeNull()

    const idxOverlayBacking = ops.findIndex(
      (o) => o.args[0] === cq.x && o.args[1] === cq.y && o.args[2] === cq.w && o.args[3] === cq.h,
    )
    const idxPlateBacking = ops.findIndex(
      (o) =>
        o.args[0] === plate!.x && o.args[1] === plate!.y && o.args[2] === plate!.w && o.args[3] === plate!.h,
    )
    expect(idxOverlayBacking, 'the CQ backing plate was drawn').toBeGreaterThanOrEqual(0)
    expect(idxPlateBacking, 'the ID plate was drawn').toBeGreaterThanOrEqual(0)
    expect(idxOverlayBacking, 'overlay first, ident last — the ident wins any overlap').toBeLessThan(
      idxPlateBacking,
    )
  })

  it('adding text never flips the #50 affirmation — the Send still asks for the plate', async () => {
    installRecordingCanvas()
    render(<SstvView snap={snap} />)
    const send = await loadPicture()

    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    const affirm = screen.getByLabelText(/already shows my callsign/i) as HTMLInputElement
    expect(affirm.checked, 'operator text is not a callsign-in-artwork affirmation').toBe(false)

    fireEvent.click(send)
    await waitFor(() => expect(sstvSend).toHaveBeenCalled())
    // sstvSend(b64, w, h, mode, idInImage) — the last argument is the affirmation.
    expect(sstvSend.mock.calls[0][4]).toBe(false)
  })

  it('Reply is disabled until a station has been heard, then prefills both calls', async () => {
    installRecordingCanvas()
    const { unmount } = render(<SstvView snap={snap} />)
    await loadPicture()
    const reply = screen.getByRole('button', { name: 'Reply' }) as HTMLButtonElement
    expect(reply.disabled, 'no FSK ID heard yet').toBe(true)
    unmount()
    cleanup()

    const heard: SstvState = {
      ...IDLE,
      gallery: [
        {
          path: '/tmp/x.png',
          mode: 'Scottie 1',
          finishedUtc: '2026-08-16T12:00:00Z',
          freqMhz: 14.23,
          lines: 256,
          fskId: 'ON8ST',
        },
      ],
    }
    getSstvState.mockResolvedValue(heard)
    sstvAutoArm.mockResolvedValue(heard)
    render(<SstvView snap={snap} />)
    await loadPicture()
    const reply2 = screen.getByRole('button', { name: 'Reply' }) as HTMLButtonElement
    await waitFor(() => expect(reply2.disabled).toBe(false))
    fireEvent.click(reply2)
    expect((screen.getByLabelText('Overlay text') as HTMLInputElement).value).toBe('ON8ST DE KD9TAW 599')
  })
})

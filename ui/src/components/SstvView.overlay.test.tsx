// @vitest-environment jsdom
//
// TEXT OVERLAYS ON THE TRANSMIT PICTURE (operator request, 2026-08-16 — the
// MMSSTV-style compose). These tests pin the three contracts the feature must keep:
//
//  1. THE IDENT WINS. Overlays draw BEFORE the ID plate inside renderTx, so operator
//     text can cover artwork but structurally never the callsign. Asserted on the
//     recorded draw order, not on hope.
//  2. TEXT THAT SPELLS OUT THE CALL *IS* THE IDENT (operator, 2026-08-16 — "remove the
//     burn-in now that we have text"). An overlay containing the callsign retires the
//     plate and sets idInImage on the Send, because Rust re-burns the plate without it;
//     text that carries no call changes neither. This removes a DUPLICATE ident, never
//     the ident: delete that text and the plate comes back on the same draw.
//  3. LIVE QSO DATA COSTS NOTHING. The Reply preset reads the newest FSK ID from the
//     gallery; with no station heard it is disabled with the reason in its tooltip.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'
import { overlayRect, type OverlayItem } from '../sstvOverlay'
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

/** What the "+ Text" button adds — pinned here rather than imported, because the
 *  component builds it inline. The point of it is that it carries NO callsign. */
const PLAIN_TEXT: OverlayItem = {
  id: 'plain',
  text: 'TEXT',
  cx: 0.5,
  cy: 0.5,
  size: 2,
  style: 'crisp',
  color: 'white',
  treatment: 'plate',
}

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

/** Index of the ID plate's backing rect among the recorded ops, or -1. The plate is
 *  identified by its EXACT geometry from `plateFor` — the same pure module the component
 *  draws with — so "the plate was drawn" is never re-derived arithmetic. */
function plateIndexIn(ops: Op[]): number {
  const canvas = document.querySelector('.sstv-tx-preview') as HTMLCanvasElement
  const p = plateFor(canvas.width, canvas.height, 'KD9TAW')
  expect(p, 'the plate has geometry at this raster').not.toBeNull()
  return ops.findIndex(
    (o) => o.args[0] === p!.x && o.args[1] === p!.y && o.args[2] === p!.w && o.args[3] === p!.h,
  )
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
    // "+ Text" — text that does NOT carry the call, so the plate is still in play and
    // there is an order to assert. (The CQ preset spells out the call and retires it.)
    fireEvent.click(screen.getByRole('button', { name: '+ Text' }))
    await waitFor(() => expect(ops.length).toBeGreaterThan(0))

    const canvas = document.querySelector('.sstv-tx-preview') as HTMLCanvasElement
    const W = canvas.width
    const H = canvas.height
    // Expected geometry from the SAME pure module the component draws with, so the
    // assertion is about ORDER, not about re-deriving arithmetic.
    const item = overlayRect(PLAIN_TEXT, W, H, (t, px) => t.length * px * 0.6)

    const idxOverlayBacking = ops.findIndex(
      (o) => o.args[0] === item.x && o.args[1] === item.y && o.args[2] === item.w && o.args[3] === item.h,
    )
    const idxPlateBacking = plateIndexIn(ops)
    expect(idxOverlayBacking, 'the text backing plate was drawn').toBeGreaterThanOrEqual(0)
    expect(idxPlateBacking, 'the ID plate was drawn').toBeGreaterThanOrEqual(0)
    expect(idxOverlayBacking, 'overlay first, ident last — the ident wins any overlap').toBeLessThan(
      idxPlateBacking,
    )
  })

  it('text that does not carry the call leaves the plate and the #50 affirmation alone', async () => {
    const ops = installRecordingCanvas()
    render(<SstvView snap={snap} />)
    const send = await loadPicture()

    ops.length = 0
    fireEvent.click(screen.getByRole('button', { name: '+ Text' }))
    await waitFor(() => expect(ops.length).toBeGreaterThan(0))
    expect(plateIndexIn(ops), '"TEXT" identifies nobody — the plate stands').toBeGreaterThanOrEqual(0)

    const affirm = screen.getByLabelText(/already shows my callsign/i) as HTMLInputElement
    expect(affirm.checked, 'operator text is not a callsign-in-artwork affirmation').toBe(false)

    fireEvent.click(send)
    await waitFor(() => expect(sstvSend).toHaveBeenCalled())
    // sstvSend(b64, w, h, mode, idInImage) — the last argument is the affirmation.
    expect(sstvSend.mock.calls[0][4]).toBe(false)
  })

  it('text carrying the call retires the plate, and the Send says so', async () => {
    const ops = installRecordingCanvas()
    render(<SstvView snap={snap} />)
    const send = await loadPicture()

    ops.length = 0
    // "CQ CQ DE KD9TAW" — the operator's own text already identifies the station.
    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    await waitFor(() => expect(ops.length).toBeGreaterThan(0))
    expect(plateIndexIn(ops), 'the text carries the call — no second ident').toBe(-1)

    fireEvent.click(send)
    await waitFor(() => expect(sstvSend).toHaveBeenCalled())
    // The Rust side redraws the plate unless idInImage is set, so the computed
    // affirmation has to reach it or the preview and the wire disagree.
    expect(sstvSend.mock.calls[0][4]).toBe(true)
  })

  it('deleting the call-bearing text revives the plate', async () => {
    const ops = installRecordingCanvas()
    render(<SstvView snap={snap} />)
    await loadPicture()

    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    ops.length = 0
    fireEvent.click(screen.getByRole('button', { name: 'Remove overlay CQ CQ DE KD9TAW' }))
    await waitFor(() => expect(ops.length).toBeGreaterThan(0))
    // The backstop is the whole point: the picture stopped identifying the station, so
    // the plate is back on the same draw — never a window with neither.
    expect(plateIndexIn(ops), 'no text carries the call any more').toBeGreaterThanOrEqual(0)
  })

  it('the #50 checkbox still suppresses the plate on its own, with no text at all', async () => {
    const ops = installRecordingCanvas()
    render(<SstvView snap={snap} />)
    const send = await loadPicture()

    // Positive control: the load render DID draw the plate through this canvas, so the
    // -1 below is the checkbox suppressing it and not a recorder that went quiet.
    expect(plateIndexIn(ops), 'the plate is drawn before the affirmation').toBeGreaterThanOrEqual(0)

    ops.length = 0
    const affirm = screen.getByLabelText(/already shows my callsign/i) as HTMLInputElement
    fireEvent.click(affirm)
    // The handler that flipped this is the one that redraws — asserted so the -1 cannot
    // pass by the redraw never happening. With no overlays and no plate it fills nothing.
    expect(affirm.checked).toBe(true)
    expect(plateIndexIn(ops), "the operator's word still stands alone").toBe(-1)

    fireEvent.click(send)
    await waitFor(() => expect(sstvSend).toHaveBeenCalled())
    expect(sstvSend.mock.calls[0][4]).toBe(true)
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

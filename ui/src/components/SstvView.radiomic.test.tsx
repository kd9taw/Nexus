// @vitest-environment jsdom
//
// SSTV CAN'T SEND WHILE THE RADIO HAS THE MIC (operator ruling, 2026-10-04, "Refuse like the voice
// keyer"). With Nexus's own Flex client and native DAX audio on, Phone at the shack takes the
// radio's own mic while a picture goes out over DAX, so the radio would ignore the picture and the
// mic would carry the over. Send says so in the operator's language and sends nothing; without
// that state the same Send goes out as before. The toast module is the real one: what is asserted
// is the sentence the operator reads.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, fireEvent, waitFor, cleanup, act } from '@testing-library/react'
import { SstvView } from './SstvView'
import * as api from '../api'
import { subscribeToasts, dismissToast, type Toast } from '../toast'
import { DE } from '../i18n/de'
import { EN, installCatalog, setLocale } from '../i18n'
import type { AppSnapshot, SstvHealth, SstvState } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
  setRfPower: vi.fn(async () => {}),
  revealSstvGallery: vi.fn(async () => {}),
  sstvManualRx: vi.fn(),
  openPanelWindow: vi.fn(async () => {}),
}))

const sstvSend = api.sstvSend as ReturnType<typeof vi.fn>
const setOperatingMode = api.setOperatingMode as ReturnType<typeof vi.fn>

/** Phone at the shack on 20 m, TX on; `radioHasMic` as the snapshot reports it. */
const snapWith = (radioHasMic: boolean) =>
  ({
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.23,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      operatingMode: 'phone',
      transmitting: false,
      txEnabled: true,
      tuning: false,
      txAllowed: true,
      flexRadioHasMic: radioHasMic,
    },
  }) as unknown as AppSnapshot

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

function installCanvasStubs() {
  const ctx = {
    clearRect: vi.fn(),
    drawImage: vi.fn(),
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
}

/** Load a fake image through the file picker and wait for Send to enable. */
async function loadPicture() {
  const input = document.querySelector('input[type=file]') as HTMLInputElement
  const file = new File([new Uint8Array([1, 2, 3])], 'photo.png', { type: 'image/png' })
  fireEvent.change(input, { target: { files: [file] } })
  await waitFor(() =>
    expect(document.querySelector<HTMLButtonElement>('.sstv-tx-send')?.disabled).toBe(false),
  )
  return document.querySelector<HTMLButtonElement>('.sstv-tx-send')!
}

let toasts: Toast[] = []
let unsub: () => void = () => {}

beforeEach(() => {
  installCanvasStubs()
  ;(api.getSstvState as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue(IDLE)
  ;(api.sstvArm as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue({ ...IDLE, armed: true })
  ;(api.sstvAutoArm as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue(IDLE)
  ;(api.getLicensedBandPlan as ReturnType<typeof vi.fn>).mockReset().mockResolvedValue([])
  sstvSend.mockReset().mockResolvedValue({ ...IDLE, sending: true, txMode: 'Scottie 1' })
  setOperatingMode.mockReset().mockResolvedValue(snapWith(false))
  unsub = subscribeToasts((t) => {
    toasts = t
  })
})
afterEach(() => {
  cleanup()
  setLocale('en')
  for (const t of toasts) dismissToast(t.id)
  unsub()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('SSTV while the radio has the mic', () => {
  it('sends no picture and says why', async () => {
    render(<SstvView snap={snapWith(true)} />)
    const send = await loadPicture()
    await act(async () => {
      fireEvent.click(send)
    })
    expect(sstvSend).not.toHaveBeenCalled()
    expect(setOperatingMode).not.toHaveBeenCalled()
    expect(toasts.map((t) => [t.kind, t.message])).toEqual([
      ['error', EN['sstv.tx.send.radioHasMic']],
    ])
  })

  it('says it in the operator’s language', async () => {
    installCatalog('de', DE)
    setLocale('de')
    expect(DE['sstv.tx.send.radioHasMic']).toBeTruthy()
    render(<SstvView snap={snapWith(true)} />)
    const send = await loadPicture()
    expect(send.textContent).toBe(DE['sstv.tx.send.label'])
    await act(async () => {
      fireEvent.click(send)
    })
    expect(sstvSend).not.toHaveBeenCalled()
    expect(toasts.map((t) => t.message)).toEqual([DE['sstv.tx.send.radioHasMic']])
  })

  it('sends as before when the radio does not have the mic', async () => {
    render(<SstvView snap={snapWith(false)} />)
    const send = await loadPicture()
    await act(async () => {
      fireEvent.click(send)
    })
    await waitFor(() => expect(sstvSend).toHaveBeenCalledTimes(1))
    expect(toasts).toEqual([])
  })
})

// @vitest-environment jsdom
//
// #101 — ON WSPR THE WATERFALL SHOWS THE WSPR SUB-BAND. Every WSPR decoder searches 200 Hz
// around 1500 Hz; the waterfall still offered Std / Full / the zoom spans centred anywhere in
// 0–4 kHz, so most of what it drew was audio no WSPR signal can occupy, and a click there set
// an RX marker the engine now clamps back anyway. The reporter's ask: lock the view to the
// sub-band while WSPR is on.
//
// Same observation technique as Waterfall.zoom.test.tsx (jsdom has no canvas): the axis and
// the click handler read the SAME `view`, so a click at each canvas edge reads the window the
// axis labels.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { WF_F_MIN, WF_F_MAX } from '../waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

const ZOOM_KEY = 'nexus.waterfall.zoom'
const W = 400

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => cleanup())

function mount(fixedWindow?: { lo: number; hi: number }) {
  const onTune = vi.fn()
  const utils = render(
    <Waterfall
      transmitting={false}
      rxOffsetHz={1500}
      txOffsetHz={1500}
      theme="dark"
      onTune={onTune}
      fixedWindow={fixedWindow}
    />,
  )
  const canvas = utils.container.querySelector('canvas.waterfall-canvas') as HTMLCanvasElement
  canvas.getBoundingClientRect = () =>
    ({ left: 0, top: 0, right: W, bottom: 200, width: W, height: 200, x: 0, y: 0 }) as DOMRect
  onTune.mockClear()
  fireEvent.mouseDown(canvas, { button: 0, clientX: 0 })
  fireEvent.mouseDown(canvas, { button: 0, clientX: W })
  return {
    lo: onTune.mock.calls[0][0] as number,
    hi: onTune.mock.calls[1][0] as number,
    zoomPicker: utils.container.querySelector('select.wf-zoom'),
  }
}

describe('WSPR waterfall window (#101)', () => {
  it('a fixed window wins over a persisted zoom, and the zoom picker is not offered', () => {
    localStorage.setItem(ZOOM_KEY, '-1') // Full — the widest persisted choice
    const { lo, hi, zoomPicker } = mount({ lo: 1400, hi: 1600 })
    expect({ lo, hi }).toEqual({ lo: 1400, hi: 1600 })
    expect(zoomPicker, 'a zoom picker that can do nothing on WSPR').toBeNull()
  })

  it('control: without a fixed window the persisted zoom still applies', () => {
    localStorage.setItem(ZOOM_KEY, '-1')
    const { lo, hi, zoomPicker } = mount()
    expect({ lo, hi }).toEqual({ lo: WF_F_MIN, hi: WF_F_MAX })
    expect(zoomPicker).not.toBeNull()
  })
})

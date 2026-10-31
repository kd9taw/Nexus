// @vitest-environment jsdom
//
// Every mode's audio waterfall opens at Std (200–3000 Hz) until the operator picks a span IN THAT
// MODE, and a span picked in one mode is that mode's alone. All the cockpits draw the same
// component, and they used to keep their span under ONE key: a 1 kHz pick in RTTY made FT8, PSK
// and JS8 open on a 1 kHz window round their decoder too. Each mode now passes a key of its own.
// The key every mode shared is still READ, so a span chosen before each mode had its own still
// opens as it did; nothing writes it any more.
//
// The window is read back the way Waterfall.zoom.test.tsx reads it: a click at each edge of the
// canvas maps through the same `view` the axis is labelled from (jsdom has no 2D canvas).
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup, screen } from '@testing-library/react'
import { Waterfall } from './Waterfall'
import { RTTY_WATERFALL_SPANS, WF_F_MIN, WF_STD_HI } from '../waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

/** The key every mode shared before each had its own. */
const SHARED_KEY = 'nexus.waterfall.zoom'
const FT_KEY = 'nexus.waterfall.ft.zoom'
const RTTY_KEY = 'nexus.waterfall.rtty.zoom'
const PSK_KEY = 'nexus.waterfall.psk.zoom'
const W = 400
const STD = { lo: WF_F_MIN, hi: WF_STD_HI }

const location = window.location
function setSearch(search: string) {
  Object.defineProperty(window, 'location', { configurable: true, value: { ...location, search } })
}

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  Object.defineProperty(window, 'location', { configurable: true, value: location })
})

function mount(zoomKey?: string, opts: { buttons?: boolean; rxHz?: number } = {}) {
  const onTune = vi.fn()
  const utils = render(
    <Waterfall
      transmitting={false}
      rxOffsetHz={opts.rxHz ?? 1500}
      txOffsetHz={1500}
      theme="dark"
      onTune={onTune}
      zoomKey={zoomKey}
      spanChoices={opts.buttons ? RTTY_WATERFALL_SPANS : undefined}
    />,
  )
  const canvas = utils.container.querySelector('canvas.waterfall-canvas') as HTMLCanvasElement
  canvas.getBoundingClientRect = () =>
    ({ left: 0, top: 0, right: W, bottom: 200, width: W, height: 200, x: 0, y: 0 }) as DOMRect
  /** The audio frequency a left click at canvas x maps to. */
  const clickHz = (x: number) => {
    onTune.mockClear()
    fireEvent.mouseDown(canvas, { button: 0, clientX: x })
    return onTune.mock.calls[0][0] as number
  }
  return { clickHz, window: () => ({ lo: clickHz(0), hi: clickHz(W) }) }
}

const pick = (span: number) =>
  fireEvent.change(screen.getByRole('combobox', { name: 'Waterfall zoom span' }), {
    target: { value: String(span) },
  })

describe('every mode opens at Std on a fresh profile', () => {
  it.each([undefined, FT_KEY, RTTY_KEY, PSK_KEY, 'nexus.waterfall.js8.zoom'])('%s: Std, select on Std', (key) => {
    const m = mount(key)
    expect(m.window()).toEqual(STD)
    expect((screen.getByRole('combobox', { name: 'Waterfall zoom span' }) as HTMLSelectElement).value).toBe('0')
  })

  it('RTTY span buttons: the Std button (3k) is pressed and the window is 200–3000 Hz', () => {
    const m = mount(RTTY_KEY, { buttons: true })
    expect(screen.getByRole('button', { name: '3k' }).getAttribute('aria-pressed')).toBe('true')
    expect(m.window()).toEqual(STD)
  })
})

describe('a span picked in one mode is that mode’s alone', () => {
  it('a 1 kHz pick in RTTY leaves FT8 and PSK opening at Std', () => {
    mount(RTTY_KEY, { buttons: true })
    fireEvent.click(screen.getByRole('button', { name: '1k' }))
    cleanup()
    expect(mount(FT_KEY).window()).toEqual(STD)
    cleanup()
    expect(mount(PSK_KEY).window()).toEqual(STD)
    cleanup()
    expect(localStorage.getItem(RTTY_KEY)).toBe('1000')
    // Positive control: RTTY itself opens where it was left, 1 kHz round its decoder.
    expect(mount(RTTY_KEY, { buttons: true }).window()).toEqual({ lo: 1000, hi: 2000 })
  })

  it('a pick in FT8 leaves RTTY at Std, and is FT8’s on the next open', () => {
    mount(FT_KEY)
    pick(600)
    expect(localStorage.getItem(FT_KEY)).toBe('600')
    cleanup()
    expect(mount(RTTY_KEY, { buttons: true }).window()).toEqual(STD)
    cleanup()
    expect(mount(FT_KEY).window()).toEqual({ lo: 1200, hi: 1800 })
  })

  it('never writes the key every mode used to share', () => {
    mount(PSK_KEY)
    pick(-1)
    expect(localStorage.getItem(PSK_KEY)).toBe('-1')
    expect(localStorage.getItem(SHARED_KEY)).toBeNull()
  })
})

describe('a span stored before each mode had its own still wins', () => {
  it('every mode opens on the stored span until it picks its own', () => {
    localStorage.setItem(SHARED_KEY, '1000')
    for (const key of [FT_KEY, RTTY_KEY, PSK_KEY]) {
      expect(mount(key).window(), key).toEqual({ lo: 1000, hi: 2000 })
      cleanup()
    }
  })

  it('a mode’s own pick beats the stored shared one', () => {
    localStorage.setItem(SHARED_KEY, '1000')
    localStorage.setItem(RTTY_KEY, '0')
    expect(mount(RTTY_KEY).window()).toEqual(STD)
    cleanup()
    expect(mount(PSK_KEY).window()).toEqual({ lo: 1000, hi: 2000 })
  })

  it('a torn-off waterfall keeps the span it stored for itself before', () => {
    // The pop-out picked 600 under the shared key; the main window has since picked 2 kHz for FT.
    localStorage.setItem(`${SHARED_KEY}.waterfall`, '600')
    localStorage.setItem(FT_KEY, '2000')
    setSearch('?panel=waterfall')
    expect(mount(FT_KEY).window()).toEqual({ lo: 1200, hi: 1800 })
    cleanup()
    // Positive control: with nothing of its own it inherits the main window's FT pick.
    localStorage.removeItem(`${SHARED_KEY}.waterfall`)
    expect(mount(FT_KEY).window()).toEqual({ lo: 500, hi: 2500 })
  })

  it('a torn-off waterfall with nothing of its own opens on the main window’s old stored span', () => {
    localStorage.setItem(SHARED_KEY, '1000')
    setSearch('?panel=waterfall')
    expect(mount(FT_KEY).window()).toEqual({ lo: 1000, hi: 2000 })
  })
})

describe('click-to-tune at Std', () => {
  it.each([FT_KEY, RTTY_KEY, PSK_KEY])('%s maps a click to the same frequency', (key) => {
    const m = mount(key, { buttons: key === RTTY_KEY })
    // 200–3000 Hz over 400 px: 7 Hz a pixel.
    expect(m.clickHz(0)).toBe(200)
    expect(m.clickHz(100)).toBe(900)
    expect(m.clickHz(186)).toBe(1502)
    expect(m.clickHz(W)).toBe(3000)
  })
})

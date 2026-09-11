// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

// A paint target and a sampled radio row, not a replacement scope. The actual
// draw loop, pointer handling, signal detector and sideband math all execute.
vi.mock('../api', () => ({ getScopeRow: vi.fn(async () => ({ source: 'rx', loHz: 0, hiHz: 4000,
  row: Array.from({ length: 512 }, (_, i) => Math.abs(i - 200) <= 2 ? 0.9 : 0.1) })) }))

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('PointerEvent', MouseEvent)
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('ImageData', class {
    constructor(public data: Uint8ClampedArray, public width: number, public height: number) {}
  })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
    fillRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {}, fill() {}, stroke() {}, setLineDash() {},
    createLinearGradient: () => ({ addColorStop() {} }),
  } as unknown as CanvasRenderingContext2D)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 800, height: 200, right: 800, bottom: 200, toJSON() { return {} } })
  HTMLCanvasElement.prototype.setPointerCapture = vi.fn()
  localStorage.clear()
})
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers() })
async function draw(ms = 150) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

it.each(['USB', 'LSB', 'CW', 'CW-L'])('keeps the actual %s native signal target with a captured remote click', async sideband => {
  const native = vi.fn(), first = vi.fn(), later = vi.fn(), begin = vi.fn(() => first)
  const base = { sideband, dialHz: 14_074_000, active: true, interactive: true, theme: 'dark' as const, onTune: native }
  const ui = render(<PhoneScope {...base}/>); await draw()
  const canvas = () => ui.container.querySelector('canvas')!
  const press = () => fireEvent.pointerDown(canvas(), { button: 0, clientX: 400, clientY: 100 })
  const release = () => fireEvent.pointerUp(canvas(), { button: 0, clientX: 400, clientY: 100 })
  press(); release()
  expect(native).toHaveBeenCalledOnce()
  const target = native.mock.calls[0][0]
  expect(target.kind).toBe('click'); expect(target.dialHz).toBeGreaterThan(14_070_000); expect(target.dialHz).toBeLessThan(14_078_000)
  native.mockClear()
  ui.rerender(<PhoneScope {...base} onBeginClick={begin}/>); await draw()
  press(); expect(begin).toHaveBeenCalledOnce(); expect(first).not.toHaveBeenCalled()
  ui.rerender(<PhoneScope {...base} onBeginClick={() => later}/>); await draw(16)
  release()
  expect(first).toHaveBeenCalledExactlyOnceWith(target.dialHz)
  expect(later).not.toHaveBeenCalled(); expect(native).not.toHaveBeenCalled()
  release(); expect(first).toHaveBeenCalledOnce()
})

it.each(['move', 'edge', 'cancel', 'capture lost', 'blur', 'escape', 'disabled', 'unmount'])('a remote %s cannot fall through to native drag or resume its click', async event => {
  const native = vi.fn(), click = vi.fn(), begin = () => click
  const base = { sideband: 'USB', dialHz: 14_074_000, active: true, interactive: true, theme: 'dark' as const, onTune: native, onBeginClick: begin }
  const ui = render(<PhoneScope {...base}/>); await draw()
  const canvas = ui.container.querySelector('canvas')!
  fireEvent.pointerDown(canvas, { button: 0, clientX: 400, clientY: 100 })
  if (event === 'move' || event === 'edge') fireEvent.pointerMove(canvas, { button: 0, clientX: event === 'edge' ? 799 : 450, clientY: 100 })
  if (event === 'cancel') fireEvent.pointerCancel(canvas)
  if (event === 'blur') fireEvent.blur(window)
  if (event === 'escape') fireEvent.keyDown(window, { key: 'Escape' })
  if (event === 'capture lost') fireEvent.lostPointerCapture(canvas)
  if (event === 'disabled') { ui.rerender(<PhoneScope {...base} interactive={false}/>); ui.rerender(<PhoneScope {...base}/>) }
  if (event === 'unmount') ui.unmount()
  await draw(1000)
  fireEvent.pointerUp(canvas, { button: 0, clientX: 400, clientY: 100 })
  expect(click).not.toHaveBeenCalled(); expect(native).not.toHaveBeenCalled()
  if (event !== 'unmount') {
    fireEvent.pointerDown(canvas, { button: 0, clientX: 400, clientY: 100 })
    fireEvent.pointerUp(canvas, { button: 0, clientX: 400, clientY: 100 })
    expect(click).toHaveBeenCalledOnce(); expect(native).not.toHaveBeenCalled()
  }
})

it('a refused press creates no capture and cannot use the later callback', async () => {
  const native = vi.fn(), later = vi.fn()
  const base = { sideband: 'USB', dialHz: 14_074_000, active: true, interactive: true, theme: 'dark' as const, onTune: native }
  const ui = render(<PhoneScope {...base} onBeginClick={() => null}/>); await draw()
  const canvas = ui.container.querySelector('canvas')!
  fireEvent.pointerDown(canvas, { button: 0, clientX: 400, clientY: 100 })
  expect(canvas.setPointerCapture).not.toHaveBeenCalled()
  ui.rerender(<PhoneScope {...base} onBeginClick={() => later}/>); await draw(16)
  fireEvent.pointerUp(canvas, { button: 0, clientX: 400, clientY: 100 })
  expect(later).not.toHaveBeenCalled(); expect(native).not.toHaveBeenCalled()
})

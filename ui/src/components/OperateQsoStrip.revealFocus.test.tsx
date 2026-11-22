// @vitest-environment jsdom
//
// A ONE-JUMP REVEAL OF A QSO-STRIP CONTROL LANDS WHERE THE CONTROL REALLY IS (OperateQsoStrip useRevealFocus). Parked,
// FT's strip is a sticky box held up at the shell's foot, so the browser's reveal of a control on its lower lines stops
// short (Chrome, measured: stopvisible's one-jump reveal check). The hook reveals it again with the strip in its own
// place. What it must never do is the point here: move anything for a focus the browser did not reveal (preventScroll),
// move anything while a pointer is down on the strip, leave the strip unparked when the reveal throws, or outlive the
// strip. jsdom lays nothing out and runs no frames: the boxes, the shell's scroll and the frame are stubbed.
import { it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import type { RadioStatus } from '../types'

const radio = {
  dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true, txEnabled: true, transmitting: false, tuning: false,
  txBusyReason: null, rigKeyed: false, holdTxFreq: false, txEven: true, txCycleAuto: true,
} as unknown as RadioStatus

let frames: Map<number, FrameRequestCallback>
let nextFrame: number
let reveal: ReturnType<typeof vi.fn>
/** Where the strip stood at each reveal: `static` is its own place. */
let positionAtReveal: string[]

beforeEach(() => {
  frames = new Map()
  nextFrame = 1
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb: FrameRequestCallback) => {
    const id = nextFrame++
    frames.set(id, cb)
    return id
  })
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation((id: number) => {
    frames.delete(id)
  })
  positionAtReveal = []
  reveal = vi.fn(function (this: HTMLElement) {
    positionAtReveal.push(document.querySelector<HTMLElement>('.cockpit-qso')!.style.position)
  })
  ;(Element.prototype as unknown as { scrollIntoView: unknown }).scrollIntoView = reveal
  const box = (top: number, height: number) => ({ top, bottom: top + height, height, left: 0, right: 500, width: 500, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    // The shell is 400 px tall; every control of the strip is where a short reveal left a lower line: below it.
    if (this.tagName === 'MAIN') return box(0, 400)
    if (this.tagName === 'BUTTON') return box(454, 24)
    return box(0, 0)
  })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

function mount() {
  const noop = vi.fn()
  const view = render(
    <main className="layout single operate-cockpit">
      <OperateQsoStrip qso={null} radio={radio} onSetMode={noop} onCallCq={noop} onResend={noop} onFreetext={noop}
        onLog={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetHoldTxFreq={noop} />
    </main>,
  )
  const shell = document.querySelector<HTMLElement>('main')!
  let top = 0
  Object.defineProperty(shell, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => { top = v } })
  const scrollTo = (v: number) => {
    top = v
    shell.dispatchEvent(new Event('scroll'))
  }
  const control = document.querySelector<HTMLElement>('.cockpit-qso button')!
  return { view, shell, scrollTo, control, setTop: (v: number) => { top = v } }
}
const runFrames = () => {
  const due = [...frames.values()]
  frames.clear()
  for (const cb of due) cb(0)
}

it('CONTROL: a reveal the browser made short is made again with the strip in its own place, and parked after', () => {
  const { scrollTo, control, setTop } = mount()
  scrollTo(0)
  control.focus()
  setTop(139) // the browser's reveal, short of the control
  runFrames()
  expect(reveal).toHaveBeenCalledTimes(1)
  expect(positionAtReveal).toEqual(['static'])
  expect(document.querySelector<HTMLElement>('.cockpit-qso')!.style.position).toBe('')
})

it('a focus with preventScroll, which the browser did not reveal, moves nothing', () => {
  const { scrollTo, control } = mount()
  scrollTo(0)
  control.focus({ preventScroll: true })
  runFrames()
  expect(reveal).not.toHaveBeenCalled()
})

it('nothing is re-revealed while a pointer is down on the strip, and is again once it is let go', () => {
  const { scrollTo, control, setTop } = mount()
  const strip = document.querySelector<HTMLElement>('.cockpit-qso')!
  scrollTo(0)
  strip.dispatchEvent(new Event('pointerdown', { bubbles: true }))
  control.focus()
  setTop(139)
  runFrames()
  expect(reveal).not.toHaveBeenCalled()
  window.dispatchEvent(new Event('pointerup'))
  control.blur()
  scrollTo(0)
  control.focus()
  setTop(139)
  runFrames()
  expect(reveal).toHaveBeenCalledTimes(1)
})

it('a reveal that throws leaves the strip parked', () => {
  const { scrollTo, control, setTop } = mount()
  reveal.mockImplementation(() => {
    throw new Error('behavior: instant is not supported')
  })
  scrollTo(0)
  control.focus()
  setTop(139)
  expect(runFrames).toThrow()
  expect(document.querySelector<HTMLElement>('.cockpit-qso')!.style.position).toBe('')
})

it('unmounting cancels a pending frame and takes every listener away', () => {
  const { view, scrollTo, control, setTop, shell } = mount()
  const strip = document.querySelector<HTMLElement>('.cockpit-qso')!
  const stripOff = vi.spyOn(strip, 'removeEventListener')
  const shellOff = vi.spyOn(shell, 'removeEventListener')
  const windowOff = vi.spyOn(window, 'removeEventListener')
  scrollTo(0)
  control.focus()
  setTop(139)
  expect(frames.size).toBe(1)
  view.unmount()
  expect(frames.size, 'the pending frame was not cancelled').toBe(0)
  expect(stripOff.mock.calls.map((c) => c[0])).toEqual(expect.arrayContaining(['focusin', 'pointerdown']))
  expect(shellOff.mock.calls.map((c) => c[0])).toContain('scroll')
  expect(windowOff.mock.calls.map((c) => c[0])).toEqual(expect.arrayContaining(['pointerup', 'pointercancel']))
})

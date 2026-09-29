// @vitest-environment jsdom
//
// THE WHEEL ON A SLIDER (#384): what `WheelRange` promises every slider it carries, pinned on a
// stand-in slider with real state. The Phone cockpit's own sliders, and what each notch commands
// there, are `PhoneCockpit.wheel.test.tsx`.
//
// ⚠️ jsdom delivers these wheel events; it does not scroll, lay out or hit-test. That a real
// Chrome sends the wheel to the slider under the pointer, with the deltas used here, is checked
// in a real browser (the #384 report), not here.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { WheelRange, WHEEL_LATCH_MS, WHEEL_REST_MS } from './WheelRange'

type SliderProps = {
  start?: number
  wheelStep?: number
  min?: number
  max?: number
  step?: number
  disabled?: boolean
  onValue?: (v: number) => void
  down?: () => void
  up?: () => void
}
function Slider({ start = 50, wheelStep = 2, min = 0, max = 100, step, disabled, onValue, down, up }: SliderProps) {
  const [value, setValue] = useState(start)
  return (
    <WheelRange
      type="range"
      aria-label="level"
      min={min}
      max={max}
      step={step}
      value={value}
      disabled={disabled}
      onChange={(e) => {
        setValue(Number(e.target.value))
        onValue?.(Number(e.target.value))
      }}
      wheelStep={wheelStep}
      onPointerDown={down}
      onPointerUp={up}
    />
  )
}
const slider = () => screen.getByRole('slider', { name: 'level' }) as HTMLInputElement
/** One mouse notch: Chromium's 100 px. Negative is up. */
const notch = (dir: 'up' | 'down', target: Element = slider()) =>
  fireEvent.wheel(target, { deltaY: dir === 'up' ? -100 : 100, deltaMode: 0 })

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('a notch moves the slider as a drag would', () => {
  it('moves one wheel step up, through the slider’s own onChange', () => {
    const onValue = vi.fn()
    render(<Slider start={50} onValue={onValue} />)
    notch('up')
    expect(slider().value).toBe('52')
    expect(onValue.mock.calls).toEqual([[52]])
  })

  it('moves down, and stops at the ends of the range', () => {
    render(<Slider start={1} />)
    notch('down')
    notch('down')
    expect(slider().value, 'went below the minimum').toBe('0')
    cleanup()
    render(<Slider start={99} />)
    notch('up')
    expect(slider().value, 'went above the maximum').toBe('100')
  })

  it('adds a trackpad’s small movements up to a notch', () => {
    render(<Slider start={50} />)
    for (let i = 0; i < 3; i++) fireEvent.wheel(slider(), { deltaY: -25, deltaMode: 0 })
    expect(slider().value, 'three quarters of a notch moved the slider').toBe('50')
    fireEvent.wheel(slider(), { deltaY: -25, deltaMode: 0 })
    expect(slider().value).toBe('52')
  })

  it('takes a line-mode wheel’s line as a notch, and a horizontal (Shift) scroll as the same', () => {
    render(<Slider start={50} />)
    fireEvent.wheel(slider(), { deltaY: -1, deltaMode: 1 })
    expect(slider().value).toBe('52')
    fireEvent.wheel(slider(), { deltaX: -100, deltaY: 0, deltaMode: 0 })
    expect(slider().value).toBe('54')
  })

  it('bounds a flick to four notches', () => {
    render(<Slider start={50} />)
    fireEvent.wheel(slider(), { deltaY: -1000, deltaMode: 0 })
    expect(slider().value).toBe('58')
  })

  it('lands a fractional step on the slider’s own grid', () => {
    render(<Slider start={0.1} min={-1} max={1} step={0.05} wheelStep={0.05} />)
    notch('up')
    expect(slider().value).toBe('0.15')
  })
})

describe('a burst of notches is one drag', () => {
  it('starts the drag at its first notch and ends it once the wheel rests', () => {
    const down = vi.fn()
    const up = vi.fn()
    render(<Slider down={down} up={up} />)
    notch('up')
    notch('up')
    act(() => vi.advanceTimersByTime(WHEEL_REST_MS - 1))
    notch('up')
    expect(down, 'each notch started a drag of its own').toHaveBeenCalledTimes(1)
    act(() => vi.advanceTimersByTime(WHEEL_REST_MS - 1))
    expect(up, 'the drag ended while the wheel was still turning').not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(1))
    expect(up).toHaveBeenCalledTimes(1)
    notch('up')
    expect(down, 'a notch after the rest is a new drag').toHaveBeenCalledTimes(2)
  })

  it('still ends the drag when the slider goes away mid-burst', () => {
    const up = vi.fn()
    const view = render(<Slider up={up} />)
    notch('up')
    view.unmount()
    expect(up).toHaveBeenCalledTimes(1)
  })
})

describe('the wheel belongs to the slider under the pointer, never to a scroll passing over it', () => {
  it('keeps the pane from scrolling, and nothing above the slider hears the wheel', () => {
    const above = vi.fn()
    render(
      <div data-testid="pane">
        <Slider start={50} />
      </div>,
    )
    screen.getByTestId('pane').addEventListener('wheel', above)
    expect(notch('up'), 'the notch was left to scroll the pane').toBe(false)
    expect(above, 'an ancestor (the scope’s wheel tuning) also took the notch').not.toHaveBeenCalled()
  })

  it('leaves the wheel to the page on a disabled slider', () => {
    const above = vi.fn()
    render(
      <div data-testid="pane">
        <Slider start={50} disabled />
      </div>,
    )
    screen.getByTestId('pane').addEventListener('wheel', above)
    expect(notch('up'), 'a disabled slider stopped the scroll').toBe(true)
    expect(slider().value).toBe('50')
    expect(above).toHaveBeenCalledTimes(1)
  })

  it('lets a scroll that began on the page carry on across the slider', () => {
    render(<Slider start={50} />)
    notch('down', document.body)
    expect(notch('up'), 'the slider took a scroll that began on the page').toBe(true)
    expect(slider().value).toBe('50')
    act(() => vi.advanceTimersByTime(WHEEL_LATCH_MS))
    expect(notch('up')).toBe(false)
    expect(slider().value).toBe('52')
  })

  it('is a plain range input without a wheel step', () => {
    // Rendered bare: the stand-in's `wheelStep = 2` default would fire on an explicit undefined.
    const onChange = vi.fn()
    render(<WheelRange type="range" aria-label="level" min={0} max={100} value={50} onChange={onChange} />)
    expect(notch('up'), 'a slider with no wheel step stopped the scroll').toBe(true)
    expect(onChange).not.toHaveBeenCalled()
  })
})

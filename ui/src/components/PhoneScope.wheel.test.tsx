// @vitest-environment jsdom
//
// #384 ON THE SCOPE'S OWN SLIDERS. The G and Z sliders sit inside the scope strip, whose wheel
// tunes the rig (`useWheelTune` on the Phone cockpit's `.ph-scope-wrap`), so a wheel over G used to
// tune the VFO through a slider that looked like it was being scrolled. With `wheelSliders` (Phone)
// the wheel moves G or Z one of its own 0.05 steps and stops there. Without it (CW) the scope's
// wheel is exactly what it was: this host's change must not reach the other.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'
import { t } from '../i18n'

// The scope never draws here: no rows, and no 2D context, so its loop returns before it starts.
vi.mock('../api', () => ({ getScopeFrame: () => new Promise(() => {}) }))

beforeEach(() => {
  // G and Z are the cockpit's persisted scale record now, so one test's notch must not start the next.
  localStorage.clear()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The scope inside a stand-in for the strip whose wheel tunes the rig. */
function mountIn(wheelSliders: boolean) {
  const tune = vi.fn()
  render(
    <div data-testid="scope-wrap">
      <PhoneScope transmitting={false} theme="dark" active={false} wheelSliders={wheelSliders} />
    </div>,
  )
  screen.getByTestId('scope-wrap').addEventListener('wheel', tune)
  return tune
}
const gain = () => screen.getByRole('slider', { name: t('scope.gain.aria') }) as HTMLInputElement
const zero = () => screen.getByRole('slider', { name: t('scope.zero.aria') }) as HTMLInputElement
const notch = (el: Element, dir: 'up' | 'down') => fireEvent.wheel(el, { deltaY: dir === 'up' ? -100 : 100, deltaMode: 0 })

describe('the scope’s G and Z take the wheel in the Phone cockpit', () => {
  it('moves G and Z one step a notch, and the wheel does not also tune the rig', () => {
    const tune = mountIn(true)
    notch(gain(), 'up')
    notch(zero(), 'down')
    expect(gain().value).toBe('0.05')
    expect(zero().value).toBe('-0.05')
    expect(tune, 'a notch over G or Z also reached the scope’s wheel tuning').not.toHaveBeenCalled()
  })

  // THE CONTROL, and CW's behaviour: the host that does not opt in keeps its wheel.
  it('leaves the wheel to the scope where the host does not opt in', () => {
    const tune = mountIn(false)
    notch(gain(), 'up')
    expect(gain().value).toBe('0')
    expect(tune).toHaveBeenCalledTimes(1)
  })
})

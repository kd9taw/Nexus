// @vitest-environment jsdom
//
// The rig scope's analysis-window control (2026-08-15).
//
// This is the one scope control with no right answer for everybody — a genuine
// time-versus-frequency trade. 1024 (85 ms) resolves 25 WPM keying and doubles the carrier
// width; 4096 (341 ms) halves the width and doubles the smear. That is why it is a control at
// all, where the trace peak-hold beside it is not: the hold follows from the signal, this does
// not follow from anything but what the operator is trying to hear.
//
// WHAT THIS FILE CAN AND CANNOT PROVE. jsdom has no 2D canvas, so `getContext('2d')` returns
// null and the render effect bails before drawing — nothing here paints a pixel, and nothing
// here proves an FFT ran at any length. That is pinned where it is computable, in
// `tempo_core::spectrum` (`the_absolute_db_axis_reads_the_same_at_every_window_length` and
// `a_longer_window_actually_narrows_the_carrier`). What is left for this file is the control
// surface, plus the one guarantee that matters on upgrade:
//
//   WITH NOTHING STORED, THE RESOLVED WINDOW IS TODAY'S.
//
// Nobody's scope may change because they installed a build that gained a control. The stored
// value is therefore read as an OPT-IN, and every other state of storage — absent, blank,
// stale, written by some other build, naming a length this build does not ship — is the
// default.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

const WIN_KEY = 'nexus.phonescope.win'

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => cleanup())

/** The control, found the way an operator finds it — by what it says. */
function mount(): HTMLButtonElement {
  render(<PhoneScope transmitting={false} theme="dark" />)
  return screen.getByRole('button', { name: /^Scope resolution/ }) as HTMLButtonElement
}

describe('rig scope analysis window', () => {
  it('defaults to today’s window with nothing stored, and writes nothing until asked', () => {
    const btn = mount()
    expect(btn.textContent).toBe('23 Hz') // 2048-point, 171 ms — the shipped behaviour
    expect(btn.className).not.toContain('on') // not highlighted: nothing was opted out of
    expect(localStorage.getItem(WIN_KEY)).toBeNull()
  })

  it('a stale, blank or foreign stored value is the default too', () => {
    // '8192' is the interesting one: it is a length the plan considered and this build
    // deliberately does not ship, so a config written by any other tree must not select it.
    for (const stored of ['', '8192', 'BALANCED', '2048', 'true', 'sharpest']) {
      localStorage.setItem(WIN_KEY, stored)
      const btn = mount()
      expect(btn.textContent, `stored=${JSON.stringify(stored)}`).toBe('23 Hz')
      cleanup()
    }
  })

  it('cycles balanced → sharp → fast → balanced, and persists each step', () => {
    const btn = mount()
    fireEvent.click(btn)
    expect(btn.textContent).toBe('12 Hz')
    expect(localStorage.getItem(WIN_KEY)).toBe('sharp')
    // Highlighted, because the operator has now opted OUT of the default — the same signal the
    // 3D and pause buttons give, and the only on-screen cue that the display is not stock.
    expect(btn.className).toContain('on')

    fireEvent.click(btn)
    expect(btn.textContent).toBe('47 Hz')
    expect(localStorage.getItem(WIN_KEY)).toBe('fast')

    fireEvent.click(btn)
    expect(btn.textContent).toBe('23 Hz')
    expect(localStorage.getItem(WIN_KEY)).toBe('balanced')
    expect(btn.className).not.toContain('on')
  })

  it('restores a stored choice on mount', () => {
    localStorage.setItem(WIN_KEY, 'fast')
    const btn = mount()
    expect(btn.textContent).toBe('47 Hz')
    expect(btn.className).toContain('on')
  })
})

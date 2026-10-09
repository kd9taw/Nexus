// @vitest-environment jsdom
//
// The rig scope's analysis-window control (2026-08-15), now in its ⚙ strip.
//
// This is the one scope control with no right answer for everybody — a genuine
// time-versus-frequency trade. 1024 (85 ms) resolves 25 WPM keying and doubles the carrier
// width; 4096 (341 ms) halves the width and doubles the smear. That is why it is a control at
// all, where the trace peak-hold beside it is not: the hold follows from the signal, this does
// not follow from anything but what the operator is trying to hear.
//
// It moved from a cycling button in the header row into the scope's ⚙ strip (ScaleStrip.tsx),
// with the averaging and the detector, and is kept per cockpit in the scale record
// (spectrum/scaleSettings.ts) that already reads the old per-window key until its first write.
//
// WHAT THIS FILE CAN AND CANNOT PROVE. jsdom has no 2D canvas, so `getContext('2d')` returns
// null and the render effect bails before drawing — nothing here paints a pixel, and nothing
// here proves an FFT ran at any length. That is pinned where it is computable, in
// `tempo_core::spectrum` (`the_absolute_db_axis_reads_the_same_at_every_window_length` and
// `a_longer_window_actually_narrows_the_carrier`); that the poll ASKS for the chosen window is
// PhoneScope.cadence.test.tsx's. What is left for this file is the control surface, plus the one
// guarantee that matters on upgrade:
//
//   WITH NOTHING STORED, THE RESOLVED WINDOW IS TODAY'S.
//
// Nobody's scope may change because they installed a build that moved a control. A stored value
// is read as an OPT-IN, and every other state of storage — absent, blank, stale, written by some
// other build, naming a length this build does not ship — is the default.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

// THE BUDGET (2026-10-09). The slowest case here, "a stale, blank or foreign stored value is the default…", takes
// 0.26 s and 0.17 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

/** The window each rig scope used before the scale record (per window, shared by Phone and CW). */
const LEGACY_WIN_KEY = 'nexus.phonescope.win'
const PHONE_KEY = 'nexus.scope.phone'
const CW_KEY = 'nexus.scope.cw'
const ROWS_KEY = 'nexus.phonescope.rows'

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => cleanup())

const gear = () => screen.getByRole('button', { name: 'Display settings' })

/** Mount, open the ⚙ strip, and find the controls the way an operator does — by what they say. */
function mount(cockpit?: 'phone' | 'cw') {
  render(<PhoneScope transmitting={false} theme="dark" cockpit={cockpit} />)
  fireEvent.click(gear())
  const strip = screen.getByRole('group', { name: 'Scope display settings' })
  const windows = within(within(strip).getByRole('group', { name: 'Resolution' })).getAllByRole('button')
  return { strip, windows }
}
const pressed = (windows: HTMLElement[]) => windows.filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => b.textContent)
const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')

describe('rig scope analysis window', () => {
  it('lives behind ⚙: closed until asked for, and the toggle says so', () => {
    render(<PhoneScope transmitting={false} theme="dark" />)
    expect(gear().getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByRole('group', { name: 'Scope display settings' })).toBeNull()
    fireEvent.click(gear())
    expect(gear().getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByRole('group', { name: 'Scope display settings' })).toBeTruthy()
    fireEvent.click(gear())
    expect(screen.queryByRole('group', { name: 'Scope display settings' })).toBeNull()
  })

  it('defaults to today’s window with nothing stored, and writes nothing until asked', () => {
    const { windows } = mount()
    expect(windows.map((b) => b.textContent)).toEqual(['47 Hz', '23 Hz', '12 Hz'])
    expect(pressed(windows)).toEqual(['23 Hz']) // 2048-point, 171 ms — the shipped behaviour
    expect(localStorage.getItem(PHONE_KEY)).toBeNull()
    expect(localStorage.getItem(LEGACY_WIN_KEY)).toBeNull()
  })

  it('a stale, blank or foreign stored value is the default too', () => {
    // '8192' is the interesting one: it is a length the plan considered and this build
    // deliberately does not ship, so a config written by any other tree must not select it.
    for (const legacy of ['', '8192', 'BALANCED', '2048', 'true', 'sharpest']) {
      localStorage.setItem(LEGACY_WIN_KEY, legacy)
      const { windows } = mount()
      expect(pressed(windows), `stored=${JSON.stringify(legacy)}`).toEqual(['23 Hz'])
      cleanup()
    }
    localStorage.clear()
    for (const record of ['{"window":"8192"}', '{"window":null}', 'not json', '[]']) {
      localStorage.setItem(PHONE_KEY, record)
      const { windows } = mount()
      expect(pressed(windows), `record=${record}`).toEqual(['23 Hz'])
      cleanup()
    }
  })

  it('picks a window, highlights it, and persists it for this cockpit only', () => {
    const { windows } = mount()
    fireEvent.click(windows[2])
    expect(pressed(windows)).toEqual(['12 Hz'])
    expect(stored(PHONE_KEY).window).toBe('sharp')
    fireEvent.click(windows[0])
    expect(pressed(windows)).toEqual(['47 Hz'])
    expect(stored(PHONE_KEY).window).toBe('fast')
    // CW keeps its own record: a Phone choice is not a CW one.
    expect(localStorage.getItem(CW_KEY)).toBeNull()
  })

  it('restores a stored choice on mount: the old per-window key until the record is written', () => {
    localStorage.setItem(LEGACY_WIN_KEY, 'fast')
    expect(pressed(mount().windows)).toEqual(['47 Hz'])
    cleanup()
    localStorage.setItem(CW_KEY, JSON.stringify({ window: 'sharp' }))
    expect(pressed(mount('cw').windows)).toEqual(['12 Hz'])
  })
})

describe('the scope’s averaging default, per cockpit', () => {
  // The defaults set 2026-10-04 ahead of the operator's ruling: CW off, so keying is not flattened;
  // Phone 250 ms.
  const smooth = (strip: HTMLElement) => within(strip).getByRole('combobox', { name: /Smooth/ }) as HTMLSelectElement

  it('is 250 ms on Phone and Off on CW, with nothing stored', () => {
    expect(smooth(mount('phone').strip).value).toBe('250')
    cleanup()
    expect(smooth(mount('cw').strip).value).toBe('0')
  })
})

describe('the slow-scope look', () => {
  const look = (strip: HTMLElement) => within(strip.parentElement!).getByRole('button', { name: /Smooth scroll|Row per sweep/ })

  it('is smooth scroll with nothing stored, and writes nothing until asked', () => {
    const b = look(mount().strip)
    expect(b.textContent).toBe('Smooth scroll')
    expect(b.getAttribute('aria-pressed')).toBe('false')
    expect(localStorage.getItem(ROWS_KEY)).toBeNull()
  })

  it('switches to a row per sweep and back, persisting each', () => {
    const b = look(mount().strip)
    fireEvent.click(b)
    expect(b.textContent).toBe('Row per sweep')
    expect(b.getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem(ROWS_KEY)).toBe('sweep')
    fireEvent.click(b)
    expect(b.textContent).toBe('Smooth scroll')
    expect(localStorage.getItem(ROWS_KEY)).toBe('smooth')
  })

  it('only the exact stored word opts out; anything else is smooth scroll', () => {
    for (const v of ['', 'SWEEP', 'step', 'true', '1']) {
      localStorage.setItem(ROWS_KEY, v)
      expect(look(mount().strip).textContent, `stored=${JSON.stringify(v)}`).toBe('Smooth scroll')
      cleanup()
    }
    localStorage.setItem(ROWS_KEY, 'sweep')
    expect(look(mount().strip).textContent).toBe('Row per sweep')
  })
})

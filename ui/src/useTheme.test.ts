// @vitest-environment jsdom
//
// SYSTEM THEME — Light / Dark / System, with Dark staying the default for a machine that never
// chose (the operator's pick, 2026-09-26: "Dark, as today"; System is an option, and existing
// saved themes are untouched).
//
// The shape that keeps every [data-theme] selector and every theme-keyed canvas cache as it is:
// the operator's CHOICE ('system' included) is what is stored in `tempo-theme`, and the theme
// the page PAINTS — what `data-theme` carries and what the hook returns — is always the resolved
// 'light' or 'dark'. index-preseed.test.ts holds the first-paint copy of the resolution.
import { afterEach, describe, expect, it } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useTheme } from './useTheme'

const QUERY = '(prefers-color-scheme: dark)'
const attr = () => document.documentElement.getAttribute('data-theme')

/** A controllable prefers-color-scheme (jsdom has no matchMedia). */
function osPrefers(initial: 'dark' | 'light') {
  let dark = initial === 'dark'
  const listeners = new Set<() => void>()
  const mql = {
    get matches() {
      return dark
    },
    media: QUERY,
    addEventListener: (_t: string, f: () => void) => listeners.add(f),
    removeEventListener: (_t: string, f: () => void) => listeners.delete(f),
  }
  window.matchMedia = ((q: string) => {
    if (q !== QUERY) throw new Error(`unexpected media query ${q}`)
    return mql
  }) as unknown as typeof window.matchMedia
  return {
    flip(to: 'dark' | 'light') {
      dark = to === 'dark'
      act(() => listeners.forEach((f) => f()))
    },
    listeners,
  }
}

afterEach(() => {
  localStorage.clear()
  document.documentElement.removeAttribute('data-theme')
  delete (window as { matchMedia?: unknown }).matchMedia
})

describe('useTheme: the default stays Dark', () => {
  it('a machine that never chose is Dark — even when the computer prefers light', () => {
    // The disconfirming case for "System became the default": the OS says light and the screen
    // still paints dark, because nothing was chosen and Dark is the default.
    osPrefers('light')
    const { result } = renderHook(() => useTheme())
    expect(result.current[0]).toBe('dark')
    expect(result.current[2]).toBe('dark')
    expect(attr()).toBe('dark')
  })

  it('a saved Light or Dark is kept exactly, whatever the computer prefers', () => {
    for (const saved of ['light', 'dark'] as const) {
      osPrefers(saved === 'light' ? 'dark' : 'light')
      localStorage.setItem('tempo-theme', saved)
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], `saved ${saved}`).toBe(saved)
      expect(attr()).toBe(saved)
      unmount()
    }
  })

  it('the removed Amber theme still migrates to Dark', () => {
    localStorage.setItem('tempo-theme', 'amber')
    const { result } = renderHook(() => useTheme())
    expect(result.current[0]).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBe('dark')
  })
})

describe('useTheme: System follows the computer', () => {
  it('resolves to the OS setting at mount, and paints only light or dark', () => {
    for (const os of ['light', 'dark'] as const) {
      osPrefers(os)
      localStorage.setItem('tempo-theme', 'system')
      const { result, unmount } = renderHook(() => useTheme())
      expect(result.current[0], `OS ${os}`).toBe(os)
      expect(result.current[2]).toBe('system')
      expect(attr()).toBe(os)
      unmount()
    }
  })

  it('follows a change of the OS setting while it is on, and keeps "system" stored', () => {
    const os = osPrefers('dark')
    localStorage.setItem('tempo-theme', 'system')
    const { result } = renderHook(() => useTheme())
    expect(attr()).toBe('dark')
    os.flip('light')
    expect(attr(), 'the OS went light and the page did not follow').toBe('light')
    expect(result.current[0]).toBe('light')
    os.flip('dark')
    expect(attr()).toBe('dark')
    // The CHOICE is what is stored: writing the resolved value would turn System into a fixed
    // theme the next time the app starts.
    expect(localStorage.getItem('tempo-theme')).toBe('system')
  })

  it('picking System from Dark applies the OS setting at once', () => {
    osPrefers('light')
    const { result } = renderHook(() => useTheme())
    act(() => result.current[1]('system'))
    expect(attr()).toBe('light')
    expect(localStorage.getItem('tempo-theme')).toBe('system')
  })

  it('picking Dark after System stops following the OS', () => {
    // The mirror of the live-follow case: a listener left behind would flip a pinned theme the
    // next time the OS changed.
    const os = osPrefers('light')
    localStorage.setItem('tempo-theme', 'system')
    const { result } = renderHook(() => useTheme())
    act(() => result.current[1]('dark'))
    expect(attr()).toBe('dark')
    expect(os.listeners.size, 'the OS listener outlived System').toBe(0)
    os.flip('light')
    expect(attr()).toBe('dark')
    expect(localStorage.getItem('tempo-theme')).toBe('dark')
  })

  it('a platform that cannot say resolves System to Dark', () => {
    localStorage.setItem('tempo-theme', 'system')
    const { result } = renderHook(() => useTheme())
    expect(result.current[0]).toBe('dark')
    expect(attr()).toBe('dark')
  })
})

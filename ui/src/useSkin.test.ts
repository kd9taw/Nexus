// @vitest-environment jsdom
//
// THE BUILT-IN THEMES AT RUNTIME (useSkin.ts; features/skins.ts). The owner — App, and each
// pop-out's own document — is the only writer of `data-skin` and the `nexus-skin` key. A theme
// paints only on its own base, the standard themes are NO attribute and NO key, and a change tells
// the canvases that cache token colours (usePaletteKey) to repaint.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useState } from 'react'
import { useSkin } from './useSkin'
import { PALETTE_EVENT, usePaletteKey } from './usePaletteRoles'
import type { Theme } from './useTheme'

const root = () => document.documentElement

afterEach(() => {
  localStorage.clear()
  root().removeAttribute('data-skin')
  vi.restoreAllMocks()
})

/** useSkin under a page theme the test can flip, as App's useTheme would. */
function mount(initial: Theme) {
  return renderHook(() => {
    const [theme, setTheme] = useState<Theme>(initial)
    const [skin, setSkin] = useSkin(theme)
    return { skin, setSkin, setTheme }
  })
}

describe('useSkin — the owner', () => {
  it('a machine that never chose is on the standard theme: no attribute, no key', () => {
    const { result } = mount('dark')
    expect(result.current.skin).toBeNull()
    expect(root().getAttribute('data-skin')).toBeNull()
    expect(localStorage.getItem('nexus-skin')).toBeNull()
  })

  it('applies a saved theme on mount, on its own base, and writes nothing', () => {
    localStorage.setItem('nexus-skin', 'amber-lcd')
    const set = vi.spyOn(Storage.prototype, 'setItem')
    const { result } = mount('dark')
    expect(result.current.skin).toBe('amber-lcd')
    expect(root().getAttribute('data-skin')).toBe('amber-lcd')
    expect(set).not.toHaveBeenCalled()
  })

  it('never paints a theme on the other base: a dark theme on the light page is no attribute', () => {
    localStorage.setItem('nexus-skin', 'amber-lcd')
    const { result } = mount('light')
    expect(result.current.skin).toBeNull()
    expect(root().getAttribute('data-skin')).toBeNull()
    // …and it comes back with its base, since the pick is still the operator's.
    act(() => result.current.setTheme('dark'))
    expect(root().getAttribute('data-skin')).toBe('amber-lcd')
  })

  it('a saved value the table does not have is the standard theme, not a dead attribute', () => {
    localStorage.setItem('nexus-skin', 'amber')
    const { result } = mount('dark')
    expect(result.current.skin).toBeNull()
    expect(root().getAttribute('data-skin')).toBeNull()
  })

  it('a pick sets the attribute and the key and tells the readers; the standard theme clears both', () => {
    const heard = vi.fn()
    window.addEventListener(PALETTE_EVENT, heard)
    const { result } = mount('light')
    heard.mockClear()
    act(() => result.current.setSkin('paper'))
    expect(result.current.skin).toBe('paper')
    expect(root().getAttribute('data-skin')).toBe('paper')
    expect(localStorage.getItem('nexus-skin')).toBe('paper')
    expect(heard).toHaveBeenCalled()
    heard.mockClear()
    act(() => result.current.setSkin(null))
    window.removeEventListener(PALETTE_EVENT, heard)
    expect(root().getAttribute('data-skin')).toBeNull()
    expect(localStorage.getItem('nexus-skin')).toBeNull()
    expect(heard).toHaveBeenCalled()
  })

  it('the palette key the canvases cache on changes with the theme', () => {
    const { result } = mount('dark')
    const key = renderHook(() => usePaletteKey())
    const before = key.result.current
    act(() => result.current.setSkin('lagoon'))
    expect(key.result.current).not.toBe(before)
    expect(key.result.current).toContain('lagoon')
  })
})

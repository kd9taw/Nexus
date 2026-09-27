// @vitest-environment jsdom
//
// #215 TEXT SIZE — the hook half. styles-text-size.test.ts holds the sheet to the picks
// (+0 / +12 / +25 %); this file holds the one writer of `data-text-size` to the stored choice,
// and index-preseed.test.ts holds the first-paint copy of it. Each case below has an observable
// that differs when the code is wrong.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { TEXT_SIZE_STORAGE_KEY, useTextSize } from './useTextSize'

const attr = () => document.documentElement.getAttribute('data-text-size')

afterEach(() => {
  localStorage.clear()
  document.documentElement.removeAttribute('data-text-size')
  vi.restoreAllMocks()
})

describe('useTextSize', () => {
  it('is Normal on a machine that never chose, and says so on <html>', () => {
    const { result } = renderHook(() => useTextSize())
    expect(result.current[0]).toBe('normal')
    expect(attr()).toBe('normal')
  })

  it('reads a saved Large and Larger back', () => {
    for (const saved of ['large', 'larger'] as const) {
      localStorage.setItem(TEXT_SIZE_STORAGE_KEY, saved)
      const { result, unmount } = renderHook(() => useTextSize())
      expect(result.current[0], `saved '${saved}' did not load`).toBe(saved)
      expect(attr()).toBe(saved)
      unmount()
    }
  })

  it('a stored value it does not know is Normal, not whatever the string says', () => {
    // A hand-edited or future value must not reach the attribute: an unknown
    // data-text-size would match no CSS block and silently read as Normal anyway, but the
    // Settings row would then show no chip pressed.
    localStorage.setItem(TEXT_SIZE_STORAGE_KEY, 'huge')
    const { result } = renderHook(() => useTextSize())
    expect(result.current[0]).toBe('normal')
    expect(attr()).toBe('normal')
  })

  it('a change applies live and persists', () => {
    const { result } = renderHook(() => useTextSize())
    act(() => result.current[1]('larger'))
    expect(attr()).toBe('larger')
    expect(localStorage.getItem(TEXT_SIZE_STORAGE_KEY)).toBe('larger')
    act(() => result.current[1]('normal'))
    expect(attr()).toBe('normal')
    expect(localStorage.getItem(TEXT_SIZE_STORAGE_KEY)).toBe('normal')
  })

  it('unreadable storage falls back to Normal instead of throwing', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const { result } = renderHook(() => useTextSize())
    expect(result.current[0]).toBe('normal')
    // …and a pick still applies for this session even though it cannot be saved.
    act(() => result.current[1]('large'))
    expect(attr()).toBe('large')
  })
})

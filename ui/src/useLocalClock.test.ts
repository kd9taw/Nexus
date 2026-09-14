// @vitest-environment jsdom
//
// #253 — THE OPTIONAL LOCAL CLOCK IS A PER-MACHINE PREFERENCE, OFF BY DEFAULT.
//
// Local time is a fact about THIS computer's time zone, so it is stored the way density and field
// mode are (localStorage), not in the station settings a backup carries to another machine.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { LOCAL_CLOCK_STORAGE_KEY, useLocalClock } from './useLocalClock'

afterEach(() => {
  localStorage.clear()
  vi.restoreAllMocks()
})

describe('useLocalClock', () => {
  it('is off on a machine that never chose', () => {
    const { result } = renderHook(() => useLocalClock())
    expect(result.current[0]).toBe(false)
  })

  it('reads a saved choice', () => {
    localStorage.setItem(LOCAL_CLOCK_STORAGE_KEY, '1')
    const { result } = renderHook(() => useLocalClock())
    expect(result.current[0]).toBe(true)
  })

  it('persists both directions', () => {
    const { result } = renderHook(() => useLocalClock())
    act(() => result.current[1](true))
    expect(result.current[0]).toBe(true)
    expect(localStorage.getItem(LOCAL_CLOCK_STORAGE_KEY)).toBe('1')
    act(() => result.current[1](false))
    expect(localStorage.getItem(LOCAL_CLOCK_STORAGE_KEY)).toBe('0')
  })

  it('unreadable storage falls back to off instead of throwing', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const { result } = renderHook(() => useLocalClock())
    expect(result.current[0]).toBe(false)
  })
})

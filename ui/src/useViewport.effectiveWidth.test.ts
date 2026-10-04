// @vitest-environment jsdom
//
// useEffectiveWidthAtLeast — the one JS reading of the window's width that decides a layout (Phone's
// left side, from about 1280 px, 2026-10-03). It must read what useViewport PUBLISHES (`--vw-eff`,
// zoom-corrected; the index.html preseed before it), never `innerWidth`, so it cannot disagree with
// `[data-viewport]` or miss the UI zoom the way a raw size @media does.
import { describe, it, expect, afterEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useEffectiveWidthAtLeast } from './useViewport'

const publish = async (px: number | null) => {
  await act(async () => {
    if (px == null) document.documentElement.style.removeProperty('--vw-eff')
    else document.documentElement.style.setProperty('--vw-eff', `${px}px`)
    await Promise.resolve()
  })
}
afterEach(() => {
  document.documentElement.style.removeProperty('--vw-eff')
  document.documentElement.style.removeProperty('--ui-zoom')
})

describe('useEffectiveWidthAtLeast', () => {
  it('answers from the published --vw-eff, at the boundary, and follows it', async () => {
    await publish(1280)
    const { result } = renderHook(() => useEffectiveWidthAtLeast(1280))
    expect(result.current).toBe(true)
    await publish(1279.5)
    expect(result.current).toBe(false)
    await publish(3440)
    expect(result.current).toBe(true)
  })

  it('is false while nothing is published — the narrow fallback, never a guess', async () => {
    await publish(null)
    const { result } = renderHook(() => useEffectiveWidthAtLeast(1280))
    expect(result.current).toBe(false)
  })

  it('reads the EFFECTIVE width, not the window: a wide window at a large zoom is narrow', async () => {
    // jsdom's window is 1024 wide; a 2000 px effective width published by useViewport wins, and a
    // window this narrow at a 175 % zoom publishes 585 px, which is what must decide.
    await publish(2000)
    const { result } = renderHook(() => useEffectiveWidthAtLeast(1280))
    expect(result.current, 'it measured innerWidth').toBe(true)
    document.documentElement.style.setProperty('--ui-zoom', '1.75')
    await publish(585)
    expect(result.current).toBe(false)
  })
})

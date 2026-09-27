// @vitest-environment jsdom
//
// DENSITY — Comfortable / Standard / Compact / Touch. The ids are `guided` / `standard` /
// `dense` / `touch`; `guided` existed with no writer until the Settings row offered it.
//
// TOUCH is Comfortable PLUS finger-sized targets, and the attributes say exactly that:
// `data-density='guided'` (so every Comfortable rule applies unchanged) and `data-touch`
// (the `--touch` 48 px and chip rules in styles.css). An explicit attribute, never
// `@media (pointer: coarse)` — a touchscreen laptop with a mouse reports `fine`. The
// sheet's half is styles-text-size.test.ts; the first-paint half is index-preseed.test.ts.
import { afterEach, describe, expect, it } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { useDensity, type Density } from './useDensity'

const html = document.documentElement
const attrs = () => ({ density: html.getAttribute('data-density'), touch: html.getAttribute('data-touch') })

afterEach(() => {
  localStorage.clear()
  html.removeAttribute('data-density')
  html.removeAttribute('data-touch')
})

describe('useDensity', () => {
  it('is Standard with no touch attribute on a machine that never chose', () => {
    const { result } = renderHook(() => useDensity())
    expect(result.current[0]).toBe('standard')
    expect(attrs()).toEqual({ density: 'standard', touch: null })
  })

  it('Touch is Comfortable plus data-touch — and persists as touch', () => {
    const { result } = renderHook(() => useDensity())
    act(() => result.current[1]('touch'))
    expect(attrs()).toEqual({ density: 'guided', touch: '1' })
    expect(localStorage.getItem('nexus-density')).toBe('touch')
  })

  it('leaving Touch takes data-touch away again, whichever density follows', () => {
    // The mirror of the case above: an effect that only ever SET the attribute passes that
    // case and leaves every later density with touch-sized chips.
    const { result } = renderHook(() => useDensity())
    for (const next of ['guided', 'standard', 'dense'] as Density[]) {
      act(() => result.current[1]('touch'))
      act(() => result.current[1](next))
      expect(attrs(), `touch → ${next}`).toEqual({ density: next, touch: null })
    }
  })

  it('a saved touch comes back as touch', () => {
    localStorage.setItem('nexus-density', 'touch')
    const { result } = renderHook(() => useDensity())
    expect(result.current[0]).toBe('touch')
    expect(attrs()).toEqual({ density: 'guided', touch: '1' })
  })

  it('an unknown stored value is Standard', () => {
    localStorage.setItem('nexus-density', 'jumbo')
    const { result } = renderHook(() => useDensity())
    expect(result.current[0]).toBe('standard')
    expect(attrs()).toEqual({ density: 'standard', touch: null })
  })
})

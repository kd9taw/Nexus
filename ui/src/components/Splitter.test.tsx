// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { Splitter, clampSplitPct } from './Splitter'

// clampSplitPct: the ONE clamp formula for a split percentage, shared by the drag and
// the mount replay — bounds are [minPx, min(maxPx, 90% of span)], all in CSS px.
describe('clampSplitPct', () => {
  it('passes an in-range percentage through unchanged', () => {
    expect(clampSplitPct(30, 1000, 100, 420)).toBe(30) // 300px ∈ [100, 420]
  })

  it('caps at maxPx (a stored % from a shorter container re-enters range)', () => {
    // 77.8% of 1000 = 778px — the phone-scope worst case from the census: a scope
    // dragged to max at one window size must NOT reopen 60% past its own drag cap.
    expect(clampSplitPct(77.8, 1000, 100, 420)).toBe(42) // 420/1000
  })

  it('floors at minPx', () => {
    expect(clampSplitPct(5, 1000, 100, 420)).toBe(10) // 50px → 100px
  })

  it('caps at 90% of the span when that is tighter than maxPx (same as the drag)', () => {
    expect(clampSplitPct(95, 400, 100, 420)).toBe(90) // min(420, 360)=360 → 90%
  })

  it('returns the input unchanged for a hidden/zero-size container', () => {
    expect(clampSplitPct(88, 0, 100, 420)).toBe(88)
    expect(clampSplitPct(88, -5, 100, 420)).toBe(88)
  })
})

describe('Splitter mount replay', () => {
  beforeEach(() => localStorage.clear())
  afterEach(() => cleanup())

  /** A measurable target: jsdom rects are all-zero, so stub the box. */
  function targetEl(heightPx: number): HTMLElement {
    const el = document.createElement('div')
    el.getBoundingClientRect = () =>
      ({ top: 0, left: 0, width: 800, height: heightPx, right: 800, bottom: heightPx, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
    return el
  }

  it('clamps a stored out-of-range % against the CURRENT container box at mount', () => {
    // Persisted 90% (legal at some other geometry) against a 1000px-tall container with
    // a 420px cap: mount must apply 42%, not replay the raw 90%.
    localStorage.setItem('nexus.split.test.scope', '90')
    const el = targetEl(1000)
    render(
      <Splitter
        axis="y"
        varName="--test-h"
        target={{ current: el }}
        storageKey="nexus.split.test.scope"
        min={100}
        max={420}
        defaultPct={22}
        label="test"
      />,
    )
    expect(el.style.getPropertyValue('--test-h')).toBe('42%')
    // The clamp is apply-side only — the operator's stored value survives untouched
    // (it comes back into force on a geometry where it is legal again).
    expect(localStorage.getItem('nexus.split.test.scope')).toBe('90')
  })

  it('applies the default % when nothing is stored', () => {
    const el = targetEl(1000)
    render(
      <Splitter
        axis="y"
        varName="--test-h"
        target={{ current: el }}
        storageKey="nexus.split.test.scope"
        min={100}
        max={420}
        defaultPct={22}
        label="test"
      />,
    )
    expect(el.style.getPropertyValue('--test-h')).toBe('22%')
  })
})

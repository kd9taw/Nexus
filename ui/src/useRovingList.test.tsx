// @vitest-environment jsdom
//
// The roving-focus reveal path (census CLASS-6 #5): a bare focus() makes the
// browser walk the WHOLE ancestor chain revealing the row — panning
// overflow:hidden cockpit boxes that have no scrollbar and no wheel path back,
// so the cockpit stays offset until a view switch remounts it. The contract
// pinned here: focus moves SILENTLY (preventScroll) and the reveal is an
// explicit scrollIntoView({ block: 'nearest' }), which stops the walk at the
// first scroller.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { useRovingList } from './useRovingList'

function List({ n }: { n: number }) {
  const roving = useRovingList(n, () => {})
  return (
    <div role="listbox" aria-label="rows" onKeyDown={roving.containerProps.onKeyDown}>
      {Array.from({ length: n }, (_, i) => (
        <div
          key={i}
          role="option"
          aria-selected={i === roving.active}
          tabIndex={roving.rowProps(i).tabIndex}
          ref={roving.rowProps(i).ref as (el: HTMLDivElement | null) => void}
          onFocus={roving.rowProps(i).onFocus}
        >
          row {i}
        </div>
      ))}
    </div>
  )
}

// jsdom has no Element.scrollIntoView at all — install one so the call is
// observable (and so the hook's optional call actually fires).
const sivSpy = vi.fn()
const focusSpy = vi.spyOn(HTMLElement.prototype, 'focus')

beforeEach(() => {
  sivSpy.mockClear()
  focusSpy.mockClear()
  ;(HTMLElement.prototype as unknown as { scrollIntoView: typeof sivSpy }).scrollIntoView = sivSpy
})
afterEach(() => {
  delete (HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView
  cleanup()
})

describe('useRovingList keyboard focus', () => {
  it('moves focus with preventScroll and reveals via scrollIntoView(nearest)', () => {
    render(<List n={3} />)
    fireEvent.keyDown(screen.getByRole('listbox'), { key: 'ArrowDown' })
    const row1 = screen.getByRole('option', { name: 'row 1' })
    expect(document.activeElement).toBe(row1)
    // The focus that moved the roving tab stop must NOT let the browser
    // auto-reveal (that is the ancestor-chain walk)…
    expect(focusSpy).toHaveBeenLastCalledWith({ preventScroll: true })
    // …the reveal is the explicit nearest-block scroll on the same row.
    expect(sivSpy).toHaveBeenCalledWith({ block: 'nearest' })
    expect(sivSpy.mock.contexts[sivSpy.mock.contexts.length - 1]).toBe(row1)
  })
})

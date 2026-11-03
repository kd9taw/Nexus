// @vitest-environment jsdom
//
// THE NEEDED BOARD'S SEARCH BOX IN A NARROW HOST, computed over the real sheets (cssCascade.testkit:
// jsdom lays nothing out, so these are the cascade winners). The board took the Spots board's search
// box when its filters were matched to the Spots board's; the box holds 240 px, and Connect's Needed
// box in the dashboard rail is 175 px. The Spots box already lets its search shrink with the head;
// the Needed box must take the same rule, or its head runs past the rail.
//
// The control: in a cockpit pane, the search keeps the width it has always had on the Spots pane.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { css, loadSheets } from '../../cssCascade.testkit'
import { NeededBox } from './NeededBox'
import { NeededPanel } from '../NeededPanel'

beforeAll(() => loadSheets())
afterEach(() => {
  cleanup()
  localStorage.clear()
})

const board = {
  alerts: [],
  bandPlan: [],
  selectedCall: null,
  onQsy: () => {},
  onSelect: () => {},
}

describe("the Needed box's search box", () => {
  it('shrinks with the head, by the Spots box rule', () => {
    render(<NeededBox board={board} />)
    const wrap = document.querySelector('.cn-needed .np-search')!
    expect(wrap, 'the board draws the shared search box').not.toBeNull()
    expect(css(wrap, 'min-width'), 'the wrapper may shrink below its content').toBe('0px')
    expect(css(wrap, 'flex-basis')).toBe('6em')
    expect(css(wrap.querySelector('input')!, 'width'), 'the input fills what the head gives it').toBe('100%')
  })

  it('keeps its 240 px in a cockpit pane, as the Spots pane does', () => {
    render(<NeededPanel {...board} pane={{ filterKey: 'nexus.cw.neededFilters' }} />)
    expect(css(document.querySelector('.np-search input')!, 'width')).toBe('240px')
  })
})

describe('the fixed "Heard on my continent" chip', () => {
  it('does not look like a control', () => {
    render(<NeededPanel {...board} />)
    fireEvent.click(document.querySelector('.np-filter-toggle')!)
    const fixed = document.querySelector('.np-chip-fixed')
    expect(fixed, 'the board states the rule').not.toBeNull()
    expect(css(fixed!, 'cursor')).toBe('default')
    // The control: a real chip keeps its pointer.
    expect(css(document.querySelector('button.np-chip')!, 'cursor')).toBe('pointer')
  })
})

// @vitest-environment jsdom
//
// THE STATE CHIP SAYS WHERE ITS STATE CAME FROM. The station places a heard station in a US state
// from the park or summit it is activating, from its licence, or from its grid, and sends which
// with the row (`stateFrom`). The chip's tooltip said "best-guess from the grid" for all three, so
// it was wrong for a state the licence or the park placed.

import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, render, within } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import { t } from '../i18n'
import type { NeedAlert } from '../types'

/** A new-state row for `call`, its state placed by `stateFrom`. */
const stateRow = (call: string, stateFrom?: string) =>
  ({
    call, entity: 'United States', band: '20m', zone: 5, tags: ['NewState'], priority: 60,
    headline: 'New state — ND on 20m (United States)', mode: 'CW', freqMhz: 14.025, stateFrom,
  }) as NeedAlert

const BOARD = [
  stateRow('W1ABC', 'park'),
  stateRow('W8LIC', 'license'),
  stateRow('W8GRD', 'grid'),
  // A station older than this window sends no source; a newer one may send one this window does
  // not know.
  stateRow('W8OLD'),
  stateRow('W8NEW', 'county'),
]

/** The STATE chip's tooltip on `call`'s row. */
const stateTitle = (call: string) => {
  const row = [...document.querySelectorAll<HTMLElement>('[role="row"]')].find((r) =>
    within(r).queryByText(call),
  )!
  return within(row).getByText(t('need.chip.newState.label')).getAttribute('title')
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  render(<NeededPanel alerts={BOARD} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />)
})
afterEach(cleanup)

describe("the Needed board's STATE chip says where the state came from", () => {
  it('names the park or summit, the licence or the grid, whichever placed it', () => {
    // Only the state the grid placed says it came from the grid.
    expect(['W1ABC', 'W8LIC', 'W8OLD'].filter((call) => /grid/i.test(stateTitle(call) ?? ''))).toEqual([])
    expect(stateTitle('W8GRD')).toMatch(/grid/i)
    expect(stateTitle('W1ABC')).toBe(t('need.chip.newState.title.park'))
    expect(stateTitle('W8LIC')).toBe(t('need.chip.newState.title.license'))
    expect(stateTitle('W8GRD')).toBe(t('need.chip.newState.title.grid'))
  })

  it('names no source when the row does not say', () => {
    expect(stateTitle('W8OLD')).toBe(t('need.chip.newState.title'))
    expect(stateTitle('W8NEW')).toBe(t('need.chip.newState.title'))
  })
})

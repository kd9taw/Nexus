// @vitest-environment jsdom
//
// AN ACTIVATOR IN A STATE THE LOG STILL NEEDS SHOWS AS A NEW STATE ON THE NEEDED BOARD (operator,
// 2026-10-06). The station scores a POTA/SOTA activator in its park's or summit's state, on the
// hunter feed's own row and on a cluster or radio row alike (`read_need_alerts`), and sends the row
// with the state need first. This pins what the board shows for it: the STATE chip ahead of the park
// and the programme, the row in the state colour, the headline naming the state and the park, and
// the New state filter keeping the row.

import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import { t } from '../i18n'
import type { NeedAlert } from '../types'

/** Two rows as the station sends them: an activator at a North Dakota park, a state the log lacks
 * on 20 m, and one at a Montana park, a state the log holds there. */
const BOARD = [
  {
    call: 'W1ABC', entity: 'United States', band: '20m', zone: 5, tags: ['NewState', 'NewPark', 'Pota'],
    priority: 60, headline: 'New state — ND on 20m (United States) · POTA US-0065', mode: 'Phone',
    freqMhz: 14.285,
  },
  {
    call: 'W1XYZ', entity: 'United States', band: '20m', zone: 5, tags: ['NewPark', 'Pota'],
    priority: 20, headline: 'POTA US-0001', mode: 'Phone', freqMhz: 14.285,
  },
] as NeedAlert[]

const panel = () =>
  render(<NeededPanel alerts={BOARD} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />)
/** The board's row for `call`, if it shows one. */
const row = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('[role="row"]')].find((r) => within(r).queryByText(call))
const chips = (r: HTMLElement) => [...r.querySelectorAll('.need-chip')].map((c) => c.textContent)

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
})
afterEach(cleanup)

describe('an activator in a state the log still needs is a new state on the Needed board', () => {
  it('leads the row with the STATE chip, in the state colour, naming the state and the park', () => {
    panel()
    const nd = row('W1ABC')!
    expect(chips(nd)).toEqual([t('need.chip.newState.label'), t('need.chip.newPark.label'), 'POTA'])
    expect(nd.classList.contains('need-state')).toBe(true)
    expect(within(nd).getByText(/New state — ND on 20m .* POTA US-0065/)).toBeTruthy()
    // CONTROL: the activator in a state the log holds leads with the park, in the park's colour.
    const mt = row('W1XYZ')!
    expect(chips(mt)).toEqual([t('need.chip.newPark.label'), 'POTA'])
    expect(mt.classList.contains('need-state')).toBe(false)
  })

  it('keeps the row under the New state filter, and only that row', () => {
    panel()
    expect(row('W1XYZ')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: t('needed.filter.toggle.idle') }))
    fireEvent.click(screen.getByRole('button', { name: t('needed.filter.newState') }))
    expect(row('W1ABC')).toBeTruthy()
    expect(row('W1XYZ')).toBeUndefined()
  })
})

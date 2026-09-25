// @vitest-environment jsdom
//
// THE NEEDED BOARD'S "WATCH LIST" CHIP KEEPS EXACTLY THE STATION'S WATCHED ROWS (operator,
// 2026-09-24: "watched counts as needed", offered as "a watched station is on the Needed board
// when heard, and the Watch list chip shows exactly your watched stations"). The station marks
// every heard station on the watch list `Wanted` and puts it first (`read_need_alerts`), for this
// window and for every Remote browser it serves; the chip keeps those rows and nothing else.
//
// It does not consult this window's own copy of the list. A Remote browser's watch list is its
// own, and adding rows by it would show stations the station was never asked to watch — more than
// "exactly". A row an older station tagged `Wanted` from its retired wanted list reads the same.

import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import { t } from '../i18n'
import { newWatchFilter, saveWatchlist } from '../watchlist'
import type { NeedAlert } from '../types'

const alert = (call: string, entity: string, over: Partial<NeedAlert> = {}): NeedAlert =>
  ({
    call, entity, band: '20m', zone: 5, tags: ['NewBand'], priority: 50,
    headline: `New band — ${entity} 20m`, mode: 'FT8', freqMhz: null, ...over,
  }) as NeedAlert

/** The station's board: two watched rows (one needed for nothing else), two that are not. */
const BOARD = [
  alert('W1AW', 'United States', { tags: ['Wanted'], priority: 120, headline: 'Watch list — United States' }),
  alert('VK9XX', 'Christmas Island', {
    tags: ['Wanted', 'NewEntity'], priority: 120, headline: 'Watch list · New one — Christmas Island',
  }),
  alert('3Y0J', 'Bouvet', { tags: ['NewEntity'], priority: 100, headline: 'New one — Bouvet' }),
  alert('K1ABC', 'United States'),
]

const panel = (alerts: NeedAlert[] = BOARD) =>
  render(<NeededPanel alerts={alerts} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />)
/** The calls the board shows, top to bottom. */
const shown = (alerts: NeedAlert[] = BOARD) =>
  [...document.querySelectorAll('[role="row"]')]
    .map((r) => alerts.find((a) => within(r as HTMLElement).queryByText(a.call))?.call)
    .filter((c): c is string => !!c)
const pickWatchChip = () => {
  fireEvent.click(screen.getByRole('button', { name: t('needed.filter.toggle.idle') }))
  fireEvent.click(screen.getByRole('button', { name: t('needed.filter.wanted') }))
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
})
afterEach(cleanup)

describe('the Watch list chip keeps the rows the station marked, and only those', () => {
  it('keeps exactly the watched rows, first', () => {
    panel()
    // Positive control: every row is on the board before the chip, the watched ones first.
    expect(shown().slice(0, 2).sort()).toEqual(['VK9XX', 'W1AW'])
    expect(shown()).toHaveLength(BOARD.length)
    pickWatchChip()
    expect(shown().sort()).toEqual(['VK9XX', 'W1AW'])
  })

  it("FIX: this window's own watch list adds no row (a Remote browser's list is its own)", () => {
    saveWatchlist([newWatchFilter('call', '3Y0J'), newWatchFilter('dxcc', 'United States')])
    panel()
    pickWatchChip()
    expect(shown().sort()).toEqual(['VK9XX', 'W1AW'])
  })

  it('with no watched row it keeps none, and never falls back to showing everything', () => {
    const unwatched = BOARD.slice(2)
    panel(unwatched)
    pickWatchChip()
    expect(shown(unwatched)).toEqual([])
    fireEvent.click(screen.getByRole('button', { name: t('needed.filter.all') }))
    expect(shown(unwatched)).toHaveLength(unwatched.length)
  })
})

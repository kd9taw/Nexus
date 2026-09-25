// @vitest-environment jsdom
//
// THE NEEDED BOARD'S "WATCH LIST" CHIP FILTERS THE REAL WATCH LIST (operator, 2026-09-24: "One
// list"). The chip used to keep only rows the station tagged `Wanted`, from `settings.wantedCalls`
// — a hidden list with no editor since the watch list replaced it (Settings ▸ Spots & Alerts). It
// now keeps the rows the watch list names: a call or prefix, a DXCC entity, or a grid, matched
// exactly as the WATCH tile on the roster, the Stations list and Spots matches them, and it follows
// an edit of the list live. A row an older station still tags `Wanted` keeps matching (a Remote
// browser watching a station that has not been upgraded).

import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import { t } from '../i18n'
import { newWatchFilter, saveWatchlist } from '../watchlist'
import type { NeedAlert } from '../types'

const alert = (call: string, entity: string, over: Partial<NeedAlert> & { grid?: string | null } = {}): NeedAlert =>
  ({
    call, entity, band: '20m', zone: 5, tags: ['NewBand'], priority: 50,
    headline: `New band — ${entity} 20m`, mode: 'FT8', freqMhz: null, ...over,
  }) as NeedAlert

const BOARD = [
  alert('VP8PJ', 'Falkland Islands'),
  alert('3Y0J', 'Bouvet'),
  alert('K9GRD', 'United States', { grid: 'EM79' }),
  alert('W1AW', 'United States'),
  alert('JA1XYZ', 'Japan', { grid: 'PM95' }),
]

const panel = () =>
  render(<NeededPanel alerts={BOARD} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} />)
const ZS8Z = alert('ZS8Z', 'Marion Island', { tags: ['Wanted', 'NewEntity'], priority: 120 })
/** The calls the board shows, top to bottom. */
const shown = () =>
  [...document.querySelectorAll('[role="row"]')]
    .map((r) => [...BOARD, ZS8Z].find((a) => within(r as HTMLElement).queryByText(a.call))?.call)
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

describe('the Watch list chip keeps the rows the watch list names', () => {
  it('FIX: by call or prefix, by DXCC entity and by grid — and nothing else', () => {
    saveWatchlist([newWatchFilter('call', 'VP8*'), newWatchFilter('dxcc', 'Bouvet'), newWatchFilter('grid', 'EM7*')])
    panel()
    // Positive control: every row is on the board before the chip.
    expect(shown().sort()).toEqual(BOARD.map((a) => a.call).sort())
    pickWatchChip()
    expect(shown().sort()).toEqual(['3Y0J', 'K9GRD', 'VP8PJ'])
  })

  it('FIX: follows an edit of the watch list without a reload', () => {
    saveWatchlist([newWatchFilter('call', 'W1AW')])
    panel()
    pickWatchChip()
    expect(shown()).toEqual(['W1AW'])
    // The Settings manager saves and announces every edit; the board re-reads the list on it.
    act(() => {
      saveWatchlist([newWatchFilter('call', 'JA1*')])
      window.dispatchEvent(new Event('nexus:watchlist-changed'))
    })
    expect(shown()).toEqual(['JA1XYZ'])
  })

  it('an empty watch list keeps no row (the chip never falls back to showing everything)', () => {
    panel()
    pickWatchChip()
    expect(shown()).toEqual([])
    // …while the board itself still holds them: clearing the chip brings every row back.
    fireEvent.click(screen.getByRole('button', { name: t('needed.filter.all') }))
    expect(shown()).toHaveLength(BOARD.length)
  })

  it('a row an older station tagged Wanted still matches (a Remote browser watching it)', () => {
    render(
      <NeededPanel
        alerts={[...BOARD, ZS8Z]}
        bandPlan={[]}
        selectedCall={null}
        onQsy={() => {}}
        onSelect={() => {}}
      />,
    )
    pickWatchChip()
    expect(shown()).toEqual(['ZS8Z'])
  })
})

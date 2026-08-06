// @vitest-environment jsdom
//
// ★ FAVORITES IS THE COCKPIT STRIP, WRITTEN DOWN — so it is a FLAT, RANK-NUMBERED list
// and its ▲▼ move a RANK.
//
// Two things made the ★ view unable to answer "what is on my strip, and in what order":
//
//   1. ▲▼ were gated to `sel === 'all'`, so under ★ there was no reorder control at all
//      — and the verb behind them (`moveMemory`) swaps MASTER-ARRAY neighbours. Under ★
//      the rows shown are the master array FILTERED, so two visually adjacent rows can be
//      20 slots apart: one press swaps a star with a non-favorite and nothing visible
//      moves. `moveFavorite` swaps the star with the previous/next STAR instead.
//   2. The list was band-sectioned (HF / VHF-UHF) and sortable. Both re-order the rows
//      away from the master order the strip and Ctrl+1..9 actually read, so the list
//      showed a truthful set in a misleading order. Under ★ the order IS the meaning:
//      row n is chip n.
//
// The rank numbers are what makes the cap legible — 1..STRIP_FAVORITE_LIMIT are the chips
// the cockpit header shows, the rest are starred but off the strip.

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { MemoriesView } from './MemoriesView'
import {
  addMemory,
  emptyBank,
  memoriesStore,
  STRIP_FAVORITE_LIMIT,
  type MemoriesBank,
} from '../features/memories'

/** Favorites F1…Fn — every other one on HF, so the band sectioning this view applies
 *  elsewhere would visibly fire here — with a non-favorite wedged between each pair, so
 *  "adjacent star" and "adjacent row" are never the same thing. */
function bankWith(favorites: number): MemoriesBank {
  let bank = emptyBank()
  for (let i = 1; i <= favorites; i++) {
    const hf = i % 2 === 1
    bank = addMemory(bank, {
      rxMhz: hf ? 7 + i * 0.01 : 145 + i * 0.01,
      mode: hf ? 'LSB' : 'FM',
      name: `F${i}`,
      favorite: true,
    })
    bank = addMemory(bank, { rxMhz: 440 + i * 0.01, mode: 'FM', name: `x${i}`, favorite: false })
  }
  return bank
}

const view = () => (
  <MemoriesView dialMhz={146.52} dialMode="FM" onRecall={() => {}} myGrid="EN52" />
)

const openFavorites = () => fireEvent.click(screen.getByText('★ Favorites'))
const rowNames = (c: HTMLElement) =>
  Array.from(c.querySelectorAll('.mv-row-name')).map((el) => el.textContent)
const ranks = (c: HTMLElement) =>
  Array.from(c.querySelectorAll('.mv-rank')).map((el) => el.textContent)

describe('the ★ view is the strip written down', () => {
  beforeEach(() => memoriesStore.set(bankWith(12)))
  afterEach(cleanup)

  it('is flat and rank-numbered, where All is band-sectioned', () => {
    const { container } = render(view())
    // The fixture spans both bands, so All really does section — without this the
    // "no sections" assertion below would pass on an empty premise.
    expect(container.querySelectorAll('.mv-section').length).toBeGreaterThan(0)

    openFavorites()
    expect(
      container.querySelectorAll('.mv-section'),
      'The ★ list is band-sectioned, so the rows are grouped in an order the cockpit ' +
        'strip does not use. Under ★ the order IS the meaning: row n is chip n.',
    ).toHaveLength(0)
    expect(rowNames(container)).toEqual(Array.from({ length: 12 }, (_, i) => `F${i + 1}`))
    expect(ranks(container)).toEqual(Array.from({ length: 12 }, (_, i) => String(i + 1)))
  })

  it('marks the ranks past the cockpit strip’s cap', () => {
    const { container } = render(view())
    openFavorites()
    const off = Array.from(container.querySelectorAll('.mv-rank.off')).map((el) => el.textContent)
    expect(off).toEqual(
      Array.from({ length: 12 - STRIP_FAVORITE_LIMIT }, (_, i) =>
        String(STRIP_FAVORITE_LIMIT + i + 1),
      ),
    )
  })

  it('keeps master order under a Grid sort — the strip does not honor it', () => {
    const { container } = render(view())
    openFavorites()
    fireEvent.click(screen.getByTitle(/Grid view/))
    fireEvent.click(screen.getByRole('button', { name: /^Name/ }))
    fireEvent.click(screen.getByTitle(/List view/))
    // A name sort would put F10/F11/F12 second through fourth.
    expect(rowNames(container)).toEqual(Array.from({ length: 12 }, (_, i) => `F${i + 1}`))
  })

  it('▲ moves a rank in one press, past whatever non-favorites sit between', () => {
    const { container } = render(view())
    openFavorites()
    const row = container.querySelectorAll('.mv-row')[1] as HTMLElement
    fireEvent.click(within(row).getByLabelText('Move F2 up'))
    expect(
      rowNames(container).slice(0, 3),
      'Under ★, ▲ must swap with the previous STAR. moveMemory swaps master-array ' +
        'neighbours — here that is a non-favorite, and the visible list does not change.',
    ).toEqual(['F2', 'F1', 'F3'])
    expect(ranks(container).slice(0, 3)).toEqual(['1', '2', '3'])
  })

  it('▲▼ are ungated under ★ (every row can be reordered, search or not)', () => {
    const { container } = render(view())
    openFavorites()
    expect(container.querySelectorAll('.mv-row-move')).toHaveLength(12)
    fireEvent.change(container.querySelector('.mv-search') as HTMLInputElement, {
      target: { value: 'F1' },
    })
    expect(container.querySelectorAll('.mv-row-move').length).toBe(
      container.querySelectorAll('.mv-row').length,
    )
  })
})

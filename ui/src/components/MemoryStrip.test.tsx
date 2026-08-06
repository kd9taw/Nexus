// @vitest-environment jsdom
//
// THE COCKPIT MEM STRIP HAS A CEILING ON HOW MANY CHIPS IT SHOWS, AND ＋ ALWAYS
// PUTS ITS CHIP UNDER THAT CEILING.
//
// The strip is one non-wrapping row (that IS the header-growth bound — see the
// component header and mem-strip-clip.test.tsx). One row plus an unbounded favorite
// count means the surplus lives off the right edge behind a scrollbar nobody scrolls:
// present in the DOM, unreadable in the header, and past Ctrl+1..9 unreachable by
// keyboard. So the strip renders the FIRST `STRIP_FAVORITE_LIMIT` favorites in master
// order — the same order `hotkeyRecallTarget` counts — and reports the rest as a count
// on ≡, which is the button that opens the place they can be reordered.
//
// The invariant that makes the cap safe is MemoryStrip's own (header, "always-visible"):
// pressing ＋ must always produce a visible chip. Appended, a new star with a full strip
// would be saved and invisible — the silent "＋ did nothing" the star-the-existing branch
// was written to prevent, back in a new disguise. It lands at rank 1 instead.

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { MemoryStrip } from './MemoryStrip'
import {
  addMemory,
  emptyBank,
  memoriesStore,
  STRIP_FAVORITE_LIMIT,
  type MemoriesBank,
} from '../features/memories'

/** A bank whose favorites are named F1…Fn, with a non-favorite between each pair so
 *  the assertions read master order, not "the first n rows". */
function bankWith(favorites: number): MemoriesBank {
  let bank = emptyBank()
  for (let i = 1; i <= favorites; i++) {
    bank = addMemory(bank, { rxMhz: 145 + i * 0.01, mode: 'FM', name: `F${i}`, favorite: true })
    bank = addMemory(bank, { rxMhz: 440 + i * 0.01, mode: 'FM', name: `x${i}`, favorite: false })
  }
  return bank
}

const chipNames = (root: HTMLElement): string[] =>
  Array.from(root.querySelectorAll('.mem-chip')).map((el) => el.textContent ?? '')

const manage = (root: HTMLElement): HTMLElement =>
  root.querySelector('.mem-strip-manage') as HTMLElement

const strip = (over: Partial<{ dialMhz: number }> = {}) => (
  <MemoryStrip dialMhz={over.dialMhz ?? 146.52} mode="FM" onRecall={() => {}} onManage={() => {}} />
)

describe('the strip shows a bounded number of chips', () => {
  beforeEach(() => memoriesStore.set(emptyBank()))
  afterEach(cleanup)

  it(`renders the first ${STRIP_FAVORITE_LIMIT} favorites in master order and no more`, () => {
    memoriesStore.set(bankWith(STRIP_FAVORITE_LIMIT + 4))
    const { container } = render(strip())
    expect(
      chipNames(container),
      'The strip renders one chip per favorite without limit. Past ~10 the surplus sits ' +
        'off the right edge of a one-row strip — saved, unreadable, and (past Ctrl+1..9) ' +
        'unreachable. Show the first STRIP_FAVORITE_LIMIT in master order.',
    ).toEqual(Array.from({ length: STRIP_FAVORITE_LIMIT }, (_, i) => `F${i + 1}`))
  })

  it('counts the favorites past the cap on ≡ — the button that opens where they live', () => {
    memoriesStore.set(bankWith(STRIP_FAVORITE_LIMIT + 4))
    const { container } = render(strip())
    expect(
      manage(container).textContent,
      'With favorites hidden by the cap the strip says nothing about them, so they read ' +
        'as lost. The count belongs on ≡ (a popover in a cockpit header is not the fix).',
    ).toContain('4')
    expect(manage(container).title).toMatch(/4 more/)
  })

  it('says nothing extra when every favorite fits', () => {
    memoriesStore.set(bankWith(STRIP_FAVORITE_LIMIT))
    const { container } = render(strip())
    expect(chipNames(container)).toHaveLength(STRIP_FAVORITE_LIMIT)
    expect(manage(container).textContent?.trim()).toBe('≡')
  })
})

describe('＋ always produces a chip the operator can see', () => {
  beforeEach(() => memoriesStore.set(emptyBank()))
  afterEach(cleanup)

  it('surfaces the new favorite at rank 1 even with the strip already full', () => {
    memoriesStore.set(bankWith(STRIP_FAVORITE_LIMIT))
    const { container } = render(strip({ dialMhz: 146.52 }))
    fireEvent.click(container.querySelector('.mem-strip-save') as HTMLElement)
    expect(
      chipNames(container)[0],
      'MemoryStrip’s own always-visible invariant: ＋ never silently does nothing. ' +
        'Appended, the new star falls past the cap and no chip appears.',
    ).toBe('146.520 FM')
    expect(chipNames(container)).toHaveLength(STRIP_FAVORITE_LIMIT)
  })

  it('stars an equivalent non-favorite at rank 1 too', () => {
    let bank = bankWith(STRIP_FAVORITE_LIMIT)
    bank = addMemory(bank, { rxMhz: 146.52, mode: 'FM', name: 'Calling', favorite: false })
    memoriesStore.set(bank)
    const { container } = render(strip({ dialMhz: 146.52 }))
    fireEvent.click(container.querySelector('.mem-strip-save') as HTMLElement)
    expect(chipNames(container)[0]).toBe('Calling')
  })
})

// @vitest-environment jsdom
//
// KEEP THE HUNTED ROW MARKED — the POTA/SOTA board (tester request, 2026-09-20).
//
// "Would it be possible to keep the last one you pressed HUNT on highlighted until I click a
// different one? When I scroll up and down it is easy to lose where you are in the list."
//
// The board already knows the hunt target — it prints it in the banner — but the rows never
// consulted it. So the mark simply FOLLOWS `snap.hunt`: clearing the target un-marks the row,
// and there is no second copy of the truth to drift.
//
// ⚠️ THE MARK IS MATCHED ON THE PAIR, call AND reference. One activator legitimately works two
// references at once (a two-fer) and appears twice on the board; a call-only match would mark
// both rows and point at the wrong park. Every test here asserts the PAIR — one row carries the
// mark AND the other does not — because "a row has the class" is inert on its own.
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot, OtaSpot } from '../types'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async (_program: string, _cached?: boolean): Promise<OtaSpot[]> => []),
  getActivation: vi.fn(async () => ({ program: null, reference: null, qsoCount: 0 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
  selfSpot: vi.fn(),
}))

import { PotaSotaView } from './PotaSotaView'

const spot = (activator: string, reference: string, over: Partial<OtaSpot> = {}): OtaSpot => ({
  program: 'POTA',
  reference,
  name: 'Test park',
  activator,
  freqKhz: 14_285,
  mode: 'SSB',
  spotter: null,
  comment: null,
  grid: null,
  newPark: false,
  bandOpen: false,
  huntedToday: false,
  ...over,
})

/** A snapshot carrying `hunt`, and NO DEFAULT PARAMETER for it.
 *
 *  A JS default fires on an explicit `undefined`, so `snap()` with a defaulted `hunt` would have
 *  let the "no hunt target" control silently render a hunt target — the control would pass
 *  whatever the code did. The absent case passes `null`, which is what a snapshot with no hunt
 *  target actually carries. */
const snap = (
  hunt: { program: string; reference: string; call: string } | null,
  logTick = 1,
): AppSnapshot => ({ hunt, radio: { dialMhz: 14.285 }, logTick }) as unknown as AppSnapshot

const huntFor = (call: string, reference: string) => ({ program: 'POTA', reference, call })

/** The row for a reference, by the reference the row PRINTS — throws rather than returning
 *  undefined, so a lookup that finds nothing fails loudly instead of asserting about nothing. */
const row = (reference: string): HTMLElement => {
  const found = Array.from(document.querySelectorAll('li.pota-spot')).find(
    (li) => li.querySelector('.pota-spot-ref')?.textContent === reference,
  )
  if (!found) throw new Error(`no row on the board for ${reference}`)
  return found as HTMLElement
}
const marked = (): string[] =>
  Array.from(document.querySelectorAll('li.pota-spot.selected')).map(
    (li) => li.querySelector('.pota-spot-ref')?.textContent ?? '?',
  )

// jsdom has no Element.scrollIntoView at all — install one so the reveal is observable, and
// take it away again so no other file inherits a scrollable jsdom.
const siv = vi.fn()
beforeAll(() => {
  ;(Element.prototype as unknown as { scrollIntoView: typeof siv }).scrollIntoView = siv
})
afterAll(() => {
  delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView
})

beforeEach(() => {
  api.getOtaSpots.mockReset()
  siv.mockClear()
})
afterEach(() => {
  cleanup()
  localStorage.clear()
})

describe('The hunted row stays marked', () => {
  it('marks the hunted row and ONLY it — the two-fer pair, not the callsign', async () => {
    // The same activator on two references at once. A call-only match marks both.
    api.getOtaSpots.mockResolvedValue([
      spot('K1ABC', 'US-0001'),
      spot('K1ABC', 'US-0002', { freqKhz: 7_185 }),
      spot('W9XYZ', 'US-0003', { freqKhz: 10_115 }),
    ])
    render(<PotaSotaView snap={snap(huntFor('K1ABC', 'US-0002'))} />)
    await screen.findByText('US-0002')

    expect(row('US-0002').className, 'the hunted park is marked').toContain('selected')
    expect(
      row('US-0001').className,
      'the SAME activator at a different park must not be marked',
    ).not.toContain('selected')
    expect(row('US-0003').className).not.toContain('selected')
    // And exactly one row, so nothing marks the board wholesale.
    expect(marked()).toEqual(['US-0002'])
  })

  it('marks nothing when there is no hunt target', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    render(<PotaSotaView snap={snap(null)} />)
    await screen.findByText('US-0001')

    expect(marked()).toEqual([])
  })

  it('marks nothing when the hunt target is not on the board', async () => {
    // Set from the map or the Needed panel, at a park this board is not showing. A call-only
    // match would mark K1ABC's row here and send the operator to the wrong park.
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    render(<PotaSotaView snap={snap(huntFor('K1ABC', 'US-9999'))} />)
    await screen.findByText('US-0001')

    expect(marked()).toEqual([])
  })

  it('reads the mark to a screen reader, not by colour alone', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    render(<PotaSotaView snap={snap(huntFor('W9XYZ', 'US-0002'))} />)
    await screen.findByText('US-0002')

    // aria-current, not aria-selected: these rows are `listitem`, a role that does not support
    // aria-selected (ARIA 1.2 allows it on gridcell/option/row/tab/treeitem), so a reader would
    // ignore it. aria-current is global and reads as "current item".
    expect(row('US-0002').getAttribute('aria-current')).toBe('true')
    expect(row('US-0001').getAttribute('aria-current')).toBeNull()
  })

  it('FOLLOWS the hunt target: clearing it un-marks the row', async () => {
    // The mark is a render of `snap.hunt`, not a second copy of it. Clear hunt target — and
    // logging the QSO, which consumes it — therefore un-mark the row with no extra wiring.
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    const view = render(<PotaSotaView snap={snap(huntFor('K1ABC', 'US-0001'))} />)
    await screen.findByText('US-0001')
    expect(marked()).toEqual(['US-0001'])

    view.rerender(<PotaSotaView snap={snap(null)} />)
    expect(marked(), 'a latched copy of the target would keep the row marked').toEqual([])
  })

  it('moves the mark to the row hunted next', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    const view = render(<PotaSotaView snap={snap(huntFor('K1ABC', 'US-0001'))} />)
    await screen.findByText('US-0001')

    view.rerender(<PotaSotaView snap={snap(huntFor('W9XYZ', 'US-0002'))} />)
    expect(marked(), 'exactly one row, and the new one').toEqual(['US-0002'])
  })

  it('scrolls the hunted row into view once, and not again when the feed re-delivers it', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    const hunt = huntFor('W9XYZ', 'US-0002')
    const view = render(<PotaSotaView snap={snap(hunt, 1)} />)
    await screen.findByText('US-0002')

    await waitFor(() => expect(siv).toHaveBeenCalledTimes(1))
    expect(siv.mock.instances[0]).toBe(row('US-0002'))
    // 'nearest' so a row already on screen moves nothing — the operator's own scrolling is not
    // fought, and a hunt clicked on a visible row does not jump the list.
    expect(siv.mock.calls[0][0]).toEqual({ block: 'nearest' })

    // A log change re-reads the cache and hands back FRESH spot objects with the same values —
    // the 60 s poll's shape. Keying the reveal on object identity would re-scroll here, under
    // the operator's hands, every time they log anything.
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    view.rerender(<PotaSotaView snap={snap({ ...hunt }, 2)} />)
    await waitFor(() => expect(api.getOtaSpots).toHaveBeenCalledWith('POTA', true))
    expect(siv, 'the same row re-delivered must not move the list again').toHaveBeenCalledTimes(1)
  })

  // POSITIVE CONTROL for the silence above: the reveal is not simply dead. A DIFFERENT target
  // is a new place to be, and it does scroll.
  it('scrolls again when the operator hunts a different row', async () => {
    api.getOtaSpots.mockResolvedValue([spot('K1ABC', 'US-0001'), spot('W9XYZ', 'US-0002')])
    const view = render(<PotaSotaView snap={snap(huntFor('W9XYZ', 'US-0002'))} />)
    await screen.findByText('US-0002')
    await waitFor(() => expect(siv).toHaveBeenCalledTimes(1))

    view.rerender(<PotaSotaView snap={snap(huntFor('K1ABC', 'US-0001'))} />)
    await waitFor(() => expect(siv).toHaveBeenCalledTimes(2))
    expect(siv.mock.instances[1]).toBe(row('US-0001'))
  })
})

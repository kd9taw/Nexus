// @vitest-environment jsdom
//
// SELECT AND DELETE IN BULK — and the one rule that keeps it from losing somebody's channels.
//
// The operator asked for "an easy way to select and delete memory entries"; deletion was one
// row at a time. Bulk deletion of a HAND-BUILT list is the most destructive thing this section
// can do, so two things are pinned here:
//
//   1. THE TARGET IS WHAT THE OPERATOR CAN SEE. Selection survives search, the ★/Nets/group
//      filters, sort and the HF / VHF-UHF sectioning — it is a set of ids, not of rows — but
//      Delete only ever removes the selected rows that are ON SCREEN, and the count on the
//      button is that same number. Deleting a row the operator narrowed away and can no
//      longer see is the failure mode this exists to make impossible.
//   2. IT IS UNDOABLE. The bank is an immutable value, so putting the rows back at the index
//      they came from is a few lines — and the index IS the ★ rank order.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoriesView } from './MemoriesView'
import { ConfirmHost } from '../confirm'
import { addMemory, emptyBank, memoriesStore, type MemoriesBank } from '../features/memories'

const toast = vi.hoisted(() => ({ push: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: toast.push }))

const view = () => (
  <>
    <MemoriesView dialMhz={146.52} dialMode="FM" onRecall={() => {}} myGrid="EN52" />
    <ConfirmHost />
  </>
)

/** Four channels spanning both band sections, so the sectioning is really in play. */
function seedFour(): MemoriesBank {
  let b = emptyBank()
  b = addMemory(b, { rxMhz: 7.185, mode: 'LSB', name: 'Alpha net' })
  b = addMemory(b, { rxMhz: 7.2, mode: 'LSB', name: 'Bravo net' })
  b = addMemory(b, { rxMhz: 146.52, mode: 'FM', name: 'Charlie rptr' })
  b = addMemory(b, { rxMhz: 147.0, mode: 'FM', name: 'Delta rptr' })
  return b
}

const names = () =>
  Array.from(document.querySelectorAll('.mv-row-name')).map((el) => el.textContent)
const pick = (name: string) => fireEvent.click(screen.getByLabelText(`Select ${name}`))
const search = (q: string) =>
  fireEvent.change(screen.getByPlaceholderText('Search all…'), { target: { value: q } })

/** Press Delete and answer the confirmation yes. */
async function deleteSelected(count: number) {
  const noun = count === 1 ? 'memory' : 'memories'
  fireEvent.click(screen.getByRole('button', { name: `Delete ${count}` }))
  await waitFor(() => expect(screen.getByText(`Delete ${count} ${noun}?`)).toBeTruthy())
  fireEvent.click(screen.getByRole('button', { name: `Delete ${count} ${noun}` }))
  await waitFor(() => expect(screen.queryByText(`Delete ${count} ${noun}?`)).toBeNull())
}

beforeEach(() => {
  toast.push.mockClear()
  memoriesStore.set(seedFour())
})
afterEach(cleanup)

describe('selecting rows and deleting them together', () => {
  it('counts what is selected and deletes exactly that', async () => {
    render(view())
    pick('Alpha net')
    pick('Charlie rptr')
    expect(screen.getByText('2 selected')).toBeTruthy()

    await deleteSelected(2)
    expect(names()).toEqual(['Bravo net', 'Delta rptr'])
  })

  it('asks first — the dialog names the count, and No keeps every row', async () => {
    render(view())
    pick('Alpha net')
    pick('Bravo net')
    fireEvent.click(screen.getByRole('button', { name: 'Delete 2' }))
    await waitFor(() => expect(screen.getByText('Delete 2 memories?')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByText('Delete 2 memories?')).toBeNull())
    expect(memoriesStore.get().memories).toHaveLength(4)
  })

  it('selects every row on screen at once', async () => {
    render(view())
    pick('Alpha net')
    fireEvent.click(screen.getByLabelText('Select all shown'))
    expect(screen.getByText('4 selected')).toBeTruthy()
    await deleteSelected(4)
    expect(memoriesStore.get().memories).toHaveLength(0)
  })
})

describe('a narrowed list can only delete what it shows', () => {
  it('drops the rows the operator can no longer see from the target', async () => {
    render(view())
    pick('Alpha net')
    pick('Bravo net')
    pick('Charlie rptr')
    expect(screen.getByText('3 selected')).toBeTruthy()

    search('alpha')
    expect(names()).toEqual(['Alpha net'])
    // The count names BOTH numbers once part of the selection is out of view. Bare
    // "1 selected" would be the bar disagreeing with the three rows actually ticked.
    expect(
      screen.getByText('1 selected · 2 not in view'),
      'the count still promises rows the operator cannot see',
    ).toBeTruthy()

    await deleteSelected(1)
    expect(
      memoriesStore.get().memories.map((m) => m.name),
      'a selected-but-hidden row was deleted — the memory-losing bug',
    ).toEqual(['Bravo net', 'Charlie rptr', 'Delta rptr'])
  })

  it('keeps the hidden rows selected, so widening the search brings them back', async () => {
    render(view())
    pick('Alpha net')
    pick('Bravo net')
    search('bravo')
    expect(screen.getByText('1 selected · 1 not in view')).toBeTruthy()
    search('')
    expect(screen.getByText('2 selected')).toBeTruthy()
  })
})

describe('the delete can be taken back', () => {
  it('puts the rows back where they were', async () => {
    render(view())
    pick('Bravo net')
    pick('Charlie rptr')
    await deleteSelected(2)
    expect(names()).toEqual(['Alpha net', 'Delta rptr'])

    const calls = toast.push.mock.calls
    const undo = calls[calls.length - 1][3] as { action: () => void; actionLabel: string }
    expect(undo.actionLabel).toBe('Undo')
    act(() => undo.action())
    expect(names(), 'restored, but not to the position they were deleted from').toEqual([
      'Alpha net',
      'Bravo net',
      'Charlie rptr',
      'Delta rptr',
    ])
  })
})

describe('deleting from ★ Favorites re-ranks the cockpit strip', () => {
  it('closes the ranks up, in order', async () => {
    let b = emptyBank()
    for (const n of ['F1', 'F2', 'F3', 'F4']) {
      b = addMemory(b, { rxMhz: 146.5 + n.charCodeAt(1) / 1000, mode: 'FM', name: n, favorite: true })
    }
    memoriesStore.set(b)
    render(view())
    fireEvent.click(screen.getByText('★ Favorites'))
    pick('F2')
    await deleteSelected(1)

    expect(names()).toEqual(['F1', 'F3', 'F4'])
    expect(
      Array.from(document.querySelectorAll('.mv-rank')).map((el) => el.textContent),
      'the ranks must close up — rank n IS cockpit chip n and Ctrl+n',
    ).toEqual(['1', '2', '3'])
  })
})

// ---------------------------------------------------------------------------
// THE BAR FOLLOWS THE SELECTION, NOT THE VISIBLE PART OF IT. Gating it on the visible
// part made the bar — and Clear with it — vanish at exactly the moment a selection went
// out of view behind a search: rows stayed ticked, nothing on screen said so, and there
// was no way to drop them without widening the filter again.
// ---------------------------------------------------------------------------
describe('a selection narrowed out of view can still be cleared', () => {
  it('keeps the bar and Clear when nothing selected is on screen', () => {
    render(view())
    pick('Alpha net')
    search('charlie')
    expect(names()).toEqual(['Charlie rptr'])
    expect(
      screen.queryByRole('button', { name: 'Clear' }),
      'the whole selection is off screen and Clear went with it',
    ).toBeTruthy()
    expect(screen.getByText('0 selected · 1 not in view')).toBeTruthy()
  })

  it('cannot delete when nothing selected is on screen', () => {
    render(view())
    pick('Alpha net')
    search('charlie')
    const del = screen.getByRole('button', { name: 'Delete 0' }) as HTMLButtonElement
    expect(del.disabled, 'Delete is live with nothing visible to delete').toBe(true)
  })

  it('Clear really drops the hidden rows', () => {
    render(view())
    pick('Alpha net')
    search('charlie')
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    search('')
    expect(document.querySelectorAll('.mv-row.picked')).toHaveLength(0)
  })
})

// ---------------------------------------------------------------------------
// SELECT-ALL IN THE SPREADSHEET'S OWN HEADER. Without it "delete this whole group" cost
// a manual tick first, because the selection bar only exists once something is selected.
// ---------------------------------------------------------------------------
describe('the grid can select every shown row from its header', () => {
  const toGrid = () =>
    fireEvent.click(screen.getByTitle('Grid view — the CHIRP-style spreadsheet'))

  it('ticks every shown row with nothing selected first', () => {
    render(view())
    toGrid()
    const all = document.querySelector('.mv-grid thead input[type="checkbox"]') as HTMLInputElement
    expect(all, 'no select-all in the grid header').toBeTruthy()
    fireEvent.click(all)
    expect(screen.getByText('4 selected')).toBeTruthy()
  })

  it('only takes what the filter shows', () => {
    render(view())
    toGrid()
    search('net')
    fireEvent.click(document.querySelector('.mv-grid thead input[type="checkbox"]')!)
    expect(screen.getByText('2 selected')).toBeTruthy()
  })
})

// ---------------------------------------------------------------------------
// THE ROW ✕ IS UNDOABLE TOO. It sits one button away from ✎, so it is the delete most
// easily hit by accident — and it was the only one with nothing behind it, while the
// bulk delete had both a confirm and an undo. It stays one click; the Undo is what
// makes that safe.
// ---------------------------------------------------------------------------
describe('deleting a single row can be taken back', () => {
  const deleteRow = (name: string) => {
    const row = Array.from(document.querySelectorAll('.mv-row')).find((li) =>
      li.querySelector('.mv-row-name')?.textContent?.includes(name),
    )!
    fireEvent.click(row.querySelector('.mv-row-del')!)
  }

  it('offers an Undo that puts the row back where it was', () => {
    render(view())
    deleteRow('Bravo net')
    expect(names()).toEqual(['Alpha net', 'Charlie rptr', 'Delta rptr'])

    const call = toast.push.mock.calls[toast.push.mock.calls.length - 1]
    const undo = call[3] as { action: () => void; actionLabel: string } | undefined
    expect(undo?.actionLabel, 'the row ✕ deleted with no way back').toBe('Undo')
    act(() => undo!.action())
    expect(names(), 'restored, but not to the position it was deleted from').toEqual([
      'Alpha net',
      'Bravo net',
      'Charlie rptr',
      'Delta rptr',
    ])
  })

  it('does not ask first — a single channel still goes in one click', () => {
    render(view())
    deleteRow('Delta rptr')
    expect(screen.queryByText(/Delete 1 memory\?/)).toBeNull()
    expect(names()).toEqual(['Alpha net', 'Bravo net', 'Charlie rptr'])
  })
})

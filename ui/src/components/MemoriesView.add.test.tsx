// @vitest-environment jsdom
//
// ＋ New OPENS A PINNED ADD PANEL — not an editor somewhere in the list.
//
// The operator's report: "the add section adds inline vs a more dedicated fixed section at the
// bottom to add entries, leading to difficulty and confusion". ＋ New created the row and
// opened the inline editor ON it, so the thing being created sat wherever the sort, the band
// sectioning and the filters happened to put it — in a bank of 200 channels, off screen.
//
// The row is still created IMMEDIATELY (a crash or a closed window cannot lose it, and every
// filter-matching behaviour ＋ New already had depends on the real row existing). What changed
// is where its editor lives: a pinned panel at the bottom of the pane, which is visibly THE
// place you are adding.
//
// ⚠️ The earlier fix this must not undo: ＋ New clears the search box. A new memory has an
// empty name, so any active query filtered it straight back out and the button looked dead
// ("I cannot hit the add memories button").
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoriesView } from './MemoriesView'
import { addMemory, emptyBank, memoriesStore } from '../features/memories'

const view = () => (
  <MemoriesView dialMhz={146.52} dialMode="FM" onRecall={() => {}} myGrid="EN52" />
)

const clickNew = () => fireEvent.click(screen.getByTitle('Add a memory by hand'))
const names = () =>
  Array.from(document.querySelectorAll('.mv-row-name')).map((el) => el.textContent)

beforeEach(() => {
  memoriesStore.set(addMemory(emptyBank(), { rxMhz: 7.185, mode: 'LSB', name: 'Old timer net' }))
})
afterEach(cleanup)

describe('the add panel is pinned, not lost in the list', () => {
  it('opens a dedicated panel and puts the editor in it', () => {
    const { container } = render(view())
    clickNew()
    const panel = container.querySelector('.mv-add')
    expect(panel, '＋ New did not open a pinned add panel').toBeTruthy()
    expect(panel!.querySelector('.mv-editor'), 'the panel has no editor in it').toBeTruthy()
    expect(
      container.querySelector('.mv-list .mv-editor'),
      'the editor is still opening inline among the rows — the reported confusion',
    ).toBeNull()
  })

  it('creates the row immediately, so nothing can lose it', () => {
    render(view())
    clickNew()
    expect(memoriesStore.get().memories).toHaveLength(2)
    expect(names()).toContain('146.520 FM')
  })

  it('still clears the search box, or the button looks dead again', () => {
    render(view())
    const search = screen.getByPlaceholderText('Search all…') as HTMLInputElement
    fireEvent.change(search, { target: { value: 'zzz' } })
    clickNew()
    expect(search.value).toBe('')
    expect(document.querySelector('.mv-add')).toBeTruthy()
  })

  it('still matches the view it was pressed in', () => {
    render(view())
    fireEvent.click(screen.getByText('★ Favorites'))
    clickNew()
    const made = memoriesStore.get().memories.find((m) => m.name === '146.520 FM')!
    expect(made.favorite, 'a memory added under ★ Favorites must be starred to be visible').toBe(
      true,
    )
    expect(document.querySelector('.mv-add')).toBeTruthy()
  })
})

describe('the add panel takes the keyboard the way a run of entries needs', () => {
  it('Escape backs out and KEEPS the channel — it is already a real one', () => {
    const { container } = render(view())
    clickNew()
    fireEvent.keyDown(container.querySelector('.mv-add')!, { key: 'Escape' })
    expect(container.querySelector('.mv-add')).toBeNull()
    expect(memoriesStore.get().memories).toHaveLength(2)
  })

  it('Enter commits the field and closes the panel', () => {
    const { container } = render(view())
    clickNew()
    const name = screen.getByLabelText('Name') as HTMLInputElement
    name.focus()
    fireEvent.change(name, { target: { value: 'W9ABC repeater' } })
    fireEvent.keyDown(name, { key: 'Enter' })
    expect(memoriesStore.get().memories.map((m) => m.name)).toContain('W9ABC repeater')
    expect(container.querySelector('.mv-add')).toBeNull()
  })

  it('Discard removes the half-filled channel outright', () => {
    const { container } = render(view())
    clickNew()
    fireEvent.click(screen.getByTitle('Delete this new memory and close the panel'))
    expect(container.querySelector('.mv-add')).toBeNull()
    expect(memoriesStore.get().memories).toHaveLength(1)
    expect(names()).toEqual(['Old timer net'])
  })
})

// ---------------------------------------------------------------------------
// ESCAPE BELONGS TO THE FIELD FIRST. The panel closes on Escape, which is right — but
// Escape already means something two layers down: "revert this field" in CommitInput,
// "drop back to the list" in the mode picker's free-text escape. Both bubbled, so an
// operator who mistyped a repeater name and pressed Escape — the way every other field
// in this app trains them to — lost the whole form they were filling in.
// ---------------------------------------------------------------------------
describe('Escape reverts the field before it closes the panel', () => {
  it('keeps the panel open when Escape reverts a text field', () => {
    const { container } = render(view())
    clickNew()
    const name = container.querySelector('.mv-add input') as HTMLInputElement
    fireEvent.change(name, { target: { value: 'Typo Ridge' } })
    fireEvent.keyDown(name, { key: 'Escape' })
    expect(
      container.querySelector('.mv-add'),
      'a field-level Escape closed the whole add panel',
    ).toBeTruthy()
  })

  it('keeps the panel open when Escape leaves the mode picker free-text', () => {
    const { container } = render(view())
    clickNew()
    const mode = container.querySelector('.mv-add select[aria-label="Mode"]') as HTMLSelectElement
    fireEvent.change(mode, { target: { value: '__other__' } })
    const free = container.querySelector('.mv-add input[aria-label="Mode"]') as HTMLInputElement
    expect(free, 'Other… did not open a free-text field').toBeTruthy()
    fireEvent.keyDown(free, { key: 'Escape' })
    expect(
      container.querySelector('.mv-add'),
      'Escaping the picker closed the panel — its documented "drop back to the list" is unreachable',
    ).toBeTruthy()
  })

  it('still closes on Escape from the panel itself', () => {
    const { container } = render(view())
    clickNew()
    fireEvent.keyDown(container.querySelector('.mv-add')!, { key: 'Escape' })
    expect(container.querySelector('.mv-add')).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// ＋ New TWICE IS NOT TWO CHANNELS. The row is created immediately and survives an
// abandoned panel by design — but that meant every ＋ New / close / ＋ New round trip
// deposited another unnamed row at the dial frequency. Reuse requires the leftover to be
// EXACTLY as created: type one character and ＋ New honestly gives you a second channel,
// because two nets on one frequency are ordinary and ＋ New must not dedupe by frequency.
// ---------------------------------------------------------------------------
describe('an abandoned add is reused, never duplicated', () => {
  const closePanel = (container: HTMLElement) =>
    fireEvent.keyDown(container.querySelector('.mv-add')!, { key: 'Escape' })

  it('does not pile up identical rows when the panel is opened and abandoned', () => {
    const { container } = render(view())
    clickNew()
    closePanel(container)
    clickNew()
    closePanel(container)
    clickNew()
    expect(
      memoriesStore.get().memories.filter((m) => m.rxMhz === 146.52),
      'each abandoned ＋ New left another unnamed channel at the dial',
    ).toHaveLength(1)
  })

  it('gives a real second channel once the first has been typed into', () => {
    const { container } = render(view())
    clickNew()
    const name = container.querySelector('.mv-add input') as HTMLInputElement
    fireEvent.change(name, { target: { value: 'Church Hill' } })
    fireEvent.blur(name)
    closePanel(container)
    clickNew()
    expect(
      memoriesStore.get().memories.filter((m) => m.rxMhz === 146.52),
      'reuse swallowed a channel the operator had already named',
    ).toHaveLength(2)
  })
})

// ---------------------------------------------------------------------------
// ENTER IS "DONE WITH THIS FIELD" — only in a field where that means something. The
// handler tested `tagName === 'INPUT'`, which is also true of the net reminder's
// checkbox and its time picker, so Enter on either shut the panel instead of doing the
// widget's own thing.
// ---------------------------------------------------------------------------
describe('Enter closes the panel only from a text field', () => {
  it('does not close on Enter from the net reminder checkbox', () => {
    const { container } = render(view())
    // The reminder checkbox lives in the net editor, so add under Nets — which is also
    // what makes ＋ New create an hfnet row.
    fireEvent.click(screen.getByText('Nets'))
    clickNew()
    const box = container.querySelector('.mv-add input[type="checkbox"]') as HTMLInputElement
    expect(box, 'no checkbox in the add panel to test').toBeTruthy()
    fireEvent.keyDown(box, { key: 'Enter' })
    expect(
      container.querySelector('.mv-add'),
      'Enter on a checkbox closed the add panel',
    ).toBeTruthy()
  })

  it('still closes on Enter from a text field', () => {
    const { container } = render(view())
    clickNew()
    const name = container.querySelector('.mv-add input') as HTMLInputElement
    fireEvent.keyDown(name, { key: 'Enter' })
    expect(container.querySelector('.mv-add')).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// "WHERE DID IT GO?" — the marker on the row being added. The grid needs it most: it is
// the 200-channel spreadsheet view, where a new row is hardest to find again.
// ---------------------------------------------------------------------------
describe('the row being added is marked in both views', () => {
  it('marks it in the list', () => {
    const { container } = render(view())
    clickNew()
    expect(container.querySelector('.mv-row.adding')).toBeTruthy()
  })

  it('marks it in the grid', () => {
    const { container } = render(view())
    fireEvent.click(screen.getByTitle('Grid view — the CHIRP-style spreadsheet'))
    clickNew()
    expect(
      container.querySelector('.mv-grid tr.adding'),
      'the new row is unmarked in the spreadsheet view, where it is hardest to find',
    ).toBeTruthy()
  })
})

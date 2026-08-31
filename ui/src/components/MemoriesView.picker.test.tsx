// @vitest-environment jsdom
//
// "USB IS THE ONLY MODE I CAN ADD" — the datalist trap, and why the fix cannot be a plain
// <select>.
//
// The mode field offered eight modes through a `<datalist>`. A datalist FILTERS its
// suggestions by what is already in the field, and a new memory is created on the rig's
// current mode — USB, nearly always. So the dropdown contained exactly one entry, the one
// already there, and seven modes were unreachable. The CTCSS field had the identical shape:
// with 103.5 in the box, the 38-tone ladder collapsed to 103.5.
//
// ⚠️ The fix must NOT be a hard `<select>`. Memories round-trip through CHIRP CSV, which
// carries modes and tones our lists have never heard of (DV, P25, a 69.3 Hz tone). A select
// that cannot represent the stored value renders as "nothing selected" and the first touch of
// the row rewrites it — silent data loss in a file the operator keeps. So: every choice always
// visible, an unrecognised value kept as its own choice, and a free-text escape for a new one.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { MemoriesView } from './MemoriesView'
import { addMemory, emptyBank, memoriesStore, type Memory } from '../features/memories'

const view = () => (
  <MemoriesView dialMhz={146.52} dialMode="FM" onRecall={() => {}} myGrid="EN52" />
)

function seed(m: Partial<Memory> & { rxMhz: number; mode: string }) {
  memoriesStore.set(addMemory(emptyBank(), { name: 'Ch1', ...m }))
}

const openEditor = () => fireEvent.click(screen.getAllByTitle('Edit')[0])
const values = (el: HTMLSelectElement) => Array.from(el.options).map((o) => o.value)
const otherValue = (el: HTMLSelectElement) =>
  Array.from(el.options).find((o) => o.textContent === 'Other…')!.value

const EVERY_MODE = ['USB', 'LSB', 'FM', 'NFM', 'AM', 'CW', 'FT8', 'FT4']

afterEach(cleanup)

describe('the mode picker shows every mode, whatever the memory is set to', () => {
  beforeEach(() => seed({ rxMhz: 14.074, mode: 'USB' }))

  it('offers all eight modes on a USB memory — the reported bug', () => {
    render(view())
    openEditor()
    const sel = screen.getByLabelText('Mode') as HTMLSelectElement
    expect(sel.tagName, 'the mode field is still a text input + datalist').toBe('SELECT')
    expect(values(sel)).toEqual(expect.arrayContaining(EVERY_MODE))
    expect(sel.value).toBe('USB')
  })

  it('offers all eight in the CHIRP grid too', () => {
    render(view())
    fireEvent.click(screen.getByTitle('Grid view — the CHIRP-style spreadsheet'))
    const sel = screen.getByLabelText('Mode') as HTMLSelectElement
    expect(sel.tagName).toBe('SELECT')
    expect(values(sel)).toEqual(expect.arrayContaining(EVERY_MODE))
  })

  it('commits the picked mode', () => {
    render(view())
    openEditor()
    fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'CW' } })
    expect(memoriesStore.get().memories[0].mode).toBe('CW')
  })

  it('takes a mode nobody listed, through the free-text escape', () => {
    render(view())
    openEditor()
    const sel = screen.getByLabelText('Mode') as HTMLSelectElement
    fireEvent.change(sel, { target: { value: otherValue(sel) } })
    const typed = screen.getByLabelText('Mode') as HTMLInputElement
    expect(typed.tagName).toBe('INPUT')
    fireEvent.change(typed, { target: { value: 'D-STAR' } })
    fireEvent.blur(typed)
    expect(memoriesStore.get().memories[0].mode).toBe('D-STAR')
  })
})

describe('an imported mode the picker has never heard of survives being edited', () => {
  beforeEach(() => seed({ rxMhz: 145.5, mode: 'DV', name: 'W9XYZ DV' }))

  it('keeps it selected as its own choice', () => {
    render(view())
    openEditor()
    const sel = screen.getByLabelText('Mode') as HTMLSelectElement
    expect(sel.value, 'a hard <select> shows nothing selected for an unknown value').toBe('DV')
    expect(values(sel)).toEqual(expect.arrayContaining(['DV', ...EVERY_MODE]))
  })

  it('does not rewrite it when another field on the row is edited', () => {
    render(view())
    openEditor()
    const name = screen.getByLabelText('Name') as HTMLInputElement
    fireEvent.change(name, { target: { value: 'Renamed' } })
    fireEvent.blur(name)
    expect(memoriesStore.get().memories[0].name).toBe('Renamed')
    expect(memoriesStore.get().memories[0].mode).toBe('DV')
  })
})

describe('the CTCSS ladder has the same trap, and the same fix', () => {
  it('offers the whole ladder while a tone is already set', () => {
    seed({ rxMhz: 146.94, mode: 'FM', kind: 'repeater', toneMode: 'tone', ctcssEncHz: 103.5 })
    render(view())
    openEditor()
    const sel = screen.getByLabelText('CTCSS Hz') as HTMLSelectElement
    expect(sel.tagName, 'the CTCSS field is still a text input + datalist').toBe('SELECT')
    expect(sel.value).toBe('103.5')
    // The ends of the EIA ladder — proof it is the whole list, not the one match.
    expect(values(sel)).toEqual(expect.arrayContaining(['67', '250.3']))
  })

  it('keeps a tone that is not on our ladder', () => {
    seed({ rxMhz: 146.94, mode: 'FM', kind: 'repeater', toneMode: 'tone', ctcssEncHz: 69.3 })
    render(view())
    openEditor()
    const sel = screen.getByLabelText('CTCSS Hz') as HTMLSelectElement
    expect(sel.value).toBe('69.3')
    expect(values(sel)).toContain('69.3')
  })
})

// ---------------------------------------------------------------------------
// THE UNSET ROW IS A PLACEHOLDER, NOT A CHOICE. The tone picker showed a "—" row that
// looked selectable and did nothing: it commits through `withNumber`, and an empty
// string parses to NaN, so the controlled select snapped straight back to the old tone.
// It cannot be made to commit either — "tone mode on, no tone" programs the rig with a
// 0 Hz tone and opens no repeater. Tone Mode = None is how tones are turned off, and it
// hides this field entirely. So the row displays the unset state and is disabled.
// ---------------------------------------------------------------------------
describe('the tone picker never offers a dead choice', () => {
  it('shows the unset row when there is no tone, disabled', () => {
    // Offset/tone fields belong to repeater-class channels — `showOffset` in the editor.
    seed({ rxMhz: 146.94, mode: 'FM', kind: 'repeater', toneMode: 'tone' })
    const { container } = render(view())
    openEditor()
    const tone = container.querySelector('select[aria-label="CTCSS Hz"]') as HTMLSelectElement
    expect(tone, 'no tone picker on a tone-mode channel').toBeTruthy()
    const dash = Array.from(tone.options).find((o) => o.value === '')
    expect(dash, 'the unset state has nothing to display as').toBeTruthy()
    expect(
      dash!.disabled,
      'the "—" row is selectable and does nothing — a dead control',
    ).toBe(true)
  })
})

// @vitest-environment jsdom
//
// #390 — A NOTE ON EACH WATCH-LIST ENTRY (a feature request, 2026-09-29): an operator with a long
// watch list forgets why each call is on it and when it can come off. So every entry can carry a
// note ("Samoa DXp 9/27-10/3"), shown and edited where the list is edited (Settings ▸ Spots &
// Alerts ▸ Watch list): given when the entry is added, or added later to an entry already there.
//
// An edit in place is saved when the field is left, or on Enter — not on every keystroke, because
// every save is announced (`nexus:watchlist-changed`) and App answers it by sending the station its
// copy of the list and reading the Needed board again. Leaving a field unchanged saves nothing.
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { WatchlistPanel } from './WatchlistPanel'
import { t } from '../i18n'
import { loadWatchlist, saveWatchlist, WATCHLIST_CHANGED, type WatchFilter } from '../watchlist'

const SAMOA: WatchFilter = { id: 'w-5w', kind: 'call', value: '5W1SA', cqOnly: true, notes: 'Samoa DXp 9/27-10/3' }
const FALKLANDS: WatchFilter = { id: 'w-vp8', kind: 'call', value: 'VP8*' }
const BOUVET: WatchFilter = { id: 'w-3y', kind: 'dxcc', value: 'Bouvet' }

/** How many times the list announced a change: each one costs App a send to the station. */
let announced = 0
const onChange = () => {
  announced += 1
}
beforeEach(() => {
  localStorage.clear()
  announced = 0
  window.addEventListener(WATCHLIST_CHANGED, onChange)
})
afterEach(() => {
  window.removeEventListener(WATCHLIST_CHANGED, onChange)
  cleanup()
})

const noteOf = (value: string) =>
  screen.getByRole('textbox', { name: t('watchlist.item.notes.aria', { value }) }) as HTMLInputElement
const valueBox = () => screen.getByRole('textbox', { name: t('watchlist.add.value.aria') }) as HTMLInputElement
const newNote = () => screen.getByRole('textbox', { name: t('watchlist.add.notes.aria') }) as HTMLInputElement
const addButton = () => screen.getByRole('button', { name: t('watchlist.add.submit') })

describe('#390 the note on a watch-list entry', () => {
  it('is shown on its entry, where the list is edited', () => {
    saveWatchlist([SAMOA, FALKLANDS])
    render(<WatchlistPanel />)
    expect(noteOf('5W1SA').value).toBe('Samoa DXp 9/27-10/3')
    expect(noteOf('VP8*').value, 'an entry with no note shows an empty field').toBe('')
  })

  it('can be given when the entry is added, and the field clears for the next one', () => {
    render(<WatchlistPanel />)
    fireEvent.change(valueBox(), { target: { value: '5W1SA' } })
    fireEvent.change(newNote(), { target: { value: '  Samoa DXp 9/27-10/3 ' } })
    fireEvent.click(addButton())
    const [added] = loadWatchlist()
    expect(added).toMatchObject({ kind: 'call', value: '5W1SA', notes: 'Samoa DXp 9/27-10/3' })
    expect(newNote().value).toBe('')
    expect(noteOf('5W1SA').value, 'and it shows on the new entry').toBe('Samoa DXp 9/27-10/3')
  })

  it('is left off an entry added without one: no empty note is stored', () => {
    render(<WatchlistPanel />)
    fireEvent.change(valueBox(), { target: { value: '3Y0J' } })
    fireEvent.click(addButton())
    const [added] = loadWatchlist()
    expect(added.value).toBe('3Y0J')
    expect('notes' in added, 'the new entry carries a notes key').toBe(false)
  })

  it('can be added to an entry already on the list, and is saved when the field is left', () => {
    saveWatchlist([FALKLANDS, BOUVET])
    render(<WatchlistPanel />)
    fireEvent.change(noteOf('VP8*'), { target: { value: 'Falklands, until November' } })
    expect(loadWatchlist()[0].notes, 'nothing is saved while typing').toBeUndefined()
    fireEvent.blur(noteOf('VP8*'))
    expect(loadWatchlist()).toEqual([{ ...FALKLANDS, notes: 'Falklands, until November' }, BOUVET])
    expect(announced, 'one save, announced once').toBe(1)
  })

  it('is saved by Enter as well', () => {
    saveWatchlist([FALKLANDS])
    render(<WatchlistPanel />)
    fireEvent.change(noteOf('VP8*'), { target: { value: 'Falklands' } })
    fireEvent.keyDown(noteOf('VP8*'), { key: 'Enter' })
    expect(loadWatchlist()[0].notes).toBe('Falklands')
  })

  it('keeps the rest of the entry as it was: its call, its kind and its CQ-only gate', () => {
    saveWatchlist([SAMOA])
    render(<WatchlistPanel />)
    fireEvent.change(noteOf('5W1SA'), { target: { value: 'Samoa DXp, extended to 10/6' } })
    fireEvent.blur(noteOf('5W1SA'))
    expect(loadWatchlist()).toEqual([{ ...SAMOA, notes: 'Samoa DXp, extended to 10/6' }])
  })

  it('comes off the entry when its field is emptied', () => {
    saveWatchlist([SAMOA])
    render(<WatchlistPanel />)
    fireEvent.change(noteOf('5W1SA'), { target: { value: '   ' } })
    fireEvent.blur(noteOf('5W1SA'))
    expect(loadWatchlist()).toStrictEqual([{ id: 'w-5w', kind: 'call', value: '5W1SA', cqOnly: true }])
  })

  it('saves nothing when a field is left unchanged', () => {
    saveWatchlist([SAMOA, FALKLANDS])
    const before = localStorage.getItem('nexus.watchlist')
    render(<WatchlistPanel />)
    fireEvent.blur(noteOf('5W1SA'))
    fireEvent.blur(noteOf('VP8*'))
    expect(localStorage.getItem('nexus.watchlist')).toBe(before)
    expect(announced).toBe(0)
  })
})

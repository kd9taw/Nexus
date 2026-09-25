// @vitest-environment jsdom
//
// THE WATCH-LIST EDITOR SHOWS THE LIST AS IT IS NOW. It read the list once, when it opened, and
// wrote its own copy back on every edit. That was safe while it was the list's only writer; since
// the old wanted list folds into the watch list at startup (features/watchlistFold), it is not. An
// editor open before the fold landed showed the list without the folded entries — and its next
// Add wrote that stale copy over the list, deleting them. It now re-reads the list on the event
// every writer sends (`nexus:watchlist-changed`).

import { afterEach, beforeEach, expect, it } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { WatchlistPanel } from './WatchlistPanel'
import { t } from '../i18n'
import { loadWatchlist, newWatchFilter, saveWatchlist } from '../watchlist'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

/** Another writer changes the list and says so, as the fold does. */
const elsewhere = (values: string[]) =>
  act(() => {
    saveWatchlist([...loadWatchlist(), ...values.map((v) => newWatchFilter('call', v))])
    window.dispatchEvent(new Event('nexus:watchlist-changed'))
  })

/** The entries the editor lists (the hint quotes VP8* as an example, so read the list itself). */
const listed = () => [...document.querySelectorAll('.watchlist-items .watchlist-value')].map((e) => e.textContent)

it('FIX: shows entries another writer added while it was open', () => {
  saveWatchlist([newWatchFilter('dxcc', 'Bouvet')])
  render(<WatchlistPanel />)
  expect(listed()).toEqual(['Bouvet'])
  elsewhere(['VP8*', '3Y0J'])
  expect(listed()).toEqual(['Bouvet', 'VP8*', '3Y0J'])
})

it('FIX: its next edit keeps them', () => {
  render(<WatchlistPanel />)
  elsewhere(['VP8*'])
  fireEvent.change(screen.getByRole('textbox', { name: t('watchlist.add.value.aria') }), { target: { value: 'K1ABC' } })
  fireEvent.click(screen.getByRole('button', { name: t('watchlist.add.submit') }))
  expect(loadWatchlist().map((f) => f.value)).toEqual(['VP8*', 'K1ABC'])
})

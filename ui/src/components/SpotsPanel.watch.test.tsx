// @vitest-environment jsdom
//
// The Spots board marks a station on the operator's WATCH LIST with the same WATCH tile the Call
// Roster and the Classic station list use, so a watched station reads the same everywhere
// (maintainer, 2026-09-23). The tile leads the comment cell — the row's widest column, where the
// "worked" badge already sits — so the comment's ellipsis can never swallow it.
//
// And a watch match counts as a need, the way a new park does, so the board's Hide worked — on
// by default — does not hide a watched station that has already been worked.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { SpotsPanel } from './SpotsPanel'
import { saveWatchlist, type WatchFilter } from '../watchlist'
import type { SpotRow } from '../types'
import { t } from '../i18n'

const spot = (call: string, over: Partial<SpotRow> = {}): SpotRow =>
  ({
    call,
    entity: 'United States',
    zone: 5,
    state: null,
    band: '20m',
    freqMhz: 14.025,
    mode: 'CW',
    submode: 'CW',
    spotter: 'W3LPL',
    corroborators: [],
    ageSecs: 60,
    comment: 'UP 2',
    licensed: true,
    spotterLocal: true,
    ...over,
  }) as SpotRow

const VP8: WatchFilter = { id: 'w-vp8', kind: 'call', value: 'VP8*' }
const BOUVET: WatchFilter = { id: 'w-3y', kind: 'dxcc', value: 'Bouvet' }

const panel = (spots: SpotRow[]) =>
  render(<SpotsPanel spots={spots} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} />)

// Both return NULL for "not there" — never undefined. An optional chain yields undefined, and
// `expect(undefined).not.toBeNull()` PASSES: a hidden row would read as "kept".
const rowOf = (call: string): HTMLElement | null =>
  (screen.queryByText(call)?.closest('[role="row"]') as HTMLElement | null | undefined) ?? null
const tileOf = (call: string): HTMLElement | null =>
  (rowOf(call)?.querySelector('.need-chip.need-watch') as HTMLElement | null | undefined) ?? null

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  saveWatchlist([VP8, BOUVET])
})
afterEach(cleanup)

describe('the WATCH tile on the Spots board', () => {
  it('marks a spot the list names — by call and by the spot’s entity — with the roster’s tile', () => {
    panel([spot('VP8PJ', { entity: 'Falkland Islands' }), spot('3Y0J', { entity: 'Bouvet' }), spot('K1ABC')])
    for (const call of ['VP8PJ', '3Y0J']) {
      expect(tileOf(call), `${call} carries no WATCH tile`).not.toBeNull()
      expect(tileOf(call)!.textContent).toBe('WATCH')
    }
    expect(tileOf('VP8PJ')!.getAttribute('title')).toBe('On your watch list: VP8*')
    expect(tileOf('3Y0J')!.getAttribute('title')).toBe('On your watch list: Bouvet')
    expect(rowOf('K1ABC'), 'the control spot is listed').not.toBeNull()
    expect(tileOf('K1ABC')).toBeNull()
  })

  it('leads the comment cell, ahead of the worked badge and the comment', () => {
    panel([spot('VP8PJ', { entity: 'Falkland Islands' })])
    const cell = rowOf('VP8PJ')!.querySelector('.np-why')!
    expect(cell.firstElementChild?.classList.contains('need-watch')).toBe(true)
    expect(cell.textContent).toContain('UP 2')
  })

  it('Hide worked (on by default) keeps a watched station worked today, and still hides the rest', () => {
    const workedToday = { workedAgoSecs: 600, workedTodayUtc: true }
    panel([spot('VP8PJ', { entity: 'Falkland Islands', ...workedToday }), spot('W6A', workedToday), spot('K1ABC')])
    expect(rowOf('VP8PJ'), 'watched and worked today').not.toBeNull()
    expect(tileOf('VP8PJ')).not.toBeNull()
    // The filter still filters — and its count names only what it actually hid.
    expect(rowOf('W6A'), 'the unwatched worked control').toBeNull()
    expect(screen.getByRole('button', { name: t('spots.filter.worked.hidden', { count: 1 }) })).toBeTruthy()
  })
})

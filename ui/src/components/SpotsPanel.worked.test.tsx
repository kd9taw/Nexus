// @vitest-environment jsdom
//
// HIDE WORKED — the Spots panel's second default-on filter (operator decisions, 2026-09-17).
//
// A station the operator already logged inside the chosen window drops out of the firehose, on
// any band or mode — unless the Needed board still has a need for it on THIS spot's band and
// mode, which always shows. The count of what it hides rides on the chip, and one click brings
// every row back with a badge saying when it was worked.
//
// The rows only carry flags (`workedAgoSecs`, `workedTodayUtc`, computed at the station); every
// decision below is the panel's, which is what keeps "where did my spots go" answerable on
// screen.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SpotsPanel, WORKED_WINDOWS, neededHere, workedWithin } from './SpotsPanel'
import type { NeedAlert, SpotRow } from '../types'
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
    comment: '',
    licensed: true,
    spotterLocal: true,
    ...over,
  }) as SpotRow

const need = (call: string, band: string, mode: string, tags: NeedAlert['tags']): NeedAlert =>
  ({ call, entity: 'United States', band, zone: 5, tags, priority: 50, headline: '', mode, freqMhz: null }) as NeedAlert

const panel = (spots: SpotRow[], needAlerts?: NeedAlert[]) =>
  render(
    <SpotsPanel
      spots={spots}
      bandPlan={[]}
      selectedCall={null}
      onSelect={() => {}}
      onWork={() => {}}
      needAlerts={needAlerts}
    />,
  )

afterEach(() => {
  cleanup()
  sessionStorage.clear()
  localStorage.clear()
})

describe('the worked window', () => {
  it('4 h hides a station worked 14,399 s ago and not one worked 14,400 s ago', () => {
    expect(workedWithin(spot('W6A', { workedAgoSecs: 14_399 }), 14_400)).toBe(true)
    expect(workedWithin(spot('W6A', { workedAgoSecs: 14_400 }), 14_400)).toBe(false)
  })

  it('"until 0000Z" follows the station clock, not the age', () => {
    // Twenty hours ago but yesterday: not worked today. A minute ago: worked today.
    expect(workedWithin(spot('W6A', { workedAgoSecs: 72_000, workedTodayUtc: false }), 'utcDay')).toBe(false)
    expect(workedWithin(spot('W6A', { workedAgoSecs: 60, workedTodayUtc: true }), 'utcDay')).toBe(true)
  })

  it('a row with no flags (an older station) is worked in no window', () => {
    for (const w of WORKED_WINDOWS) expect(workedWithin(spot('W6A'), w), String(w)).toBe(false)
  })
})

describe('still needed here', () => {
  it('a NewBand need on the spot’s own band keeps it; the same need on another band does not', () => {
    expect(neededHere([need('W6A', '20m', 'CW', ['NewBand'])], '20m', 'CW')).toBe(true)
    expect(neededHere([need('W6A', '40m', 'CW', ['NewBand'])], '20m', 'CW')).toBe(false)
  })

  it('an activity label is not a need', () => {
    expect(neededHere([need('W6A', '20m', 'CW', ['Dxped'])], '20m', 'CW')).toBe(false)
  })
})

describe('the Hide worked chip', () => {
  const worked = spot('W6A', { workedAgoSecs: 600, workedTodayUtc: true })
  const fresh = spot('K1ABC')

  it('is on by default: the worked station is hidden and counted on the chip', () => {
    panel([worked, fresh])
    expect(screen.queryByText('W6A')).toBeNull()
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(screen.getByRole('button', { name: t('spots.filter.worked.hidden', { count: 1 }) })).toBeTruthy()
  })

  it('off hides nothing, and the worked row says when it was worked', () => {
    panel([worked, fresh])
    fireEvent.click(screen.getByRole('button', { name: t('spots.filter.worked.hidden', { count: 1 }) }))
    expect(screen.getByText('W6A')).toBeTruthy()
    expect(screen.getByText(t('spots.row.worked', { age: '10m' }))).toBeTruthy()
    expect(screen.getByRole('button', { name: t('spots.filter.worked.label') }).getAttribute('aria-pressed')).toBe('false')
  })

  it('never hides a station still needed on this band and mode — and a need elsewhere does not rescue it', () => {
    panel([worked], [need('W6A', '20m', 'CW', ['NewBand'])])
    expect(screen.getByText('W6A'), 'needed on 20 m CW, where it is spotted').toBeTruthy()
    cleanup()
    panel([worked], [need('W6A', '40m', 'CW', ['NewBand'])])
    expect(screen.queryByText('W6A'), 'a 40 m need says nothing about this 20 m spot').toBeNull()
  })

  it('the window decides: 1 h keeps a station worked 2 h ago, 4 h hides it', () => {
    const twoHours = spot('W6A', { workedAgoSecs: 7_200, workedTodayUtc: true })
    panel([twoHours, fresh])
    const picker = screen.getByRole('combobox', { name: t('spots.filter.workedWindow.aria') })
    fireEvent.change(picker, { target: { value: '3600' } })
    expect(screen.getByText('W6A')).toBeTruthy()
    fireEvent.change(picker, { target: { value: '14400' } })
    expect(screen.queryByText('W6A')).toBeNull()
  })

  it('points at the chip when it is what empties the list', () => {
    panel([worked])
    expect(screen.getByText(t('spots.empty.worked', { count: 1 }))).toBeTruthy()
  })
})

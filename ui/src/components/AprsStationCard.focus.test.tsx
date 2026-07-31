// @vitest-environment jsdom
//
// Focus custody for the station card (census CLASS-6 #6): the card takes focus
// on open (so Escape works and JAWS announces the dialog) but must take it
// SILENTLY (preventScroll) and must GIVE IT BACK on close — the old code let
// focus fall to <body>, restarting the operator's Tab position at the top of
// the document after every card.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { AprsStationCard } from './AprsStationCard'
import type { AprsStation } from '../api'

vi.mock('../api', () => ({ openQrzPage: vi.fn(async () => {}) }))

const NOW = 1_700_000_000

function stn(over: Partial<AprsStation> = {}): AprsStation {
  return {
    call: 'W9AA-9',
    lat: 41.9,
    lon: -87.6,
    symbolTable: '/',
    symbolCode: '>',
    kind: 'position',
    text: '',
    speedKnots: null,
    courseDeg: null,
    path: [],
    raw: 'W9AA-9>APRS,WIDE1-1:!4154.00N/08736.00W>',
    lastHeardUnix: NOW - 60,
    lastRfUnix: NOW - 60,
    lastInetUnix: null,
    sourceKind: 'rf',
    packets: 3,
    firstHeardUnix: NOW - 3600,
    wx: null,
    ...over,
  }
}

const mount = (call = 'W9AA-9') =>
  render(<AprsStationCard station={stn({ call })} nowSec={NOW} me={null} onClose={() => {}} />)

let outside: HTMLButtonElement
beforeEach(() => {
  outside = document.createElement('button')
  outside.textContent = 'heard-stations row'
  document.body.appendChild(outside)
  outside.focus()
})
afterEach(() => {
  cleanup()
  outside.remove()
})

describe('AprsStationCard focus custody', () => {
  it('takes focus on open without scrolling, and returns it on close', () => {
    const focusSpy = vi.spyOn(HTMLElement.prototype, 'focus')
    const { unmount } = mount()
    const card = screen.getByRole('dialog')
    expect(document.activeElement).toBe(card)
    // The take must be silent — no ancestor-chain reveal walk over the map.
    expect(focusSpy).toHaveBeenCalledWith({ preventScroll: true })
    unmount()
    // Close (✕ / Escape / new selection all unmount) hands focus back.
    expect(document.activeElement).toBe(outside)
    focusSpy.mockRestore()
  })

  it('keeps the ORIGINAL restore target across station changes while open', () => {
    const { rerender, unmount } = render(
      <AprsStationCard station={stn()} nowSec={NOW} me={null} onClose={() => {}} />,
    )
    // Focus now sits on the card; a click on another map station re-fires the
    // focus effect — it must not record the card itself as "where focus was".
    rerender(
      <AprsStationCard station={stn({ call: 'K9XYZ' })} nowSec={NOW} me={null} onClose={() => {}} />,
    )
    expect(document.activeElement).toBe(screen.getByRole('dialog'))
    unmount()
    expect(document.activeElement).toBe(outside)
  })
})

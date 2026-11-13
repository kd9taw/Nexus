// @vitest-environment jsdom
//
// #506 — the Needed board's band chips: 60 m was missing from the always-present list, and a band
// taken from a live alert (60 m, 2 m, 70 cm) was appended where it arrived, after 6 m. The chips are
// now one list in band order, low to high.
import { describe, it, expect, afterEach, beforeEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import type { NeedAlert } from '../types'

beforeEach(() => localStorage.clear())
afterEach(cleanup)

const alert = (call: string, band: string): NeedAlert =>
  ({
    call,
    entity: 'United States',
    band,
    zone: 5,
    tags: [],
    priority: 1,
    headline: 'New band slot',
    mode: 'FT8',
    freqMhz: null,
  }) as NeedAlert

function bandChips(alerts: NeedAlert[]): string[] {
  const { container } = render(
    <NeededPanel alerts={alerts} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} onWork={() => {}} />,
  )
  fireEvent.click(container.querySelector('.np-filter-toggle')!)
  return [...container.querySelectorAll('.np-filter-bands .np-chip')].map((b) => b.textContent ?? '')
}

describe('#506 the Needed band chips run in band order', () => {
  it('60 m is always offered, between 80 m and 40 m', () => {
    expect(bandChips([])).toEqual(['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m'])
  })

  it('a band from a live alert takes its place by frequency, not the order it arrived in', () => {
    const chips = bandChips([alert('W1AW', '70cm'), alert('K2ABC', '2m'), alert('G4XYZ', '2200m'), alert('JA1AA', '20m')])
    expect(chips).toEqual(['2200m', '160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '2m', '70cm'])
  })

  it('a band label it does not know goes last rather than vanishing', () => {
    expect(bandChips([alert('W1AW', 'QO-100')]).at(-1)).toBe('QO-100')
  })
})

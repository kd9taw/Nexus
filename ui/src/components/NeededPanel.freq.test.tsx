// @vitest-environment jsdom
//
// The Needed list shows the frequency (operator request, 2026-08-22).
//
// It was already in the data — `NeedAlert.freqMhz`, used for the QSY tooltip — but a needs list
// is where an operator decides what to chase next, and "which band" is a coarser question than
// "where exactly". The panel could tell you a rare one was on 20 m without telling you it was on
// 14.074.
//
// Two things this pins that are easy to get wrong:
//   * A need with NO exact frequency is ORDINARY, not an error. Many needs are derived from the
//     logbook rather than spotted, so those rows must read as deliberate rather than broken.
//   * Sorting by frequency must put those rows LAST, not first. Treating a missing frequency as
//     zero would bury the least actionable rows above 1.8 MHz.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup, within, fireEvent } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import type { NeedAlert } from '../types'

afterEach(cleanup)

const alert = (over: Partial<NeedAlert>): NeedAlert =>
  ({
    call: 'K1ABC',
    entity: 'United States',
    band: '20m',
    zone: 5,
    tags: [],
    priority: 1,
    headline: 'New band slot',
    mode: 'FT8',
    freqMhz: null,
    ...over,
  }) as NeedAlert

const panel = (alerts: NeedAlert[]) =>
  render(
    <NeededPanel
      alerts={alerts}
      bandPlan={[]}
      selectedCall={null}
      onQsy={() => {}}
      onSelect={() => {}}
      onWork={() => {}}
    />,
  )

describe('the frequency column', () => {
  it('shows the spot frequency to the kHz an operator would dial', () => {
    panel([alert({ call: 'DL1XYZ', freqMhz: 14.074 })])
    expect(screen.getByText('14.074')).toBeTruthy()
  })

  it('keeps trailing zeros, because a ragged decimal is slow to scan', () => {
    // 7.1 must read as 7.100 under 14.074, not as a shorter string in the same column.
    panel([alert({ call: 'G0ABC', band: '40m', freqMhz: 7.1 })])
    expect(screen.getByText('7.100')).toBeTruthy()
  })

  it('says so with an em-dash when the need is band-level only', () => {
    // The control that matters: a blank cell reads as a rendering fault, and most needs are
    // derived rather than spotted.
    panel([alert({ call: 'VK2DEF', freqMhz: null })])
    const row = screen.getByText('VK2DEF').closest('.np-row') as HTMLElement
    expect(within(row).getByText('—')).toBeTruthy()
  })

  it('sorts a missing frequency LAST, not as zero', () => {
    panel([
      alert({ call: 'NOFREQ', freqMhz: null }),
      alert({ call: 'LOW', band: '160m', freqMhz: 1.84 }),
      alert({ call: 'HIGH', freqMhz: 14.074 }),
    ])
    // Scoped to the header: "Freq" also appears in the filter chrome, and an ambiguous query
    // would pass or fail for reasons unrelated to sorting.
    const header = document.querySelector('.np-header') as HTMLElement
    fireEvent.click(within(header).getByRole('button', { name: /Freq/i }))
    const calls = [...document.querySelectorAll('.np-row:not(.np-header) .np-call')].map((e) =>
      e.textContent?.trim().replace(/\s.*$/, ''),
    )
    expect(calls[0]).toBe('LOW')
    expect(calls[calls.length - 1]).toBe('NOFREQ')
  })
})

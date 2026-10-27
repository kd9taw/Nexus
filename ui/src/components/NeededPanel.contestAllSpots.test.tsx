// @vitest-environment jsdom
//
// The contest "show all SSB" bar on the Needed board. During a contest the board's
// demand gate (only unworked NEEDS surface) hides exactly what a contester scans for: everyone on
// the air. This bar, shown ONLY while a contest runs, offers one click to the un-gated Spots view.
// Outside a contest it does not render and the board is exactly as it was.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { NeededPanel } from './NeededPanel'
import type { NeedAlert } from '../types'

afterEach(cleanup)

const alert = (over: Partial<NeedAlert> = {}): NeedAlert =>
  ({
    call: 'K1ABC',
    entity: 'United States',
    band: '20m',
    zone: 5,
    tags: [],
    priority: 1,
    headline: 'New band slot',
    mode: 'Phone',
    freqMhz: 14.25,
    ...over,
  }) as NeedAlert

describe('Needed board — contest show-all-SSB bar', () => {
  it('shows an "all spots" control while a contest runs and calls the host on click', () => {
    const onShowAllSpots = vi.fn()
    render(
      <NeededPanel
        alerts={[alert()]}
        bandPlan={[]}
        selectedCall={null}
        onQsy={() => {}}
        onSelect={() => {}}
        contestActive
        onShowAllSpots={onShowAllSpots}
      />,
    )
    const btn = screen.getByRole('button', { name: 'Show all spots' })
    expect(btn).toBeTruthy()
    fireEvent.click(btn)
    expect(onShowAllSpots).toHaveBeenCalledTimes(1)
  })

  it('does not render the bar outside a contest (default behaviour unchanged)', () => {
    render(
      <NeededPanel
        alerts={[alert()]}
        bandPlan={[]}
        selectedCall={null}
        onQsy={() => {}}
        onSelect={() => {}}
        contestActive={false}
        onShowAllSpots={() => {}}
      />,
    )
    expect(screen.queryByRole('button', { name: 'Show all spots' })).toBeNull()
  })

  it('does not render the bar when no host handler is wired', () => {
    render(
      <NeededPanel
        alerts={[alert()]}
        bandPlan={[]}
        selectedCall={null}
        onQsy={() => {}}
        onSelect={() => {}}
        contestActive
      />,
    )
    expect(screen.queryByRole('button', { name: 'Show all spots' })).toBeNull()
  })
})

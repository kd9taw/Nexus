// @vitest-environment jsdom
//
// The per-row Spot button on the Spots board. Each row carries a one-click Spot that
// opens the SAME reviewable cluster-spot popup (SpotDialog) the cockpits open, seeded with THAT
// row's call and frequency. The press must spot the station, not work the row (the row's own
// click QSYs/works it), and the confirm popup is kept (it posts publicly from the station's login).
import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { SpotsPanel } from './SpotsPanel'
import type { SpotRow } from '../types'

const spot = (over: Partial<SpotRow>): SpotRow => ({
  call: 'W1AW',
  entity: 'United States',
  zone: 5,
  band: '20m',
  freqMhz: 14.025,
  mode: 'CW',
  spotter: 'K3LR',
  corroborators: [],
  ageSecs: 30,
  comment: '',
  licensed: true,
  ...over,
})

describe('SpotsPanel row Spot button', () => {
  afterEach(cleanup)

  it('opens the cluster-spot popup seeded with the row call and frequency', () => {
    render(
      <SpotsPanel
        spots={[spot({ call: 'DL1ABC', band: '40m', mode: 'Phone', freqMhz: 7.185 })]}
        bandPlan={[]}
        selectedCall={null}
        onSelect={() => {}}
        onWork={() => {}}
      />,
    )
    // No popup until the row's Spot button is pressed.
    expect(screen.queryByRole('dialog')).toBeNull()
    fireEvent.click(screen.getByLabelText('Spot DL1ABC on the DX cluster'))
    const dialog = screen.getByRole('dialog')
    expect(dialog).toBeTruthy()
    // Seeded: call = this row's call, frequency = this row's dial (3 dp, dot-decimal).
    const inputs = dialog.querySelectorAll('input.settings-input')
    expect((inputs[0] as HTMLInputElement).value).toBe('DL1ABC')
    expect((inputs[1] as HTMLInputElement).value).toBe('7.185')
  })

  it('spots the station without working the row (stopPropagation)', () => {
    const onWork = vi.fn()
    const onSelect = vi.fn()
    render(
      <SpotsPanel
        spots={[spot({ call: 'JA2DEF', freqMhz: 14.074 })]}
        bandPlan={[]}
        selectedCall={null}
        onSelect={onSelect}
        onWork={onWork}
      />,
    )
    fireEvent.click(screen.getByLabelText('Spot JA2DEF on the DX cluster'))
    expect(screen.getByRole('dialog')).toBeTruthy()
    // The Spot press must NOT trigger the row's work/select handlers.
    expect(onWork).not.toHaveBeenCalled()
    expect(onSelect).not.toHaveBeenCalled()
  })

  it('every rendered row carries its own Spot button', () => {
    render(
      <SpotsPanel
        spots={[
          spot({ call: 'W1AW', freqMhz: 14.025 }),
          spot({ call: 'K2ABC', freqMhz: 14.03, spotter: 'W1AW' }),
        ]}
        bandPlan={[]}
        selectedCall={null}
        onSelect={() => {}}
        onWork={() => {}}
      />,
    )
    expect(screen.getByLabelText('Spot W1AW on the DX cluster')).toBeTruthy()
    expect(screen.getByLabelText('Spot K2ABC on the DX cluster')).toBeTruthy()
  })
})

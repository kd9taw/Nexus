// @vitest-environment jsdom
//
// Who is at the key (#25). Two operators swapping the mic every few contacts cannot be asked to
// open Settings, and a wrong operator is SILENT — nothing misbehaves, and it is discovered at
// submission when the log is already wrong. So the indicator must be always visible and must say
// who without being asked.
//
// The paired risk is the opposite: for the single-op station that is nearly everyone, this must
// cost nothing at all. Both directions are tested.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'


beforeAll(() => {
  // Radix DropdownMenu measures and captures pointers; jsdom has neither.
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})

afterEach(cleanup)

const radio = {
  dialMhz: 14.074,
  band: '20m',
  sideband: 'USB',
  rigMode: 'USB',
  rigConfirmed: true,
  nextSlotMs: 0,
  txEven: true,
  txCycleAuto: true,
  txEnabled: false,
  txAllowed: true,
  transmitting: false,
  tuning: false,
  qsoRecording: false,
  catOk: true,
  dtSec: 0,
  clockOffsetMs: 0,
} as unknown as RadioStatus

function renderBar(onOpenGuide: () => void, over: Record<string, unknown> = {}) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW"
      mygrid="EN52xa"
      radio={radio}
      link={{ tier: 'FT8' } as never}
      bandPlan={[]}
      onSetFrequency={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onHaltTx={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onSetHoldTxFreq={noop}
      tier="FT8"
      onTierChange={noop}
      onOpenGuide={onOpenGuide}
      {...over}
    />,
  )
}

describe('the operator at the key', () => {
  it('is not shown at all for a single-op station', () => {
    renderBar(() => {}, { operator: '' })
    expect(
      screen.queryByTitle(/who is at the key/i),
      'an empty operator is the single-op case and must cost no width',
    ).toBeNull()
  })

  it('shows who is operating, without being asked', () => {
    renderBar(() => {}, { operator: 'W1ABC' })
    const chip = screen.getByTitle(/who is at the key/i)
    expect(chip.textContent).toMatch(/W1ABC/)
  })

  it('swaps seats from the roster in one click', () => {
    const onSetOperator = vi.fn()
    renderBar(() => {}, {
      operator: 'W1ABC',
      operatorRoster: ['W1ABC', 'G0PQR'],
      onSetOperator,
    })
    // Radix opens on pointerdown, not click — same gesture the Help menu test uses.
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), {
      button: 0,
      ctrlKey: false,
    })
    fireEvent.click(screen.getByText(/Switch to G0PQR/))
    expect(onSetOperator).toHaveBeenCalledWith('G0PQR')
  })

  it('does not offer to switch to whoever is already operating', () => {
    renderBar(() => {}, {
      operator: 'W1ABC',
      operatorRoster: ['W1ABC', 'G0PQR'],
      onSetOperator: vi.fn(),
    })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), {
      button: 0,
      ctrlKey: false,
    })
    expect(screen.queryByText(/Switch to W1ABC/)).toBeNull()
  })

  it('can go back to single-op, because that is how an activation ends', () => {
    const onSetOperator = vi.fn()
    renderBar(() => {}, { operator: 'W1ABC', operatorRoster: [], onSetOperator })
    fireEvent.pointerDown(screen.getByTitle(/who is at the key/i), {
      button: 0,
      ctrlKey: false,
    })
    fireEvent.click(screen.getByText(/Single operator/))
    expect(onSetOperator).toHaveBeenCalledWith('')
  })
})

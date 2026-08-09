// @vitest-environment jsdom
//
// Help ▸ Getting started is the guide's only always-available entry point: it
// lives in the one top-bar group no cockpit hides. Wiring it is three lines, and
// all three are silently droppable — the menu item can lose its handler, or the
// group can be moved behind a `hideDigitalChrome`-style flag. This opens the
// real menu and asserts the item calls back.
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
      theme="dark"
      onThemeChange={noop}
      onOpenGuide={onOpenGuide}
      {...over}
    />,
  )
}

describe('Help ▸ Getting started', () => {
  it('opens the guide from the top bar', () => {
    const onOpenGuide = vi.fn()
    renderBar(onOpenGuide)
    fireEvent.pointerDown(
      screen.getByRole('button', { name: 'Help' }),
      { button: 0, ctrlKey: false, pointerType: 'mouse' },
    )
    fireEvent.click(screen.getByText('Getting started'))
    expect(onOpenGuide).toHaveBeenCalledTimes(1)
  })

  it('survives every chrome-hiding flag a cockpit sets', () => {
    const onOpenGuide = vi.fn()
    renderBar(onOpenGuide, {
      hideTxControls: true,
      hideFrequencyControl: true,
      hideDigitalChrome: true,
    })
    expect(screen.getByRole('button', { name: 'Help' })).toBeTruthy()
  })
})

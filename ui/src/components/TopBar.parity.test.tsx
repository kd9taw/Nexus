// @vitest-environment jsdom
// The auto-picked TX cycle must be VISIBLE (POTA field report). Double-clicking a station
// flips the period correctly, but the flip showed only in the Auto button's small text — the
// two big Tx-1st/2nd buttons light only for a manual lock. A working flip that looks like a
// no-op gets reported as broken, and was.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const radio = (over: Partial<RadioStatus>) =>
  ({
    dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true,
    nextSlotMs: 0, txCycleAuto: true, txEven: true, txEnabled: false, txAllowed: true,
    transmitting: false, tuning: false, qsoRecording: false, catOk: true, dtSec: 0,
    clockOffsetMs: 0, ...over,
  }) as unknown as RadioStatus

function bar(r: RadioStatus) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW" mygrid="EN52xa" radio={r} link={{ tier: 'FT8' } as never} bandPlan={[]}
      onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop}
      onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop}
      tier="FT8" onTierChange={noop}
 onOpenGuide={noop}
    />,
  )
}

describe('the auto-picked cycle is visible on the big buttons', () => {
  it('marks the side auto currently holds, as derived — never as the manual lock', () => {
    bar(radio({ txCycleAuto: true, txEven: true }))
    const first = screen.getByRole('button', { name: /Tx 1st/ })
    const second = screen.getByRole('button', { name: /Tx 2nd/ })
    expect(first.className).toContain('derived')
    expect(first.className, 'derived must not impersonate a manual lock').not.toContain('active')
    expect(first.getAttribute('aria-pressed'), 'not pressed — the operator did not lock it').toBe('false')
    expect(second.className).not.toContain('derived')
  })

  it('moves when the flip happens — the whole point', () => {
    bar(radio({ txCycleAuto: true, txEven: false }))
    expect(screen.getByRole('button', { name: /Tx 2nd/ }).className).toContain('derived')
    expect(screen.getByRole('button', { name: /Tx 1st/ }).className).not.toContain('derived')
  })

  it('yields entirely to a manual lock', () => {
    bar(radio({ txCycleAuto: false, txEven: true }))
    const first = screen.getByRole('button', { name: /Tx 1st/ })
    expect(first.className).toContain('active')
    expect(first.className).not.toContain('derived')
  })
})

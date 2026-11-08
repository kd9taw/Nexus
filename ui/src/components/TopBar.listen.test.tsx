// @vitest-environment jsdom
//
// #468: the top bar draws the listening level's chip in the group every screen keeps (beside Help) while
// receive audio plays on this computer, and nothing at all otherwise.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { createElement } from 'react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'

vi.mock('../api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.0.0')),
  haltTx: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
}))
afterEach(cleanup)

const RADIO = { dialMhz: 14.074, band: '20m', sideband: 'USB', txEnabled: true, txAllowed: true, tuning: false,
  transmitting: false, txEven: true, txCycleAuto: true, holdTxFreq: false, catOk: true, dtSec: 0, clockOffsetMs: 0 }

function bar(extra: Record<string, unknown>) {
  const noop = () => {}
  return render(
    createElement(TopBar, {
      mycall: 'KD9TAW', mygrid: 'EN61', radio: RADIO as unknown as RadioStatus, link: { tier: 'FT8' }, bandPlan: [],
      onSetFrequency: noop, onSetTxEnabled: noop, onSetTune: noop, onHaltTx: noop, onSetTxEven: noop,
      onSetTxCycleAuto: noop, onSetHoldTxFreq: noop, tier: 'FT8', onTierChange: noop, onOpenGuide: noop,
      ...extra,
    } as unknown as Parameters<typeof TopBar>[0]),
  ).container
}

describe('the top bar’s listening level (#468)', () => {
  it.each([
    ['an FT screen', {}],
    ['a cockpit (Phone, CW, RTTY…)', { hideTxControls: true, hideFrequencyControl: true, hideDigitalChrome: true }],
  ])('on %s, in the chip group beside Help', (_where, extra) => {
    const c = bar({ ...extra, listenLevel: { level: 0.3, onCommit: async () => {} } })
    // Only the chip is in the bar; its slider opens in a popover (ListenLevel.test.tsx).
    expect(c.querySelector('.topbar-chips button.listen-chip')).not.toBeNull()
    expect(c.querySelector('.topbar-chips input')).toBeNull()
  })

  it('CONTROL: absent while receive audio is not played here', () => {
    expect(bar({}).querySelector('.listen-chip')).toBeNull()
  })
})

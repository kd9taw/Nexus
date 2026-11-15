// @vitest-environment jsdom
//
// JS8 TRANSMITS, SO THE TOP BAR OFFERS TX ON AND TUNE ON IT.
//
// The UI's receive-only list still named JS8 after the engine's JS8 mode became transmit-capable.
// The JS8 screen hides the top bar's TX cluster (its own header carries the controls), but the
// tier stays JS8 when the operator moves on to the Logbook, the Map, Field Day or Settings, and
// there the bar's TX On and Tune were greyed out with "This mode is receive-only in Nexus". The
// engine is the TX guard either way; this only stops the bar refusing what the engine allows.
// CONTROL: Stop TX is live on both tiers, and an empty list of receive-only tiers is what the
// engine's modes declare today.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus, Tier } from '../types'
import { isRxOnly } from '../types'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const radio = {
  dialMhz: 14.078, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true,
  nextSlotMs: 0, txCycleAuto: true, txEven: true, txEnabled: false, txAllowed: true,
  transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
  qsoRecording: false, catOk: true, dtSec: 0, clockOffsetMs: 0,
} as unknown as RadioStatus

/** The bar as App renders it over a screen with no cockpit of its own (Logbook, Map, Field Day):
 *  the TX cluster shows. */
function bar(tier: Tier) {
  const noop = () => {}
  render(
    <TopBar
      mycall="W9ABC" mygrid="EN52xa" radio={radio} link={{ tier } as never} bandPlan={[]}
      onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop}
      onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop}
      tier={tier} onTierChange={noop} onOpenGuide={noop}
    />,
  )
  const cluster = screen.getByRole('group', { name: 'Transmit controls' })
  const byText = (s: string) =>
    [...cluster.querySelectorAll('button')].find((b) => b.textContent === s) as HTMLButtonElement
  return { txOn: byText('TX Off'), tune: byText('Tune'), stop: cluster.querySelector('[data-stop-control="stop-tx"]') as HTMLButtonElement }
}

describe('the top bar TX cluster on the JS8 tier', () => {
  for (const tier of ['JS8', 'FT8'] as Tier[]) {
    it(`${tier}: TX On and Tune are offered, Stop TX is live`, () => {
      const { txOn, tune, stop } = bar(tier)
      expect(txOn, 'the TX On/Off button rendered').toBeTruthy()
      expect(txOn.disabled, `TX On on ${tier}`).toBe(false)
      expect(txOn.title).not.toMatch(/receive-only/i)
      expect(tune.disabled, `Tune on ${tier}`).toBe(false)
      expect(stop.disabled).toBe(false)
    })
  }

  it('no shipped tier is receive-only, as the modes crate declares', () => {
    const tiers: Tier[] = ['TempoFast', 'TempoDeep', 'FT8', 'FT4', 'FT2', 'FST4', 'FST4W', 'Q65', 'MSK144', 'JT65', 'WSPR', 'JS8']
    expect(tiers.filter(isRxOnly)).toEqual([])
  })
})

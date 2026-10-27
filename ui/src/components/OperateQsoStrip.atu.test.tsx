// @vitest-environment jsdom
//
// ⭐ #344 IN THE FT STRIP: "after running ATU maybe have it change to ATU ON (and then you could
// click to turn it off)". Operate draws its own ATU button (`OperateQsoStrip`), so the rule
// `CockpitTxStrip.atu.test.tsx` pins for the other screens is pinned here too: the label follows
// the radio's tuner, a click always runs a tune-up, and the button's menu switches the tuner out
// where the CAT path can.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup, screen, fireEvent } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const radio = (over: Partial<RadioStatus>) =>
  ({
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true,
    txEnabled: true, transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    holdTxFreq: false, txEven: true, txCycleAuto: true, ...over,
  }) as unknown as RadioStatus

function strip(over: Partial<RadioStatus>, onAtuTune: () => void, onAtuBypass: () => void) {
  const noop = vi.fn()
  render(
    <OperateQsoStrip
      qso={null}
      radio={radio(over)}
      onSetMode={noop}
      onCallCq={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onAtuTune={onAtuTune}
      onAtuBypass={onAtuBypass}
      onHaltTx={noop}
      onSetHoldTxFreq={noop}
    />,
  )
}

describe("Operate's ATU follows the radio's tuner (#344)", () => {
  it('ATU ON while the tuner is in line, and a click still runs a tune-up', () => {
    const tune = vi.fn()
    const out = vi.fn()
    strip({ atu: true, atuSwitchOut: true }, tune, out)
    const on = screen.getByRole('button', { name: 'ATU ON' })
    expect(on.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(on)
    expect([tune.mock.calls.length, out.mock.calls.length]).toEqual([1, 0])
  })

  it('ATU while the tuner is out, and the click runs the tune-up', () => {
    const tune = vi.fn()
    const out = vi.fn()
    strip({ atu: false, atuSwitchOut: true }, tune, out)
    const atu = screen.getByRole('button', { name: 'ATU' })
    expect(atu.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(atu)
    expect([tune.mock.calls.length, out.mock.calls.length]).toEqual([1, 0])
  })

  it('the right-click menu switches the tuner out where the path can, and runs no tune-up', async () => {
    const tune = vi.fn()
    const out = vi.fn()
    strip({ atu: true, atuSwitchOut: true }, tune, out)
    fireEvent.contextMenu(screen.getByRole('button', { name: 'ATU ON' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Switch tuner out' }))
    expect([out.mock.calls.length, tune.mock.calls.length]).toEqual([1, 0])
  })

  it('CONTROL: no menu where the path cannot switch it out (the Flex client)', () => {
    const tune = vi.fn()
    const out = vi.fn()
    strip({ atu: true, atuSwitchOut: false }, tune, out)
    const btn = screen.getByRole('button', { name: 'ATU ON' })
    fireEvent.contextMenu(btn)
    fireEvent.click(btn)
    expect(screen.queryByRole('menuitem')).toBeNull()
    expect([tune.mock.calls.length, out.mock.calls.length]).toEqual([1, 0])
  })
})

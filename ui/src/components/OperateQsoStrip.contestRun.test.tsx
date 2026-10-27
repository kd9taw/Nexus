// @vitest-environment jsdom
//
// THE AUTO-CQ PILL GOES OUT WHEN A CONTEST'S RUNNING STOPS.
//
// In a contest the Running stops after each contact, as WSJT-X does with Auto Seq on: TX goes
// off, the CQ waits, and callers are not taken until the operator calls CQ again. The pill kept
// shining and its tooltip kept saying "calling CQ continuously", so a glance at the strip said a
// CQ was going out when nothing was. In a contest (the snapshot carries no `qso` there) the pill
// now shows only while TX is on, and its tooltip says the run stops after each contact. Outside a
// contest Call CQ still returns to CQ after each contact, and the pill is unchanged.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import { t } from '../i18n'
import type { QsoStatus, RadioStatus } from '../types'

afterEach(cleanup)

const radio = (txEnabled: boolean) =>
  ({
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true,
    txEnabled, transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    holdTxFreq: false, txEven: true, txCycleAuto: true,
  }) as unknown as RadioStatus

/** A plain CQ run outside a contest, calling its CQ. */
const plainRun = {
  state: 'CallingCq', dxcall: null, rxReport: null, running: true, cqRunning: true,
} as unknown as QsoStatus

function pill(qso: QsoStatus | null, contestRunning: boolean | undefined, txEnabled: boolean) {
  const noop = vi.fn()
  const { container } = render(
    <OperateQsoStrip
      qso={qso}
      contestRunning={contestRunning}
      radio={radio(txEnabled)}
      onSetMode={noop}
      onCallCq={noop}
      onResend={noop}
      onFreetext={noop}
      onLog={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onHaltTx={noop}
      onSetHoldTxFreq={noop}
    />,
  )
  return container.querySelector('.cq-autocq')
}

describe("the AUTO-CQ pill follows a contest's Running", () => {
  it('a stopped contest run (TX off): no pill', () => {
    expect(pill(null, true, false)).toBeNull()
  })

  it('a contest run calling CQ (TX on): the pill says it stops after each contact', () => {
    const p = pill(null, true, true)
    expect(p, 'the pill did not render').not.toBeNull()
    expect(p!.getAttribute('title')).toBe(t('operate.strip.autoCq.contestTitle'))
    expect(t('operate.strip.autoCq.contestTitle')).not.toBe(t('operate.strip.autoCq.title'))
  })

  it("the contest's S&P: no pill, as before", () => {
    expect(pill(null, false, true)).toBeNull()
  })

  it('outside a contest a CQ run keeps its pill and tooltip, TX on or off, as before', () => {
    for (const tx of [true, false]) {
      const p = pill(plainRun, undefined, tx)
      expect(p, `TX ${tx ? 'on' : 'off'}`).not.toBeNull()
      expect(p!.getAttribute('title')).toBe(t('operate.strip.autoCq.title'))
      cleanup()
    }
  })
})

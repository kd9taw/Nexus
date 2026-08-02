// @vitest-environment jsdom
//
// The receive-only tiers (Q65 / MSK144 / FST4 / FST4W / JT65 / WSPR) decode but do
// not transmit. The engine refuses to arm TX or start a CQ run on them; these cases
// pin the other half — that the strip does not OFFER those controls.
//
// The bug this came from: on MSK144, Call CQ looked like it worked. The sequencer
// armed, "Now sending" filled in, and the operator watched a CQ run that produced no
// RF and no CAT. A control that silently does nothing is worse than one that is
// visibly unavailable.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'

import { OperateQsoStrip } from './OperateQsoStrip'
import type { QsoStatus, RadioStatus } from '../types'

afterEach(cleanup)

const qso = { running: false, state: 'Idle', dxcall: null, txNow: null } as unknown as QsoStatus
const radio = {
  txEnabled: false,
  tuning: false,
  holdTxFreq: false,
} as unknown as RadioStatus

/** The project has no jest-dom matchers wired up — assert the DOM property. */
const btn = (name: RegExp) =>
  screen.getByRole('button', { name }) as HTMLButtonElement

function strip(rxOnly: boolean) {
  return render(
    <OperateQsoStrip
      qso={qso}
      radio={radio}
      rxOnly={rxOnly}
      onSetMode={vi.fn()}
      onCallCq={vi.fn()}
      onResend={vi.fn()}
      onFreetext={vi.fn()}
      onLog={vi.fn()}
      onSetTxEnabled={vi.fn()}
      onSetTune={vi.fn()}
      onHaltTx={vi.fn()}
      onSetHoldTxFreq={vi.fn()}
    />,
  )
}

describe('OperateQsoStrip on a receive-only tier', () => {
  it('disables every control that would commit to transmitting', () => {
    strip(true)
    expect(btn(/call cq/i).disabled).toBe(true)
    expect(btn(/tx off|tx on/i).disabled).toBe(true)
    expect(btn(/tune/i).disabled).toBe(true)
  })

  it('leaves Stop TX live — disarming must always be available', () => {
    // The operator's way out if they switched tiers mid-over. A guard that also
    // blocks the stop would be a transmit-safety regression, not a fix.
    strip(true)
    expect(btn(/stop tx/i).disabled).toBe(false)
  })

  it('says it is not transmitting instead of showing a pending over', () => {
    strip(true)
    expect(screen.getByText(/receive-only, not transmitting/i)).toBeTruthy()
  })

  it('explains why, rather than just going grey', () => {
    strip(true)
    expect(btn(/call cq/i).getAttribute('title')).toMatch(/receive-only/i)
  })

  it('leaves a transmit-capable tier completely alone', () => {
    strip(false)
    expect(btn(/call cq/i).disabled).toBe(false)
    expect(btn(/tx off|tx on/i).disabled).toBe(false)
    expect(btn(/tune/i).disabled).toBe(false)
    expect(screen.queryByText(/receive-only/i)).toBeNull()
  })
})

describe('the controls absorbed from the deleted status row', () => {
  // TX AUTO / Skip Tx1 / the next-slot countdown / the telemetry cell moved into
  // the strip as OPTIONAL props (decode-first rebuild). Two invariants: a host
  // that passes none of them gets the strip cleanly without them, and the
  // relocated controls keep their pre-move enablement — they never disabled on
  // receive-only tiers outside the strip, so a future `disabled={noTx}` creeping
  // onto them during the move is a regression this pins out.
  it('are cleanly absent when the host passes none of the absorbed props', () => {
    const { container } = strip(true) // the fixture passes no absorbed props
    expect(screen.queryByRole('button', { name: /tx auto|tx 1st|tx 2nd/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /skip tx1/i })).toBeNull()
    expect(screen.queryByText(/next \d+s/)).toBeNull()
    expect(container.querySelector('.cq-period, .cq-skiptx1, .cq-next, .cq-telemetry')).toBeNull()
  })

  it('keep their pre-move enablement on a receive-only tier', () => {
    render(
      <OperateQsoStrip
        qso={qso}
        radio={{ ...radio, txCycleAuto: true, txEven: true } as RadioStatus}
        rxOnly
        onSetMode={vi.fn()}
        onCallCq={vi.fn()}
        onResend={vi.fn()}
        onFreetext={vi.fn()}
        onLog={vi.fn()}
        onSetTxEnabled={vi.fn()}
        onSetTune={vi.fn()}
        onHaltTx={vi.fn()}
        onSetHoldTxFreq={vi.fn()}
        onSetTxEven={vi.fn()}
        onSetTxCycleAuto={vi.fn()}
        skipTx1={false}
        onSkipTx1={vi.fn()}
        nextSlotSec={12}
      />,
    )
    expect(btn(/tx auto/i).disabled).toBe(false)
    expect(btn(/skip tx1/i).disabled).toBe(false)
    expect(screen.getByText(/next 12s/)).toBeTruthy()
  })
})

describe('OperateQsoStrip on a beacon tier (WSPR / FST4W)', () => {
  function beaconStrip() {
    return render(
      <OperateQsoStrip
        qso={qso}
        radio={radio}
        beacon
        onSetMode={vi.fn()}
        onCallCq={vi.fn()}
        onResend={vi.fn()}
        onFreetext={vi.fn()}
        onLog={vi.fn()}
        onSetTxEnabled={vi.fn()}
        onSetTune={vi.fn()}
        onHaltTx={vi.fn()}
        onSetHoldTxFreq={vi.fn()}
      />,
    )
  }

  it('keeps the TX controls live — a beacon DOES key the radio', () => {
    // The distinction from receive-only. Disabling TX here would be wrong: the
    // operator still has to arm transmit before the schedule can key anything.
    beaconStrip()
    expect(btn(/tx off|tx on/i).disabled).toBe(false)
    expect(btn(/stop tx/i).disabled).toBe(false)
  })

  it('disables the QSO controls — there is no exchange to sequence', () => {
    beaconStrip()
    expect(btn(/call cq/i).disabled).toBe(true)
    expect(btn(/s&p/i).disabled).toBe(true)
  })

  it('explains that it transmits on a schedule rather than looking broken', () => {
    beaconStrip()
    expect(btn(/call cq/i).getAttribute('title')).toMatch(/beacon/i)
    expect(screen.getByText(/transmits on schedule/i)).toBeTruthy()
  })
})

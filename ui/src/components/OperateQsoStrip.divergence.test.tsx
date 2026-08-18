// @vitest-environment jsdom
//
// THE RADIO MOVED AND OPERATE SAID NOTHING (2026-08-17 Flex audit, wave-1 #54, wave-2 #64).
//
// A mode changed at the radio — in SmartSDR, from a Maestro, or on the front panel — simply
// stands: the read-back is display-only by contract (`observe_rig_mode` never overwrites the
// commanded sideband), and the steady-state mode push only fires when the APP's own target
// changes, which a radio-side change never disturbs. Every other cockpit that can show the
// disagreement does; Operate could not, because App hides the TopBar's whole frequency-readout
// group there and the mismatch pill lives inside it. So the one cockpit that transmits
// unattended, for hours, was the one with no signal at all.
//
// The filter half is worse: the rig's real RX width is polled and stored and rendered NOWHERE in
// this cockpit, so a CW filter left in — or a SmartSDR slice narrowed at the radio — throws away
// most of the FT8 window with nothing on screen to explain the sudden quiet.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'

import { OperateQsoStrip } from './OperateQsoStrip'
import type { QsoStatus, RadioStatus } from '../types'

afterEach(cleanup)

const qso = { running: false, state: 'Idle', dxcall: null, txNow: null } as unknown as QsoStatus

function strip(radio: Partial<RadioStatus>) {
  return render(
    <OperateQsoStrip
      qso={qso}
      radio={
        {
          txEnabled: false,
          tuning: false,
          holdTxFreq: false,
          sideband: 'USB',
          rigConfirmed: true,
          ...radio,
        } as unknown as RadioStatus
      }
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

describe('Operate says when the radio no longer matches what Nexus commanded', () => {
  it('shows the rig mode when it disagrees', () => {
    strip({ rigMode: 'LSB', sideband: 'USB' })
    expect(screen.getByText(/rig: LSB/)).toBeTruthy()
  })

  it('says nothing when the rig agrees — including the DATA variant of the same sideband', () => {
    // PKTUSB is USB with the rear jack live: the same emission, and what Nexus commands for
    // FT8. Flagging it would fire permanently on every correctly-configured digital station,
    // which is exactly how a warning stops being read.
    strip({ rigMode: 'PKTUSB', sideband: 'USB' })
    expect(screen.queryByText(/^rig: /)).toBeNull()
    cleanup()
    strip({ rigMode: 'USB', sideband: 'USB' })
    expect(screen.queryByText(/^rig: /)).toBeNull()
  })

  it('says nothing before CAT has been confirmed', () => {
    // An unconfirmed read is the persisted seed, not the radio — claiming a disagreement from
    // it would greet every launch with a warning about nothing.
    strip({ rigMode: 'FM', sideband: 'USB', rigConfirmed: false })
    expect(screen.queryByText(/^rig: /)).toBeNull()
  })

  it('flags a filter far too narrow for an FT8 window', () => {
    strip({ filterWidthHz: 500 })
    expect(screen.getByText(/filter 500 Hz/)).toBeTruthy()
  })

  it('leaves an ordinary SSB filter alone', () => {
    // 2.4 kHz is the normal, perfectly good case. A chip here would nag every station on
    // every band, and a chip that is always up says nothing at all.
    strip({ filterWidthHz: 2400 })
    expect(screen.queryByText(/filter /)).toBeNull()
    cleanup()
    strip({}) // width unknown (the rig doesn't report it) is not a fault either
    expect(screen.queryByText(/filter /)).toBeNull()
  })
})

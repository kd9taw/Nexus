// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'

vi.mock('../api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.17.12')),
}))

function radio(over: Partial<RadioStatus>): RadioStatus {
  return {
    dialMhz: 14.074,
    band: '20m',
    sideband: 'USB',
    slot: 0,
    source: 'native',
    sourceLabel: 'Native',
    nextSlotMs: 5000,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    rxLevel: 0.3,
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
    ...over,
  } as unknown as RadioStatus
}

function renderBar(r: RadioStatus) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW"
      mygrid="EN61"
      radio={r}
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
      onOpenGuide={noop}
    />,
  )
}

afterEach(cleanup)

describe('TopBar mode readout', () => {
  // The USB/FM toggle wrote settings.sideband, which the CAT layer stopped reading in
  // 0aad08f8 — clicking it could not command the rig, and the retune it armed re-asserted
  // the section's policy mode, yanking a hand-set FM rig into USB/USB-D. It is gone.
  it('renders no mode toggle — it could not command the rig', () => {
    const { container } = renderBar(radio({}))
    expect(container.querySelectorAll('.freq-mode-btn')).toHaveLength(0)
    expect(screen.queryByRole('group', { name: 'Phone mode' })).toBeNull()
  })

  it('flags the rig sitting in a different mode than Nexus believes', () => {
    renderBar(radio({ sideband: 'USB', rigMode: 'FM', rigConfirmed: true }))
    expect(screen.getByText('rig: FM')).toBeTruthy()
  })

  it('stays quiet when the rig agrees', () => {
    renderBar(radio({ sideband: 'USB', rigMode: 'USB', rigConfirmed: true }))
    expect(screen.queryByText(/^rig:/)).toBeNull()
  })

  // PKTUSB/USB-D is USB with the rear jack live — the same sideband, and what Nexus
  // deliberately commands for FT8/FT4. Flagging it outside a cockpit would cry wolf on
  // every digital QSO.
  it('treats the data variants as the same sideband family', () => {
    renderBar(radio({ sideband: 'USB', rigMode: 'PKTUSB', rigConfirmed: true }))
    expect(screen.queryByText(/^rig:/)).toBeNull()
  })

  // …and FM has a data variant too, since Nexus started commanding PKTFM for an SSTV image on
  // an FM channel. A Hamlib backend reports an FTDX10 in DATA-FM as PKTFM, so without this the
  // bar cried wolf for the length of every picture.
  it('treats FM-D / PKTFM as the FM family', () => {
    renderBar(radio({ sideband: 'FM', rigMode: 'PKTFM', rigConfirmed: true }))
    expect(screen.queryByText(/^rig:/)).toBeNull()
    cleanup()
    renderBar(radio({ sideband: 'FM', rigMode: 'FM-D', rigConfirmed: true }))
    expect(screen.queryByText(/^rig:/)).toBeNull()
  })

  it('…and still flags a rig that is genuinely on the wrong side of an FM channel', () => {
    // The negative half: collapsing the family must not collapse everything. A rig left in
    // USB-D while Nexus believes FM is the reported bug, not a naming variant.
    renderBar(radio({ sideband: 'FM', rigMode: 'PKTUSB', rigConfirmed: true }))
    expect(screen.getByText('rig: PKTUSB')).toBeTruthy()
  })

  it('stays quiet when no CAT read has confirmed the mode', () => {
    renderBar(radio({ sideband: 'USB', rigMode: 'FM', rigConfirmed: false }))
    expect(screen.queryByText(/^rig:/)).toBeNull()
  })
})

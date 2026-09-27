// @vitest-environment jsdom
//
// THE TOP BAR'S TX PLATE FOLLOWS THE TRANSMITTER, WHOEVER KEYED IT.
//
// The plate keyed on `radio.transmitting` alone, and that is the FT slot flag: only the
// slot/beacon path writes it. So a Phone voice over, CW, the tune carrier, or the mic key held at
// the radio read "RX" in green on the bar while the cockpit header's ON AIR sign under it was red
// — two annunciators on one screen disagreeing about the transmitter. The #57 fix had moved the
// header's sign, the TX meters and the S-meter onto the arbiter (`txBusyReason`, the engine's
// `tx_owner()` answer, and `rigKeyed`, the rig's own PTT read back) and missed this plate.
//
// Display only: nothing here keys the rig, and nothing keys differently because of it.
import { describe, it, expect, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TopBar } from './TopBar'
import { CockpitHeader } from './CockpitHeader'
import type { AppSnapshot, RadioStatus } from '../types'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

/** Not keyed, every arbiter field present and saying so; each case sets what it keys with. */
const radio = (over: Partial<RadioStatus>) =>
  ({
    dialMhz: 14.2, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true,
    nextSlotMs: 0, txCycleAuto: true, txEven: true, txEnabled: false, txAllowed: true,
    transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    qsoRecording: false, catOk: true, dtSec: 0, clockOffsetMs: 0, ...over,
  }) as unknown as RadioStatus

/** Every way the transmitter gets keyed, as the snapshot reports it. The sentences are the
 *  engine's own (`TxOwner::busy_reason`). */
const KEYED: [string, Partial<RadioStatus>][] = [
  ['a Phone voice over', { txBusyReason: 'A voice message is transmitting — stop it first' }],
  ['the PTT button held in Nexus', { txBusyReason: 'Mic PTT is held — release it first' }],
  ['CW sending', { txBusyReason: 'CW is sending — stop it first' }],
  ['the tune carrier', { tuning: true, txBusyReason: 'Tune carrier is up — stop tuning first' }],
  ['the mic key held at the radio', { rigKeyed: true }],
  ['an FT over', { transmitting: true }],
]

/** Nothing on the air — including the two states that sit closest to it. */
const IDLE: [string, Partial<RadioStatus>][] = [
  ['nothing keyed', {}],
  ['transmit armed, nothing on the air yet', { txEnabled: true }],
  ['a station too old to send the arbiter fields', { txBusyReason: undefined, rigKeyed: undefined }],
]

/** The plate as App renders the bar over the Phone cockpit (no readout, no digital chrome). */
function plate(over: Partial<RadioStatus>): Element {
  const noop = () => {}
  const { container } = render(
    <TopBar
      mycall="KD9TAW" mygrid="EN52xa" radio={radio(over)} link={{ tier: 'FT8' } as never} bandPlan={[]}
      onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop}
      onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop}
      tier="FT8" onTierChange={noop} onOpenGuide={noop}
      hideFrequencyControl hideDigitalChrome
    />,
  )
  const el = container.querySelector('.txrx-indicator')
  expect(el, 'the TX/RX plate did not render').not.toBeNull()
  return el!
}

/** The cockpit header's sign as Phone and CW render it: no TX-enable latch, so the passive pill. */
function headerSign(over: Partial<RadioStatus>): Element {
  const snap = { radio: radio(over) } as unknown as AppSnapshot
  const { container } = render(<CockpitHeader snap={snap} modeIndicator={<span>SSB</span>} bandControl={<span>—</span>} />)
  const el = container.querySelector('.cockpit-txstate')
  expect(el, 'the header sign did not render').not.toBeNull()
  return el!
}

describe("the top bar's TX plate", () => {
  it.each(KEYED)('reads TX during %s', (_what, over) => {
    const el = plate(over)
    expect(el.classList.contains('tx'), `class "${el.className}"`).toBe(true)
    expect(el.classList.contains('rx')).toBe(false)
    expect(el.textContent).toBe('TX')
  })

  it.each(IDLE)('reads RX with %s', (_what, over) => {
    const el = plate(over)
    expect(el.classList.contains('rx'), `class "${el.className}"`).toBe(true)
    expect(el.classList.contains('tx')).toBe(false)
    expect(el.textContent).toBe('RX')
  })

  it.each([...KEYED, ...IDLE])("agrees with the cockpit header's ON AIR sign: %s", (_what, over) => {
    const header = headerSign(over).classList.contains('on')
    cleanup()
    const bar = plate(over).classList.contains('tx')
    expect(bar, `the header says ${header ? 'ON AIR' : 'not on the air'}, the top bar says ${bar ? 'TX' : 'RX'}`).toBe(header)
  })
})

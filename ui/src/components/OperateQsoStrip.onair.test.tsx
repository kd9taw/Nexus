// @vitest-environment jsdom
//
// OPERATE'S STRIP SAYS "▲ TRANSMITTING" WHENEVER THE TRANSMITTER IS ON THE AIR, WHOEVER KEYED IT.
//
// The strip is Operate's annunciator (its header draws no ON AIR pill), and it keyed on
// `radio.transmitting`, the FT slot flag. So Operate's own Tune button, a key held at the radio,
// or an over another screen started (a Phone voice over, a CW macro) keyed the rig while the strip
// said "▼ Receiving" in green and its red wash stayed off. It now follows `isOnAir()`, the answer
// the header's ON AIR sign, the top bar's plate and the meters show.
//
// Display only. Every reader of the strip's `tx` class and of that flag was checked first: the
// class is read by two paint rules (the wash, and the caption's ON AIR sign in styles.css) and by
// no code, and the flag decided only the class and the caption. No control in the strip is
// enabled, disabled or routed on it; Stop TX's authority is `useStationStopControl()` alone.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import { CockpitTxStrip } from './CockpitTxStrip'
import { t } from '../i18n'
import type { RadioStatus } from '../types'

afterEach(cleanup)

/** Not keyed, TX armed, every arbiter field present and saying so; each case sets what it keys. */
const radio = (over: Partial<RadioStatus>) =>
  ({
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true,
    txEnabled: true, transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    holdTxFreq: false, txEven: true, txCycleAuto: true, ...over,
  }) as unknown as RadioStatus

/** Every way the transmitter gets keyed while Operate is on screen, as the snapshot reports it.
 *  The sentences are the engine's own (`TxOwner::busy_reason`). */
const KEYED: [string, Partial<RadioStatus>][] = [
  ["Operate's own Tune", { tuning: true, txBusyReason: 'Tune carrier is up — stop tuning first' }],
  ['a key held at the radio', { rigKeyed: true }],
  ['a Phone voice over started from another screen', { txBusyReason: 'A voice message is transmitting — stop it first' }],
  ['a CW macro', { txBusyReason: 'CW is sending — stop it first' }],
  ['an FT over (as before)', { transmitting: true }],
]
const IDLE: [string, Partial<RadioStatus>, string][] = [
  ['nothing keyed, TX armed', {}, 'operate.strip.state.receiving'],
  ['nothing keyed, TX off', { txEnabled: false }, 'operate.strip.state.txOff'],
  ['a station too old to send the arbiter fields', { txBusyReason: undefined, rigKeyed: undefined }, 'operate.strip.state.receiving'],
]

function strip(over: Partial<RadioStatus>): { section: Element; caption: Element } {
  const noop = vi.fn()
  const { container } = render(
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
      onHaltTx={noop}
      onSetHoldTxFreq={noop}
    />,
  )
  const section = container.querySelector('.cockpit-qso')
  const caption = container.querySelector('.cq-statecap')
  expect(section, 'the strip did not render').not.toBeNull()
  expect(caption, 'the state caption did not render').not.toBeNull()
  return { section: section!, caption: caption! }
}

describe("Operate's strip follows the transmitter", () => {
  it.each(KEYED)('%s: ▲ TRANSMITTING and the red wash', (_what, over) => {
    const { section, caption } = strip(over)
    expect(section.classList.contains('tx'), `class "${section.className}"`).toBe(true)
    expect(section.classList.contains('rx')).toBe(false)
    expect(caption.textContent).toBe(t('operate.strip.state.transmitting'))
  })

  it.each(IDLE)('%s: no wash, and the caption says so', (_what, over, key) => {
    const { section, caption } = strip(over)
    expect(section.classList.contains('rx'), `class "${section.className}"`).toBe(true)
    expect(section.classList.contains('tx')).toBe(false)
    expect(caption.textContent).toBe(t(key as 'operate.strip.state.receiving'))
  })

  it.each([...KEYED, ...IDLE.map(([w, o]) => [w, o] as [string, Partial<RadioStatus>])])(
    "agrees with the cockpit header's ON AIR sign: %s",
    (_what, over) => {
      // Every other screen's sign is its TX strip's caption since operator batch 60 (the header's
      // pill moved there with the latch).
      const { container } = render(<CockpitTxStrip radio={radio(over)} onStopTx={() => {}} />)
      const header = container.querySelector('.cq-statecap')!.classList.contains('tx')
      cleanup()
      const onAir = strip(over).section.classList.contains('tx')
      expect(onAir, `the header says ${header ? 'ON AIR' : 'not on the air'}, the strip says ${onAir ? 'TRANSMITTING' : 'not'}`).toBe(header)
    },
  )
})

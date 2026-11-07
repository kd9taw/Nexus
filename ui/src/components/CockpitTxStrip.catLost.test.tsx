// @vitest-environment jsdom
//
// A LOST CAT LINK THAT TURNED TRANSMIT OFF IS READ IN THE TX STATE, WITHOUT HOVERING. The CAT status
// is a tooltip on the cockpits, and the top bar has no room for one more chip on FT at 1366 x 768:
// it takes a line, which moves Stop TX. The state sits after the cluster on FT's strip and on every
// other screen's, so it moves none of the controls. It follows the status the backend sends, so it
// comes and goes with it. The on-air sign and a connection that transmits nothing are said first.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import { CockpitTxStrip } from './CockpitTxStrip'
import { EN, installCatalog, setLocale } from '../i18n'
import { DE } from '../i18n/de'
import type { RadioStatus } from '../types'

const SENTENCE = EN['settings.rigControl.catSilentTxOff']
const LOST = { txEnabled: false, catOk: false, catDetail: `${SENTENCE} 14.0740 MHz CAT link unreachable (1/3)` }

afterEach(() => {
  cleanup()
  setLocale('en')
})

const radio = (over: Partial<RadioStatus>) =>
  ({
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true,
    txEnabled: true, transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    holdTxFreq: false, txEven: true, txCycleAuto: true, ...over,
  }) as unknown as RadioStatus

function ft(over: Partial<RadioStatus>): Element {
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
  return container.querySelector('.cq-statecap')!
}

function other(over: Partial<RadioStatus>): Element {
  const { container } = render(<CockpitTxStrip radio={radio(over)} onSetTxEnabled={vi.fn()} onStopTx={vi.fn()} />)
  return container.querySelector('.cq-statecap')!
}

describe.each([
  ["FT's strip", ft],
  ["every other screen's strip", other],
] as const)('%s', (_, state) => {
  it('says transmit is off, why and what to press, with the whole sentence on hover', () => {
    const s = state(LOST)
    expect(s.textContent).toBe('■ TX off — CAT lost: press TX On')
    expect(s.getAttribute('title')).toBe(SENTENCE)
  })

  it('in the operator\'s language', () => {
    installCatalog('de', DE)
    setLocale('de')
    const s = state(LOST)
    expect(s.textContent).toBe(DE['operate.strip.state.txOffCatLost'])
    expect(s.textContent).toContain('TX On')
    expect(s.getAttribute('title')).toBe(DE['settings.rigControl.catSilentTxOff'])
  })

  it('keeps explaining once CAT is back, in words for that, until transmit is back on', () => {
    const back = { txEnabled: false, catOk: true, catDetail: `${EN['settings.rigControl.catBackTxOff']} 14.074 MHz` }
    const s = state(back)
    expect(s.textContent).toBe('■ TX off — CAT is back: press TX On')
    expect(s.getAttribute('title')).toBe(EN['settings.rigControl.catBackTxOff'])
    cleanup()
    expect(state({ ...back, txEnabled: true }).textContent).toBe('▼ Receiving')
  })

  it('goes when the status does, and when transmit is back on', () => {
    expect(state({ txEnabled: false, catDetail: 'Connected — 14.074 MHz' }).textContent).toBe('■ TX off')
    cleanup()
    expect(state({ ...LOST, txEnabled: true }).textContent).toBe('▼ Receiving')
  })

  it('is second to the on-air sign and to a connection that transmits nothing', () => {
    expect(state({ ...LOST, rigKeyed: true }).textContent).toBe(EN['operate.strip.state.transmitting'])
    cleanup()
    const refused = state({ ...LOST, txRefusal: 'This connection is receive only' })
    expect(refused.textContent).toBe(EN['operate.strip.state.connectionNoTx'])
    expect(refused.getAttribute('title')).toBe('This connection is receive only')
  })
})

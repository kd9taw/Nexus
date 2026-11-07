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
const LOST = { txEnabled: false, catOk: false, catDetail: `14.0740 MHz CAT link unreachable (1/3) ${SENTENCE}` }

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
  return container.querySelector('.cockpit-qso')!
}

function other(over: Partial<RadioStatus>): Element {
  const { container } = render(<CockpitTxStrip radio={radio(over)} onSetTxEnabled={vi.fn()} onStopTx={vi.fn()} />)
  return container.querySelector('.cockpit-txstrip')!
}

/** What the strip says: its TX state, and the words beside it (null when there are none). */
const says = (strip: Element) => {
  const words = strip.querySelector('.cat-txoff-note')
  return {
    state: strip.querySelector('.cq-statecap')!.textContent,
    words: words?.textContent ?? null,
    hover: words?.getAttribute('title') ?? null,
  }
}

describe.each([
  ["FT's strip", ft],
  ["every other screen's strip", other],
] as const)('%s', (_, strip) => {
  it('keeps its TX state as it was, and says why and what to press beside it, with the whole sentence on hover', () => {
    expect(says(strip(LOST))).toEqual({ state: '■ TX off', words: 'CAT lost: press TX On', hover: SENTENCE })
  })

  it("in the operator's language", () => {
    installCatalog('de', DE)
    setLocale('de')
    expect(says(strip(LOST))).toEqual({
      state: DE['operate.strip.state.txOff'],
      words: DE['operate.strip.state.txOffCatLost'],
      hover: DE['settings.rigControl.catSilentTxOff'],
    })
  })

  it('keeps explaining once CAT is back, in words for that, until transmit is back on', () => {
    const back = { txEnabled: false, catOk: true, catDetail: `14.074 MHz ${EN['settings.rigControl.catBackTxOff']}` }
    expect(says(strip(back))).toEqual({
      state: '■ TX off',
      words: 'CAT is back: press TX On',
      hover: EN['settings.rigControl.catBackTxOff'],
    })
    cleanup()
    expect(says(strip({ ...back, txEnabled: true }))).toEqual({ state: '▼ Receiving', words: null, hover: null })
  })

  it('says nothing more for any other status', () => {
    expect(says(strip({ txEnabled: false, catDetail: 'Connected — 14.074 MHz' })).words).toBeNull()
  })

  it('is second to the on-air sign and to a connection that transmits nothing', () => {
    expect(says(strip({ ...LOST, rigKeyed: true }))).toEqual({
      state: EN['operate.strip.state.transmitting'], words: null, hover: null,
    })
    cleanup()
    expect(says(strip({ ...LOST, txRefusal: 'This connection is receive only' })).words).toBeNull()
  })
})

// @vitest-environment jsdom
//
// BESIDE TUNE while Tune is the Flex radio's own carrier: the radio's tune power, and its own
// transmit timeout, or, when it reports none, the warning the operator signed, word for word
// ("Warn only": Tune still starts). Drawn as the LAST child of each TX strip, so the controls keep
// their places; a station without it renders nothing new.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { RadioTuneNote } from './RadioTuneNote'
import { CockpitTxStrip } from './CockpitTxStrip'
import { OperateQsoStrip } from './OperateQsoStrip'
import type { RadioStatus } from '../types'

afterEach(cleanup)

/** The signed text's item 4, word for word. */
const SIGNED_WARNING =
  "The radio's transmit timeout is off. If Nexus or the network fails during a tune, nothing will " +
  'end the carrier. Set a transmit timeout in SmartSDR.'

/** Not keyed, TX armed (the strips' own tests' fixture), with `extra`. */
function radio(extra: Partial<RadioStatus>): RadioStatus {
  return {
    dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true,
    txEnabled: true, transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false,
    holdTxFreq: false, txEven: true, txCycleAuto: true, ...extra,
  } as unknown as RadioStatus
}

/** The note's text, or null when it renders nothing. */
function note(r: RadioStatus): string | null {
  const { container } = render(<RadioTuneNote radio={r} />)
  const text = container.querySelector('.radio-tune-note')?.textContent ?? null
  cleanup()
  return text
}

describe('the note beside Tune', () => {
  it('renders nothing on a station whose Tune is not the radio’s own carrier', () => {
    expect(note(radio({}))).toBeNull()
    expect(note(radio({ flexTune: null }))).toBeNull()
  })

  it('shows the radio’s tune power and its transmit timeout, in seconds or whole minutes', () => {
    expect(note(radio({ flexTune: { powerPct: 10, txTimeoutMs: 30_000 } }))).toBe(
      'Radio tune power 10 %Radio TX timeout 30 s',
    )
    expect(note(radio({ flexTune: { powerPct: 25, txTimeoutMs: 600_000 } }))).toBe(
      'Radio tune power 25 %Radio TX timeout 10 min',
    )
    expect(note(radio({ flexTune: { powerPct: 10, txTimeoutMs: 1_500 } }))).toBe(
      'Radio tune power 10 %Radio TX timeout 1.5 s',
    )
  })

  it('shows the signed warning, word for word, when the radio reports no timeout or none yet', () => {
    for (const txTimeoutMs of [0, null, undefined]) {
      render(<RadioTuneNote radio={radio({ flexTune: { powerPct: 10, txTimeoutMs } })} />)
      const el = screen.getByRole('note')
      expect(el.textContent).toBe(`Radio tune power 10 %${SIGNED_WARNING}`)
      expect(el.classList.contains('warn')).toBe(true)
      cleanup()
    }
    // The control: with a timeout reported, no warning and no warning style.
    render(<RadioTuneNote radio={radio({ flexTune: { powerPct: 10, txTimeoutMs: 30_000 } })} />)
    expect(screen.getByRole('note').textContent).not.toContain(SIGNED_WARNING)
    expect(screen.getByRole('note').classList.contains('warn')).toBe(false)
  })

  it('says why Tune keys nothing when the licence refuses the radio’s carrier here', () => {
    expect(
      note(radio({ flexTune: { powerPct: 10, txTimeoutMs: 30_000 }, flexTuneRefused: true })),
    ).toBe(
      'Radio tune power 10 %Radio TX timeout 30 s' +
        "Tune keys nothing here: the radio's own carrier would be outside your CW privileges on this frequency.",
    )
  })
})

describe('in the TX strips: the last child, after the controls and the TX state', () => {
  const tune = { flexTune: { powerPct: 10, txTimeoutMs: 0 } }

  it('the shared TX strip', () => {
    const { container } = render(
      <CockpitTxStrip radio={radio(tune)} onTune={() => {}} onStopTx={() => {}} />,
    )
    const strip = container.querySelector('.cockpit-txstrip')!
    expect(strip.lastElementChild?.classList.contains('radio-tune-note')).toBe(true)
    expect(strip.lastElementChild?.textContent).toBe(`Radio tune power 10 %${SIGNED_WARNING}`)
    // The controls are where they were: the cluster comes first, Tune and Stop TX in it.
    const cluster = strip.firstElementChild!
    expect(cluster.classList.contains('cq-txctl')).toBe(true)
    expect(screen.getByRole('button', { name: /^tune$/i }).closest('.cq-txctl')).toBe(cluster)
    expect(screen.getByRole('button', { name: /^stop tx$/i }).closest('.cq-txctl')).toBe(cluster)
    cleanup()
    // Without Tune on the screen there is nothing for it to sit beside.
    const bare = render(<CockpitTxStrip radio={radio(tune)} onStopTx={() => {}} />)
    expect(bare.container.querySelector('.radio-tune-note')).toBeNull()
  })

  it('Operate’s strip', () => {
    const noop = vi.fn()
    const { container } = render(
      <OperateQsoStrip
        qso={null}
        radio={radio(tune)}
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
    const strip = container.querySelector('.cockpit-qso')!
    expect(strip.lastElementChild?.classList.contains('radio-tune-note')).toBe(true)
    expect(strip.lastElementChild?.textContent).toBe(`Radio tune power 10 %${SIGNED_WARNING}`)
  })
})

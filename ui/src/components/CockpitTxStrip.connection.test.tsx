// @vitest-environment jsdom
//
// A CONNECTION THAT TRANSMITS NOTHING SAYS SO IN THE TX STRIP. The Icom network connection is
// receive and control only in its Beta: the engine refuses every transmission on it and names the
// connection (`radio.txRefusal`). The strip's state reads "No TX on this connection" with that
// reason on hover, and Tune, disabled like any lockout, gives the same reason, so nothing on screen
// blames the licence for it. The control: with no refusal the strip reads as it always has.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { CockpitTxStrip } from './CockpitTxStrip'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ haltTx: vi.fn(() => Promise.resolve(null)) }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

const REFUSED = 'Transmit is off on the Icom network connection (Beta: receive and control only)'

const radioWith = (over: Record<string, unknown> = {}) =>
  ({
    dialMhz: 14.074,
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: true,
    tuning: false,
    txAllowed: true,
    ...over,
  }) as unknown as AppSnapshot['radio']

afterEach(cleanup)

describe('the TX strip on a connection that transmits nothing', () => {
  it('names the connection, not the licence', () => {
    render(<CockpitTxStrip radio={radioWith({ txAllowed: false, txRefusal: REFUSED })} onTune={() => {}} />)
    const state = screen.getByText('No TX on this connection')
    expect(state.getAttribute('title')).toBe(REFUSED)
    const tune = screen.getByRole('button', { name: 'Tune' }) as HTMLButtonElement
    expect(tune.disabled).toBe(true)
    expect(tune.title).toBe(REFUSED)
  })

  it('reads as it always has with no refusal', () => {
    render(<CockpitTxStrip radio={radioWith()} onTune={() => {}} />)
    expect(screen.queryByText('No TX on this connection')).toBeNull()
    const tune = screen.getByRole('button', { name: 'Tune' }) as HTMLButtonElement
    expect(tune.disabled).toBe(false)
    expect(tune.title).toMatch(/steady carrier/)
  })

  it('still shows what the radio itself puts on the air', () => {
    render(
      <CockpitTxStrip
        radio={radioWith({ txAllowed: false, txRefusal: REFUSED, rigKeyed: true })}
        onTune={() => {}}
      />,
    )
    expect(screen.queryByText('No TX on this connection')).toBeNull()
  })
})

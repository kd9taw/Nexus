// @vitest-environment jsdom
//
// FT'S TX CLUSTER, IN FT'S ORDER, ON EVERY SCREEN (operator batch 60, 2026-10-01: "FT's strip
// under the scope in every mode, FT's order, staying in view; Stop TX at the same spot on all 8
// screens").
//
// It replaced #287's rule, which pinned Tune beside the band picker in the cockpit HEADER. That
// header wrapped, and everything in it moved with the wrap: N66 measured Stop TX in four places
// on the eight operating screens, Tune in seven, and the header Stop TX under the dock at the
// 150–175 % pins. Now the header draws no transmit control at all and the strip draws them all.
//
// What is pinned here, structurally (jsdom does not lay out — the same-spot geometry and the
// reach at every pin are measured in a real browser, recorded in the change's report):
//   · the order is FT's: the latch (button, or its read-only twin) · Tune · ATU · Stop TX
//     (· Hold Tx), and nothing renders between them;
//   · the latch slot is always there, so Tune and Stop TX sit at the same place whether a screen
//     arms TX here or not;
//   · the cockpit header carries none of the four any more (Tune's #287 slot is gone with them).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, screen } from '@testing-library/react'
import { CockpitHeader } from './CockpitHeader'
import { CockpitTxStrip } from './CockpitTxStrip'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({ setFrequency: vi.fn(() => Promise.resolve(null)), haltTx: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))
vi.mock('../useWheelTune', () => ({ useWheelTune: () => undefined }))

const snap = {
  radio: { dialMhz: 14.29, catOk: true, sideband: 'USB', transmitting: false, txEnabled: true, tuning: false, txAllowed: true, atu: true, holdTxFreq: false },
} as unknown as AppSnapshot

const cluster = (c: HTMLElement) => [...c.querySelector('.cq-txctl')!.children].map((e) => e.textContent!.trim())

afterEach(cleanup)

describe("the TX strip is FT's cluster", () => {
  it('a screen that arms TX here: TX On · Tune · ATU · Stop TX, in that order', () => {
    const { container } = render(
      <CockpitTxStrip radio={snap.radio} onSetTxEnabled={() => {}} onTune={() => {}} onAtuTune={() => {}} onStopTx={() => {}} />,
    )
    expect(cluster(container)).toEqual(['TX On', 'Tune', 'ATU', 'Stop TX'])
    expect(screen.getByRole('button', { name: 'TX On' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('Phone and CW (no latch here): the read-only twin holds the first slot, so Tune does not move', () => {
    const { container } = render(<CockpitTxStrip radio={snap.radio} onTune={() => {}} onAtuTune={() => {}} onStopTx={() => {}} />)
    expect(cluster(container)).toEqual(['TX On', 'Tune', 'ATU', 'Stop TX'])
    const first = container.querySelector('.cq-txctl')!.firstElementChild!
    expect(first.tagName, 'the read-only latch must not be a button').toBe('SPAN')
    expect(first.className).toContain('op-btn monitor')
    // CONTROL: the button form wears the same classes, which is what gives both one box.
    cleanup()
    const armed = render(<CockpitTxStrip radio={snap.radio} onSetTxEnabled={() => {}} onStopTx={() => {}} />).container
    expect(armed.querySelector('.cq-txctl')!.firstElementChild!.className).toContain('op-btn monitor')
  })

  it('a slot mode adds Hold Tx last, after Stop TX', () => {
    const { container } = render(
      <CockpitTxStrip radio={snap.radio} onSetTxEnabled={() => {}} onTune={() => {}} onAtuTune={() => {}} onStopTx={() => {}}
        hold={{ on: false, onChange: () => {} }} />,
    )
    expect(cluster(container)).toEqual(['TX On', 'Tune', 'ATU', 'Stop TX', 'Hold Tx'])
  })

  it('the TX state reads after the cluster, never inside it', () => {
    const { container } = render(<CockpitTxStrip radio={snap.radio} onStopTx={() => {}} />)
    const cap = container.querySelector('.cq-statecap')!
    expect(cap.closest('.cq-txctl')).toBeNull()
    expect(cap.textContent).toBe('▼ Receiving')
  })
})

describe('the cockpit header draws no transmit control', () => {
  it('no latch, Tune, ATU or Stop TX — whatever the cockpit passes', () => {
    const { container } = render(
      <CockpitHeader
        snap={snap}
        modeIndicator={<span>Phone</span>}
        bandControl={<span data-testid="band">20m</span>}
        frequencyExtras={<span>step</span>}
        actions={<button type="button">Depth</button>}
        power={{ value: 50, unit: '%', onChange: () => {} }}
      >
        <button type="button">Mode extra</button>
      </CockpitHeader>,
    )
    for (const name of [/^stop tx$/i, /^tune$|^tuning…$/i, /^atu$/i, /^(▼ |■ )?tx (on|off)$/i]) {
      expect(screen.queryByRole('button', { name }), `the header still draws ${name}`).toBeNull()
    }
    expect(container.querySelector('.ch-tune, .cockpit-txstate, .cockpit-stoptx')).toBeNull()
    // CONTROL: the header itself rendered — its own controls are there.
    expect(screen.getByRole('button', { name: 'Depth' })).toBeTruthy()
    expect(container.querySelector('.cockpit-cat')).not.toBeNull()
  })
})

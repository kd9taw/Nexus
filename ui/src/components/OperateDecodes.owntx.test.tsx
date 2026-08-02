// @vitest-environment jsdom
//
// Pins the load-bearing dependency of the promoted Rx-Frequency pane (claim (a) of the
// decode-first rebuild): the operator's OWN TX lines render interleaved in the 'rx'
// stream, visually distinct (WSJT-X yellow = the `mine own-tx` row classes), fed by the
// engine's own_tx → mine:true pipeline through the SAME filter path as received decodes.
// Building a second TX-line source is forbidden (double-row risk) — this test is the
// tripwire that the single shipped source keeps reaching the pane.
//
// NOTE: green on arrival by design — it pins shipped behaviour the rebuild PROMOTES,
// it does not introduce it.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow } from '../types'

afterEach(cleanup)

const mineRow: DecodeRow = {
  from: 'KD9TAW',
  snr: 0,
  dtSec: 0,
  freqHz: 1500,
  message: 'W1AW KD9TAW EN61',
  isCq: false,
  directedToMe: false,
  worked: false,
  mine: true,
  txAt: 1_722_500_000,
  tier: 'FT8',
  rv: -1,
}

const offFreqRow: DecodeRow = {
  from: 'JA1XYZ',
  snr: -12,
  dtSec: 0.1,
  freqHz: 2500, // > RX_TOL_HZ from the 1500 Hz marker, not directed to me
  message: 'CQ JA1XYZ PM95',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: -1,
}

describe("the Rx-Frequency stream ('rx' filter) shows the operator's own TX lines", () => {
  it('renders a mine row with the own-tx (WSJT-X yellow) classes; off-frequency rows filtered', () => {
    const { container } = render(
      <OperateDecodes
        decodes={[mineRow, offFreqRow]}
        slot={100}
        rxOffsetHz={1500}
        band="20m"
        tier="FT8"
        harqRescues={0}
        onCall={() => {}}
        lockedFilter="rx"
        compact
        title="Rx Frequency · 1500 Hz"
      />,
    )
    const own = container.querySelector('.decode-row.mine.own-tx')
    expect(own, 'no own-TX row rendered — the mine:true pipeline is not reaching the pane').not.toBeNull()
    expect(own!.textContent).toContain('W1AW KD9TAW EN61')
    // The 'rx' filter is deliberate WSJT-X semantics: mine || directedToMe || ±50 Hz.
    expect(container.textContent).not.toContain('JA1XYZ')
  })
})

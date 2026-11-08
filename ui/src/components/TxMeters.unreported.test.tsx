// @vitest-environment jsdom
//
// A METER THE RADIO DOES NOT REPORT says so with its ⊘ note in place of the empty bar and dash.
// Beside them the note was wider than Operate's half of its fixed telemetry cell, which clips, so
// on FT it was cut off (the geometry is the browser probe's: `scripts/browser-probe --desktop
// txmeters`). What jsdom can pin is the other half: a COMP row carrying Phone's controls keeps its
// track, because that track is the processor-level slider and the radio still takes the level.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const OFF = { rigKeyed: false, swrScaleVerified: true, compLevel: 0.35 } as unknown as RadioStatus
/** One over in which PO, ALC and SWR read and COMP stayed silent. */
const KEYED = { ...OFF, rigKeyed: true, txSwr: 1.2, txAlc: 0.3, txPoW: 50, txCompDb: null } as unknown as RadioStatus

function compRowAfterAnOver(props: { inline?: boolean; pinned?: boolean; controls?: object }) {
  const { container, rerender } = render(<TxMeters radio={KEYED} {...props} />)
  rerender(<TxMeters radio={OFF} {...props} />)
  const comp = [...container.querySelectorAll('.ph-txmeter')].find(
    (r) => r.querySelector('.ph-txmeter-label')?.textContent === 'COMP',
  ) as HTMLElement
  return {
    note: comp?.querySelector('.ph-unavail[role="note"]') != null,
    track: comp?.querySelector('.ph-txmeter-track') != null,
    slider: comp?.querySelector('[role="slider"]') != null,
    value: comp?.querySelector('.ph-txmeter-value') != null,
  }
}

describe('a meter the radio does not report', () => {
  it("in Operate's cell, shows the note alone, with no empty bar or dash beside it", () => {
    expect(compRowAfterAnOver({ inline: true })).toEqual({ note: true, track: false, slider: false, value: false })
  })

  it('in Phone, keeps the COMP level slider beside the note', () => {
    const r = compRowAfterAnOver({ pinned: true, controls: { onCompLevel: () => {} } })
    expect(r.note).toBe(true)
    expect(r.slider, 'the processor level can no longer be set').toBe(true)
  })

  // CONTROL: a meter that read during the over keeps its bar and value, and has no note.
  it('that DID read keeps its bar and value', () => {
    const { container, rerender } = render(<TxMeters radio={KEYED} inline />)
    rerender(<TxMeters radio={OFF} inline />)
    const po = [...container.querySelectorAll('.ph-txmeter')].find(
      (r) => r.querySelector('.ph-txmeter-label')?.textContent === 'PO',
    ) as HTMLElement
    expect(po.querySelector('.ph-txmeter-track')).not.toBeNull()
    expect(po.querySelector('.ph-txmeter-value')).not.toBeNull()
    expect(po.querySelector('.ph-unavail')).toBeNull()
  })
})

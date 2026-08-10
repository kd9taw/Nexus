// @vitest-environment jsdom
//
// THE TOP BAR'S TWO-ROW CONTRACT (ultrawide + snapped-window field reports,
// 2026-08-09). A single flex-wrap bar wrapped its TAIL group — Help/Light/Dark/
// Field — onto a lone, left-aligned second row, and the operator could not find
// the Field chip twice in one evening. The contract, computed from the RENDERED
// structure (the ui-layout rule — a regex on the stylesheet cannot see where a
// group landed):
//   · Row 1 always exists and ends with the chip cluster (theme switcher), so
//     Help/Light/Dark/Field sit top-right at every width, in every section.
//   · Row 2 exists ONLY in the digital views (the mode pills are slot-sync
//     furniture); CW/Phone/RTTY/SSTV/APRS get a single clean row — the Field
//     chip must be present there too, which is the operator's exact question.
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const radio = {
  dialMhz: 14.074,
  band: '20m',
  sideband: 'USB',
  rigMode: 'USB',
  rigConfirmed: true,
  nextSlotMs: 0,
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
} as unknown as RadioStatus

function renderBar(over: Record<string, unknown> = {}) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW"
      mygrid="EN52xa"
      radio={radio}
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
      theme="dark"
      onThemeChange={noop}
      onOpenGuide={noop}
      field={false}
      onFieldChange={noop}
      {...over}
    />,
  )
}

describe('the top bar rows', () => {
  it('row 1 ends with the chip cluster, and the pills get their own row (digital)', () => {
    const { container } = renderBar()
    const rows = container.querySelectorAll('.topbar-row')
    expect(rows.length).toBe(2)
    // The tail cluster — Help + theme switcher — is INSIDE row 1, its last group.
    const row1 = rows[0]
    expect(row1.querySelector('.theme-switcher'), 'chips live in row 1').not.toBeNull()
    const groups = row1.querySelectorAll(':scope > .topbar-group')
    expect(
      groups[groups.length - 1].querySelector('.theme-switcher'),
      'and they are its LAST group — top-right at every width',
    ).not.toBeNull()
    // The pills row holds the tier furniture, not the chips.
    const row2 = rows[1]
    expect(row2.querySelector('.tier-toggle')).not.toBeNull()
    expect(row2.querySelector('.theme-switcher')).toBeNull()
  })

  it('CW/Phone (no digital chrome): one row, Field chip still present', () => {
    const { container } = renderBar({ hideDigitalChrome: true, hideTxControls: true })
    const rows = container.querySelectorAll('.topbar-row')
    expect(rows.length, 'the pills row must not render empty').toBe(1)
    // The operator's question, answered by the DOM: the Field chip exists in the
    // non-digital cockpits too, in the same top-right cluster.
    const chip = screen.getByRole('button', { name: 'Field' })
    expect(rows[0].contains(chip)).toBe(true)
    expect(screen.getByRole('button', { name: 'Help' })).toBeTruthy()
  })

  it('the Field chip is absent only when the host does not wire it', () => {
    renderBar({ field: undefined, onFieldChange: undefined })
    expect(screen.queryByRole('button', { name: 'Field' })).toBeNull()
  })
})

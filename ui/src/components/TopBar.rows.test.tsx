// @vitest-environment jsdom
//
// THE TOP-BAR FORMAT CONTRACT (operator rulings, 2026-08-10, after seven layout
// rounds): the bar is the RELEASE organization — one flat flex-wrap header, no
// row wrappers — with the tier row left-packed in flow: mode pills, then the
// Help/OP/Field cluster ("option 1": in the space before the Tx-cycle group),
// then Auto/Tx 1st/Tx 2nd, slack collecting at the right edge. Light/Dark moved
// to Settings \u25b8 Appearance; the bar keeps only the FIELD quick toggle, which
// must exist in every view (outdoors is when Settings cannot be read).
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
      onOpenGuide={noop}
      field={false}
      onFieldChange={noop}
      {...over}
    />,
  )
}

describe('the top-bar release format', () => {
  it('is one flat header — no row wrappers — with pills \u2192 cluster \u2192 Tx cycle in flow', () => {
    const { container } = renderBar()
    expect(container.querySelector('.topbar-row'), 'the row experiment stays dead').toBeNull()
    expect(container.querySelector('.topbar-right'), 'and so does the right block').toBeNull()
    const header = container.querySelector('header.topbar')!
    const kids = [...header.children].map((el) => el.className)
    const iPills = kids.findIndex((c) => c.includes('tier-toggle') && !c.includes('tx-period'))
    const iChips = kids.findIndex((c) => c.includes('topbar-chips'))
    const iTx = kids.findIndex((c) => c.includes('tx-period'))
    expect(iPills, 'pills present').toBeGreaterThanOrEqual(0)
    expect(iChips, 'cluster present').toBeGreaterThan(iPills)
    expect(iTx, 'Tx cycle after the cluster (option 1)').toBeGreaterThan(iChips)
  })

  it('Light/Dark are OUT of the bar; Field remains', () => {
    renderBar()
    expect(screen.queryByRole('button', { name: 'Light' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Dark' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Field' })).toBeTruthy()
  })

  it('CW/Phone (no digital chrome): Field is still one tap away', () => {
    const { container } = renderBar({ hideDigitalChrome: true, hideTxControls: true })
    expect(container.querySelector('.tier-toggle')).toBeNull()
    expect(screen.getByRole('button', { name: 'Field' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Help' })).toBeTruthy()
  })

  it('the Field chip is absent only when the host does not wire it', () => {
    renderBar({ field: undefined, onFieldChange: undefined })
    expect(screen.queryByRole('button', { name: 'Field' })).toBeNull()
  })
})

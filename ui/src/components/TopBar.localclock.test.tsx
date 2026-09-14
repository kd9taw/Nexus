// @vitest-environment jsdom
//
// #253 — AN OPTIONAL LOCAL-TIME CLOCK BESIDE UTC (Settings ▸ Workspace, off by default).
//
// UTC stays the station clock: it is what logs, spots and slots use, so it never moves or goes
// away. The local clock is an extra, off unless the operator turns it on.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'
import { EN } from '../i18n'

const radio = {
  dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true, nextSlotMs: 0,
  txEven: true, txCycleAuto: true, txEnabled: false, txAllowed: true, transmitting: false,
  tuning: false, qsoRecording: false, catOk: true, dtSec: 0, timeSyncOk: true, clockOffsetMs: null,
} as unknown as RadioStatus

function renderBar(showLocalClock?: boolean) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW" mygrid="EN52xa" radio={radio} link={{ tier: 'FT8' } as never} bandPlan={[]}
      onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetTxEven={noop}
      onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} tier="FT8" onTierChange={noop} onOpenGuide={noop}
      showLocalClock={showLocalClock}
    />,
  )
}

// A fixed instant, so both clocks have something exact to say.
const INSTANT = new Date(Date.UTC(2026, 8, 14, 13, 5, 9))
const p = (n: number) => String(n).padStart(2, '0')

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] })
  vi.setSystemTime(INSTANT)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('#253 the local clock beside UTC', () => {
  it('is not shown by default, and UTC is', () => {
    const { container } = renderBar()
    expect(container.querySelector('.local-clock')).toBeNull()
    expect(container.querySelector('.utc-clock .utc-time')?.textContent).toBe('13:05:09')
  })

  it('shows this computer\'s local time when turned on, labelled as local, UTC unchanged', () => {
    const { container } = renderBar(true)
    const local = container.querySelector('.local-clock')
    expect(local, 'the local clock did not render').not.toBeNull()
    const expected = `${p(INSTANT.getHours())}:${p(INSTANT.getMinutes())}:${p(INSTANT.getSeconds())}`
    expect(local!.querySelector('.utc-time')?.textContent).toBe(expected)
    expect(local!.textContent).toContain(EN['topbar.localClock.label'])
    expect(container.querySelector('.utc-clock:not(.local-clock) .utc-time')?.textContent).toBe('13:05:09')
  })

  it('sits directly after the UTC clock', () => {
    const { container } = renderBar(true)
    const utc = container.querySelector('.utc-clock:not(.local-clock)')!
    expect(utc.nextElementSibling?.classList.contains('local-clock')).toBe(true)
  })
})

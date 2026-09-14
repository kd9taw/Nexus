// @vitest-environment jsdom
//
// #268 (and #235, closed into it) — THE RX FREQUENCY PANE AND −B4.
//
// Field reports: a worked station's RR73 vanished from the Rx Frequency pane, and a restart
// "fixed" it. −B4 hiding a just-worked station is by design; what was wrong is that the Rx
// Frequency pane had no −B4 chip of its own, and read the setting ONCE when it mounted — so
// turning −B4 off in Band Activity never reached it until Nexus restarted.
//
// Both panes are mounted side by side exactly as the cockpit does (the Rx pane locked to
// 'rx' and compact), because the defect lives BETWEEN two instances: each one on its own
// looks correct.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, fireEvent, cleanup, within } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import type { DecodeRow } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const RX_HZ = 1200

const decode = (over: Partial<DecodeRow> = {}): DecodeRow => ({
  from: 'W1AW',
  snr: -12,
  dtSec: 0.2,
  freqHz: RX_HZ,
  message: 'CQ W1AW FN31',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
  ...over,
})

// A worked station sitting on our RX frequency — the RR73 that disappeared.
const rows = [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31' }),
  decode({ from: 'PD2BS', message: 'KD9TAW PD2BS RR73', worked: true, isCq: false, freqHz: RX_HZ + 5 }),
]

function mountBoth() {
  const common = {
    decodes: rows,
    slot: 100,
    rxOffsetHz: RX_HZ,
    band: '20m',
    tier: 'FT8' as const,
    harqRescues: 0,
    onCall: () => {},
  }
  render(
    <>
      <OperateDecodes {...common} title="Band Activity" />
      <OperateDecodes {...common} title="Rx Frequency" lockedFilter="rx" compact hideExcludedCountries={false} />
    </>,
  )
  const [band, rx] = [...document.querySelectorAll('section.operate-decodes')] as HTMLElement[]
  return { band, rx }
}

const shows = (pane: HTMLElement, call: RegExp) => within(pane).queryByText(call) != null
const b4Chip = (pane: HTMLElement) => within(pane).queryByRole('button', { name: '−B4' })

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('−B4 in the Rx Frequency pane (#268)', () => {
  it('the Rx Frequency pane has a −B4 chip of its own', () => {
    const { rx } = mountBoth()
    expect(b4Chip(rx), 'no −B4 chip in the Rx Frequency pane').not.toBeNull()
  })

  it('turning −B4 OFF in Band Activity reaches the Rx Frequency pane live, no remount', () => {
    localStorage.setItem('nexus.decodes.hideB4', '1')
    const { band, rx } = mountBoth()
    // Both start hiding the worked station — the reported state.
    expect(shows(band, /PD2BS/)).toBe(false)
    expect(shows(rx, /PD2BS/)).toBe(false)
    expect(shows(rx, /W1AW/)).toBe(true) // the pane is not simply empty

    fireEvent.click(b4Chip(band)!)
    expect(shows(band, /PD2BS/)).toBe(true)
    expect(shows(rx, /PD2BS/), 'Rx Frequency still hiding after −B4 went off in Band Activity').toBe(true)
  })

  it('toggling −B4 in the Rx Frequency pane takes effect live in both panes', () => {
    const { band, rx } = mountBoth()
    expect(shows(rx, /PD2BS/)).toBe(true)
    fireEvent.click(b4Chip(rx)!)
    expect(b4Chip(rx)!.getAttribute('aria-pressed')).toBe('true')
    expect(shows(rx, /PD2BS/)).toBe(false)
    expect(shows(band, /PD2BS/)).toBe(false)
    expect(b4Chip(band)!.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(b4Chip(rx)!)
    expect(shows(rx, /PD2BS/)).toBe(true)
    expect(shows(band, /PD2BS/)).toBe(true)
  })
})

// @vitest-environment jsdom
//
// #268 (and #235, closed into it) — THE RX FREQUENCY PANE AND −B4.
//
// Field reports: a worked station's RR73 vanished from the Rx Frequency pane, and a restart
// "fixed" it. THIS file is the restart half: the Rx Frequency pane had no −B4 chip of its own
// and read the setting ONCE when it mounted, so turning −B4 off in Band Activity never reached
// it until Nexus restarted.
//
// The disappearance itself — "−B4 hiding a just-worked station is by design", as this comment
// used to say flatly — is `OperateDecodes.b4directed.test.tsx`. It is by design for a station
// calling CQ and it was never right for the partner's RR73, which is ADDRESSED TO US.
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

// A worked station sitting on our RX frequency — what −B4 is for, and what this file
// toggles the chip against.
//
// ⚠️ NOT the RR73 any more, and the swap is the point. `directedToMe` on the PD2BS line is
// what the engine really puts there (`Msg::parse("KD9TAW PD2BS RR73").addressee()` is us), and
// this fixture used to leave it at the helper's `false` — so the row it called "the RR73 that
// disappeared" was not the row the operator saw, and the file could not have noticed that a
// message addressed to us is exempt from −B4 (`OperateDecodes.b4directed.test.tsx`, the other
// half of #268). The chip-sync assertions below need a row −B4 really hides, so they use one:
// a worked station calling CQ at nobody in particular.
const rows = [
  decode({ from: 'W1AW', message: 'CQ W1AW FN31' }),
  decode({ from: 'G3XYZ', message: 'CQ G3XYZ IO91', worked: true, freqHz: RX_HZ + 5 }),
  decode({
    from: 'PD2BS',
    message: 'KD9TAW PD2BS RR73',
    worked: true,
    isCq: false,
    directedToMe: true,
    freqHz: RX_HZ + 5,
  }),
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
    expect(shows(band, /G3XYZ/)).toBe(false)
    expect(shows(rx, /G3XYZ/)).toBe(false)
    expect(shows(rx, /W1AW/)).toBe(true) // the pane is not simply empty

    fireEvent.click(b4Chip(band)!)
    expect(shows(band, /G3XYZ/)).toBe(true)
    expect(shows(rx, /G3XYZ/), 'Rx Frequency still hiding after −B4 went off in Band Activity').toBe(true)
  })

  it('toggling −B4 in the Rx Frequency pane takes effect live in both panes', () => {
    const { band, rx } = mountBoth()
    expect(shows(rx, /G3XYZ/)).toBe(true)
    fireEvent.click(b4Chip(rx)!)
    expect(b4Chip(rx)!.getAttribute('aria-pressed')).toBe('true')
    expect(shows(rx, /G3XYZ/)).toBe(false)
    expect(shows(band, /G3XYZ/)).toBe(false)
    expect(b4Chip(band)!.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(b4Chip(rx)!)
    expect(shows(rx, /G3XYZ/)).toBe(true)
    expect(shows(band, /G3XYZ/)).toBe(true)
  })
})

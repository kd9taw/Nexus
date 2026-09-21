// @vitest-environment jsdom
//
// #268 — A DECODE ADDRESSED TO ME MUST NOT VANISH UNDER −B4.
//
// bitslave's report: "Finalising QSO - other operator's RR73 disappears from Rx window. At
// the time, -B4 was switched on." The 1.13.0 work (`OperateDecodes.b4live.test.tsx`) fixed the
// half that needed a RESTART — the Rx Frequency pane read −B4 once at mount. It left the
// disappearance itself, and the disappearance is the report.
//
// THE MECHANISM, and it is why the row goes away WHILE THE OPERATOR IS LOOKING AT IT. The −B4
// filter has always exempted the station mid-QSO (`partnerCall`), so the partner's rows stay
// through the exchange. Both of the terms that keep the row visible expire at the same instant:
// the RR73 completes the contact, so `partnerCall` clears AND `log_qso` refreshes the worked
// index — and `recent_decodes` is rebuilt from scratch on every snapshot (engine.rs,
// `working_a_station_clears_the_icons_on_a_decode_already_on_screen`), so `worked` flips to true
// under a row that is already on screen. One 4 Hz poll later the terminal message of the QSO the
// operator just finished is gone from the pane whose job is to show that QSO.
//
// The retroactive REFRESH is deliberate and stays (that engine test is an operator ruling: chips
// clear when you work the station). What must not follow from it is a row being taken away: a
// message addressed to this operator is traffic for them, never "clutter from a station I have
// worked". Same class as the exemptions already beside it — `!d.mine` (don't hide my own echo)
// and the partner check.
//
// Both panes are mounted as the cockpit does, because −B4 is shared state between them.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, within } from '@testing-library/react'
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

// The state one poll AFTER the QSO completed: the contact is logged (`worked`), so the
// sequencer has let go of the partner (`partnerCall` is undefined below) and the flags were
// re-asked. `directedToMe` is what the engine really puts on this line —
// `Msg::parse("PD2BS RR73")`'s addressee is us (engine.rs, `directed_to_me`).
const rr73 = decode({
  from: 'PD2BS',
  message: 'KD9TAW PD2BS RR73',
  directedToMe: true,
  worked: true,
  isCq: false,
  freqHz: RX_HZ + 5,
})
// A worked station who is NOT talking to us — the row −B4 exists to remove. Its presence is
// the control: if it were also showing, the assertion below would pass with −B4 doing nothing.
const otherWorked = decode({ from: 'G3XYZ', message: 'CQ G3XYZ IO91', worked: true, freqHz: RX_HZ })

function mountBoth(rows: DecodeRow[]) {
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
      <OperateDecodes
        {...common}
        title="Rx Frequency"
        lockedFilter="rx"
        compact
        hideExcludedCountries={false}
      />
    </>,
  )
  const [band, rx] = [...document.querySelectorAll('section.operate-decodes')] as HTMLElement[]
  return { band, rx }
}

const shows = (pane: HTMLElement, call: RegExp) => within(pane).queryByText(call) != null

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('−B4 and a decode addressed to me (#268)', () => {
  it('keeps the partner RR73 that just finished the QSO, in the Rx Frequency pane', () => {
    localStorage.setItem('nexus.decodes.hideB4', '1')
    const { rx } = mountBoth([otherWorked, rr73])
    expect(shows(rx, /G3XYZ/), 'control: −B4 is really hiding an ordinary worked station').toBe(
      false,
    )
    expect(shows(rx, /PD2BS/), 'the RR73 that ended our QSO vanished from Rx Frequency').toBe(true)
  })

  it('keeps it in Band Activity too — the same filter, the same reason', () => {
    localStorage.setItem('nexus.decodes.hideB4', '1')
    const { band } = mountBoth([otherWorked, rr73])
    expect(shows(band, /G3XYZ/), 'control: −B4 is really hiding an ordinary worked station').toBe(
      false,
    )
    expect(shows(band, /PD2BS/)).toBe(true)
  })

  it('a worked station calling US by name is not hidden either', () => {
    // The generalisation the RR73 is one case of: −B4 declutters stations I COULD work and
    // already have. A station putting my callsign on the air is not that, whatever the
    // logbook says, and missing it is the same self-own as hiding my own echo.
    localStorage.setItem('nexus.decodes.hideB4', '1')
    const calling = decode({
      from: 'PD2BS',
      message: 'KD9TAW PD2BS -07',
      directedToMe: true,
      worked: true,
      isCq: false,
    })
    const { rx } = mountBoth([otherWorked, calling])
    expect(shows(rx, /G3XYZ/)).toBe(false)
    expect(shows(rx, /PD2BS/)).toBe(true)
  })

  it('−B4 still hides a worked station that is only calling CQ', () => {
    // The filter has to keep working, or the fix above is just "−B4 off".
    localStorage.setItem('nexus.decodes.hideB4', '1')
    const { rx, band } = mountBoth([otherWorked])
    expect(shows(rx, /G3XYZ/)).toBe(false)
    expect(shows(band, /G3XYZ/)).toBe(false)
  })
})

// @vitest-environment jsdom
//
// ⭐ #191 — THE SWITCH THAT FIXES THIS EXISTS, AND THE OPERATOR LOOKED TWICE AND MISSED IT.
//
// F4CYH, IC-7300 + IC-7100, 17 comments: "nexus switch to USB mode when i select the SSTV tab
// … i must switch manually to USB-D to have the sound out of icoms, and, when tx is over (or if
// i stop it manually), the icom are back in USB mode … it would be better to leave in data mode
// as for ft". A second operator on Discord the same week: "manually switches to data-u every
// time".
//
// 1.13.0 shipped exactly that control — "Hold the data mode while SSTV is receiving", per
// radio. It lives in Settings ▸ Radio ▸ Rig & CAT ▸ **Advanced**, a COLLAPSED disclosure, and
// the reporter had already failed to find a different switch in that same group:
//
//   maintainer — "'Advanced' is a collapsed section — if you do not click it open there is
//                 nothing on the screen to find, which is exactly why you looked twice and
//                 came up empty."
//   reporter   — "it's on me also, i did not notice the arrow for 'advanced' to expand the view"
//
// So this is not a missing capability. Every observation in that thread was made in the SSTV
// cockpit, watching the radio — that is where they were looking, and that is where the pointer
// goes. It carries no copy of the setting: it is a link, and `SettingsGroup` opens the
// disclosure it lands on (the Issue #62 mechanism), so the switch is ON SCREEN on arrival.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor, cleanup, fireEvent } from '@testing-library/react'
import { SstvView } from './SstvView'
import { resolveTarget, searchSettings } from '../settings/registry'
import * as api from '../api'
import type { AppSnapshot, BandChannel, SstvHealth, SstvState } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  setOperatingMode: vi.fn(),
}))
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))
vi.mock('../announce', () => ({ announce: vi.fn() }))

const getSstvState = api.getSstvState as ReturnType<typeof vi.fn>
const sstvAutoArm = api.sstvAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>

const NO_HEALTH: SstvHealth = {
  armed: false,
  audioPeak: 0,
  lastAudioUnix: null,
  drains: 0,
  visSeen: 0,
  lastVisUnix: null,
  unknownVis: 0,
  lastUnknownVisCode: null,
  lastUnknownVisUnix: null,
  images: 0,
  lastImageUnix: null,
}

const IDLE: SstvState = {
  armed: false,
  mode: null,
  linesDone: 0,
  linesTotal: 0,
  previewRgbBase64: null,
  previewWidth: 0,
  previewHeight: 0,
  hedrShiftHz: 0,
  gallery: [],
  health: NO_HEALTH,
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
}

/** The reporter's dial: 14.230, the HF calling channel the hold did nothing for until #191. */
const snap = {
  radio: {
    dialMhz: 14.23,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    tuning: false,
    txAllowed: true,
  },
} as unknown as AppSnapshot

const PLAN: BandChannel[] = [
  { band: '20m', group: 'HF', dialMhz: 14.23, mode: 'USB', label: '20 m · SSTV', note: '' },
]

/** An armed receiver hearing the band — the state the view opens in, and the state the hold is
 *  about ("for the whole time the SSTV receiver is running"). */
function armedHearing(): SstvState {
  return {
    ...IDLE,
    armed: true,
    health: {
      ...NO_HEALTH,
      armed: true,
      audioPeak: 0.4,
      drains: 500,
      lastAudioUnix: Math.floor(Date.now() / 1000),
    },
  }
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(armedHearing())
  sstvAutoArm.mockReset().mockResolvedValue(armedHearing())
  getLicensedBandPlan.mockReset().mockResolvedValue(PLAN)
})
afterEach(cleanup)

/** The pointer, by its accessible name — never by class, so a rename of the one the operator
 *  reads is a failure here rather than a silent pass. */
const signpost = () => screen.queryByRole('button', { name: /hold the data mode/i })

describe('the SSTV view points at the switch that holds the data mode', () => {
  it("offers it while the receiver runs and the hold is off — the reporter's exact state", async () => {
    render(
      <SstvView snap={snap} onOpenSettings={vi.fn()} holdDataSubmode={false} />,
    )
    await waitFor(() => expect(signpost()).toBeTruthy())
  })

  it('says what the radio is doing, in the words the report used', async () => {
    const { container } = render(
      <SstvView snap={snap} onOpenSettings={vi.fn()} holdDataSubmode={false} />,
    )
    await waitFor(() => expect(signpost()).toBeTruthy())
    const text = container.querySelector('.sstv-datamode-hint')?.textContent ?? ''
    // "the icom are back in USB mode" / "i must switch manually to USB-D". The line has to
    // describe the SYMPTOM, or an operator cannot tell it is about the thing bothering them.
    expect(text).toMatch(/plain USB/i)
    expect(text).toMatch(/data mode/i)
  })

  it('lands on the collapsed Advanced group the switch is actually inside', async () => {
    const onOpenSettings = vi.fn()
    render(
      <SstvView snap={snap} onOpenSettings={onOpenSettings} holdDataSubmode={false} />,
    )
    await waitFor(() => expect(signpost()).toBeTruthy())
    fireEvent.click(signpost() as HTMLElement)
    expect(onOpenSettings).toHaveBeenCalledWith('rig-advanced')
    // …and that target is a real one. A pointer at a section id that no longer resolves is the
    // dead-end this whole mechanism exists to stop, and it fails silently.
    expect(resolveTarget('rig-advanced')).toEqual({ tab: 'radio', section: 'rig-advanced' })
  })

  it('POSITIVE CONTROL: an operator who already turned it on is not nagged', async () => {
    render(<SstvView snap={snap} onOpenSettings={vi.fn()} holdDataSubmode />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalled())
    expect(signpost()).toBeNull()
  })

  it('POSITIVE CONTROL: it says nothing while the receiver is stopped', async () => {
    // The hold only acts while the receiver runs, so advertising it against a stopped receiver
    // would promise something the switch does not do — the reporter's other ask, still open.
    getSstvState.mockResolvedValue(IDLE)
    sstvAutoArm.mockResolvedValue(IDLE)
    render(
      <SstvView snap={snap} active={false} onOpenSettings={vi.fn()} holdDataSubmode={false} />,
    )
    await new Promise((r) => setTimeout(r, 20))
    expect(signpost()).toBeNull()
  })

  it('POSITIVE CONTROL: a host that cannot open Settings renders no dead link', async () => {
    render(<SstvView snap={snap} holdDataSubmode={false} />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalled())
    expect(signpost()).toBeNull()
  })
})

describe('the search box answers the words this report was written in', () => {
  // The signpost above catches the operator who is looking at the radio. The search box is for
  // the one who has gone to Settings already — the state #62 produced this mechanism for, and
  // the state this reporter was in when the route they were handed came up empty.
  //
  // ⚠️ MEASURED, NOT GUESSED. Most of the thread's vocabulary already landed on `rig-advanced`
  // ("usb-d", "data mode", "sstv usb", "hold data mode"); only the cases below returned
  // NOTHING AT ALL, and only those were added. A keyword for a word that already works is a
  // keyword nothing is holding down.
  const ids = (q: string) => searchSettings(q).map((h) => h.section.id)

  it('finds it by the other sideband the reporter named', () => {
    // "pressing send without touching the radio switch in USB-D (or LSB-D if 80m)". `usb-d`
    // was a keyword and `lsb-d` was not, which is an asymmetry no operator can see.
    expect(ids('lsb-d')).toContain('rig-advanced')
  })

  it('finds it by the SYMPTOM, which is what an operator actually types', () => {
    // "the icom are back in USB mode". Nobody searches for the name of a switch they have
    // never seen; they search for what the radio just did. The FM half of this section already
    // carries its reporter's symptom words ("reverts to FM"); these are the HF twins.
    expect(ids('back to usb')).toContain('rig-advanced')
    expect(ids('stays in usb')).toContain('rig-advanced')
  })

  it('POSITIVE CONTROL: a word in no section still answers nothing', () => {
    // Keywords are cheap and a section that matches everything is a section that means
    // nothing — this is what stops the entries above being widened until they always hit.
    expect(searchSettings('zzzznotasetting')).toEqual([])
    expect(ids('rotator')).not.toContain('rig-advanced')
  })
})

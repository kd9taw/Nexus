// @vitest-environment jsdom
//
// ⭐ WHAT THE SSTV RECEIVER SAYS IT IS HEARING (field bug, 2026-08-01).
//
// Operator report, FTdx10 on 14.236 then 14.230: "I hear a signal but the SSTV is
// not decoding … not working or decoding as the image comes in." Two things were
// true at once and neither was visible:
//
//   1. The receiver was never started. Arming was a manual, session-only,
//      default-off button, so the ordinary way to use SSTV — open the view, tune,
//      wait — fed the decoder nothing. The quoted hint text is itself the proof:
//      'Tune 14.230 / 145.800 — images decode here' rendered ONLY on the disarmed
//      branch.
//   2. Nothing on the screen could have told them. A stopped receiver, a dead
//      capture device, a silent input and an unsupported mode all rendered that
//      same sentence — which also named two frequencies while they sat on a third.
//
// These cases pin the fix at the view boundary: entering the view starts the
// receiver, and the caption states what is actually being heard.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, waitFor, cleanup } from '@testing-library/react'
import { SstvView, sstvDecodeStatus, sstvChannelForDial } from './SstvView'
import * as api from '../api'
import { announce as announceFn } from '../announce'
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

const announce = vi.mocked(announceFn)
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

/** The operator's actual dial in the field report: the 20 m overflow channel. */
const snap = {
  radio: {
    dialMhz: 14.236,
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
  { band: '20m-alt2', group: 'HF', dialMhz: 14.236, mode: 'USB', label: '20 m · SSTV (alt 2)', note: '' },
  { band: '2m', group: 'VHF', dialMhz: 145.8, mode: 'FM', label: '2 m · ISS downlink', note: '' },
]

/** A fixed "now" for the pure-ladder cases. The stamps below are all relative to
 * it: the ladder judges ages, so a fixture with an ancient timestamp would read as
 * a dead capture device no matter what else it said. */
const NOW = 1_754_000_000

/** Health of a receiver that is armed and hearing a real signal, as of `NOW`. */
const HEARING: SstvHealth = {
  ...NO_HEALTH,
  armed: true,
  audioPeak: 0.4,
  lastAudioUnix: NOW,
  drains: 500,
}

/** The same, but stamped against the real wall clock — the component reads
 * `Date.now()`, so a component-level fixture has to live in the present. */
function hearingNow(over: Partial<SstvHealth> = {}): SstvHealth {
  return { ...HEARING, lastAudioUnix: Math.floor(Date.now() / 1000), ...over }
}

/** The caption element under test (never the header's frequency readout). */
function caption(): HTMLElement {
  const el = document.querySelector('.sstv-band-caption')
  if (!el) throw new Error('no idle caption rendered')
  return el as HTMLElement
}

beforeEach(() => {
  getSstvState.mockReset().mockResolvedValue(IDLE)
  sstvAutoArm.mockReset().mockResolvedValue(IDLE)
  getLicensedBandPlan.mockReset().mockResolvedValue(PLAN)
  announce.mockReset()
})
afterEach(cleanup)

describe('opening the SSTV view starts the receiver', () => {
  it('calls sstv_auto_arm on entry — the operator never has to know to arm', async () => {
    render(<SstvView snap={snap} />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalled())
  })

  it('does it once per entry, not once per render', async () => {
    const { rerender } = render(<SstvView snap={snap} active />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalledTimes(1))
    rerender(<SstvView snap={snap} active />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalledTimes(1))
  })

  it('does not touch the receiver while the view is hidden (the host keeps it mounted)', async () => {
    render(<SstvView snap={snap} active={false} />)
    await new Promise((r) => setTimeout(r, 10))
    expect(sstvAutoArm).not.toHaveBeenCalled()
  })

  it('re-arms on the next entry, so a session-long visit does not go deaf', async () => {
    const { rerender } = render(<SstvView snap={snap} active />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalledTimes(1))
    rerender(<SstvView snap={snap} active={false} />)
    rerender(<SstvView snap={snap} active />)
    await waitFor(() => expect(sstvAutoArm).toHaveBeenCalledTimes(2))
  })
})

describe('the idle caption states what is being heard', () => {
  it('names the frequency for the band the radio is ON, not a hardcoded pair', async () => {
    getSstvState.mockResolvedValue({ ...IDLE, armed: true, health: hearingNow() })
    render(<SstvView snap={snap} />)
    // The operator was on 14.236 while the old hint recited "14.230 / 145.800".
    await waitFor(() => expect(caption().textContent).toMatch(/14\.236/))
    expect(caption().textContent).not.toMatch(/145\.800/)
  })

  it('says so when audio is reaching the decoder — the one thing the old hint never could', async () => {
    getSstvState.mockResolvedValue({ ...IDLE, armed: true, health: hearingNow() })
    render(<SstvView snap={snap} />)
    await waitFor(() => expect(caption().textContent).toMatch(/hearing audio/i))
    expect(caption().textContent).toMatch(/no SSTV header yet/i)
    expect(caption().className).toContain('rx-listening')
  })

  // ⭐ THE STATE THE VIEW OPENS IN. Arming resets the health and the effect re-reads
  // it in the same breath, so this shape — armed, no drains, no stamp — is what the
  // screen renders on every single entry. It used to render the `silent` copy, which
  // tells the operator their sound card is on the wrong input before the decode
  // thread has looked even once.
  it('⭐ does not accuse the sound card before the decoder has reported anything', async () => {
    getSstvState.mockResolvedValue({
      ...IDLE,
      armed: true,
      health: { ...NO_HEALTH, armed: true },
    })
    render(<SstvView snap={snap} />)
    await waitFor(() => expect(caption().className).toContain('rx-starting'))
    expect(caption().textContent).not.toMatch(/Audio input/)
  })

  it('⭐ names an unsupported mode instead of looking like a dead band', async () => {
    const now = Math.floor(Date.now() / 1000)
    getSstvState.mockResolvedValue({
      ...IDLE,
      armed: true,
      health: hearingNow({ unknownVis: 1, lastUnknownVisCode: 0x37, lastUnknownVisUnix: now }),
    })
    render(<SstvView snap={snap} />)
    await waitFor(() => expect(caption().textContent).toMatch(/cannot decode/i))
    expect(caption().className).toContain('rx-unsupported')
  })
})

// ⭐ THE CAPTION IS SPOKEN ON CHANGE, NOT LIVE-REGIONED. It restates the age of the
// last picture every second, so `role="status"` on it made a screen reader read the
// whole paragraph out again ~90 times after one image. a11y is always-on here, so the
// answer is to speak the TRANSITION once — not to go silent, and not to nag.
describe('what a screen reader hears', () => {
  beforeEach(() => vi.useFakeTimers({ shouldAdvanceTime: true }))
  afterEach(() => vi.useRealTimers())

  async function settle() {
    await act(async () => {
      for (let i = 0; i < 6; i++) await Promise.resolve()
    })
  }
  /** One 1 Hz poll tick with a new health reading. */
  async function poll(health: SstvHealth) {
    getSstvState.mockResolvedValue({ ...IDLE, armed: true, health })
    await act(async () => {
      vi.advanceTimersByTime(1000)
    })
    await settle()
  }

  it('⭐ announces the change once and never repeats it while the age ticks', async () => {
    getSstvState.mockResolvedValue({ ...IDLE, armed: true, health: hearingNow() })
    render(<SstvView snap={snap} />)
    await settle()

    // The caption itself must NOT be a live region — that is what repeated.
    expect(caption().getAttribute('role')).toBeNull()
    expect(caption().getAttribute('aria-live')).toBeNull()
    // And the state you walked into is not news.
    expect(announce).not.toHaveBeenCalled()

    await poll(
      hearingNow({
        unknownVis: 1,
        lastUnknownVisCode: 0x37,
        lastUnknownVisUnix: Math.floor(Date.now() / 1000),
      }),
    )
    expect(announce).toHaveBeenCalledTimes(1)
    expect(String(announce.mock.calls[0][0])).toMatch(/cannot decode/i)

    // Four more seconds of the same state: the caption re-renders (the age clock) and
    // the reader must stay quiet.
    for (let i = 0; i < 4; i++) {
      await poll(
        hearingNow({
          unknownVis: 1,
          lastUnknownVisCode: 0x37,
          lastUnknownVisUnix: Math.floor(Date.now() / 1000) - 1 - i,
        }),
      )
    }
    expect(announce).toHaveBeenCalledTimes(1)
  })
})

describe('sstvDecodeStatus — the ladder', () => {
  const ch20 = PLAN[1]

  it('a stopped receiver says it is stopped, and how to start it', () => {
    const s = sstvDecodeStatus({ ...NO_HEALTH, armed: false }, NOW, ch20)
    expect(s.state).toBe('off')
    expect(s.text).toMatch(/stopped/i)
    expect(s.text).toMatch(/Arm/)
  })

  it('no state at all (the poll has not answered) is NOT reported as a healthy idle', () => {
    expect(sstvDecodeStatus(null, NOW, ch20).state).toBe('off')
  })

  it('audio arriving at a real level with no header is the healthy idle state', () => {
    const s = sstvDecodeStatus(HEARING, NOW + 5, ch20)
    expect(s.state).toBe('listening')
    expect(s.text).toMatch(/14\.236 USB/)
  })

  it('samples arriving but silent is a routing/level fault, not a dead band', () => {
    const s = sstvDecodeStatus({ ...HEARING, audioPeak: 0 }, NOW + 5, ch20)
    expect(s.state).toBe('silent')
    expect(s.text).toMatch(/Audio input/)
  })

  it('nothing arriving at all is the capture fault — and it says the speaker proves nothing', () => {
    const s = sstvDecodeStatus({ ...HEARING, lastAudioUnix: null, drains: 500 }, NOW + 5, ch20)
    expect(s.state).toBe('nocapture')
    expect(s.text).toMatch(/speaker/)
  })

  it('waits a few drains before crying capture-fault (arming resets the health)', () => {
    const s = sstvDecodeStatus({ ...HEARING, lastAudioUnix: null, drains: 2 }, NOW + 5, ch20)
    expect(s.state).not.toBe('nocapture')
  })

  // ⭐ AND WHAT IT SAYS INSTEAD MUST NOT BE AN ACCUSATION. Arming resets the health
  // and the view re-reads it immediately, so this — no drains, no stamp, zero peak —
  // is the state on EVERY entry to the view. It used to fall through to `silent`,
  // which tells the operator their sound card is on the wrong input. It is also
  // permanent in the one case where the decode thread never drains at all.
  it('⭐ having no reading yet is its own state, not a fault it cannot prove', () => {
    const fresh = { ...NO_HEALTH, armed: true } // exactly what sstv_auto_arm leaves
    const s = sstvDecodeStatus(fresh, NOW, ch20)
    expect(s.state).toBe('starting')
    expect(s.text).not.toMatch(/Audio input/)
    expect(s.text).not.toMatch(/speaker/)
  })

  it('still calls silence silence once audio has actually arrived', () => {
    const s = sstvDecodeStatus({ ...HEARING, audioPeak: 0, drains: 3 }, NOW + 1, ch20)
    expect(s.state).toBe('silent')
  })

  it('⭐ an unsupported mode is NAMED — it used to be a console line nobody could see', () => {
    const s = sstvDecodeStatus(
      { ...HEARING, unknownVis: 1, lastUnknownVisCode: 0x37, lastUnknownVisUnix: NOW - 10 },
      NOW + 5,
      ch20,
    )
    expect(s.state).toBe('unsupported')
    expect(s.text).toMatch(/0x37/)
    // And it must not read as a fault in the operator's station.
    expect(s.text).toMatch(/audio path are fine/i)
  })

  it('an old unsupported-mode burst stops being news', () => {
    // Audio is still arriving NOW; only the burst is old. Without that the capture
    // ladder above would answer first and the case would prove nothing.
    const s = sstvDecodeStatus(
      {
        ...HEARING,
        lastAudioUnix: NOW + 600,
        unknownVis: 1,
        lastUnknownVisCode: 0x37,
        lastUnknownVisUnix: NOW,
      },
      NOW + 600,
      ch20,
    )
    expect(s.state).toBe('listening')
  })

  it('a decoded picture latches — it is a durable fact about the whole chain', () => {
    const s = sstvDecodeStatus({ ...HEARING, images: 2, lastImageUnix: NOW - 10 }, NOW + 5, ch20)
    expect(s.state).toBe('decoded')
    expect(s.text).toMatch(/2 images decoded/)
  })

  it('says nothing about a frequency it has no channel for', () => {
    const s = sstvDecodeStatus({ ...NO_HEALTH, armed: false }, NOW, null)
    expect(s.text).not.toMatch(/\d+\.\d{3}/)
  })
})

describe('sstvChannelForDial', () => {
  it('picks the channel for the band the radio is on, nearest the dial', () => {
    expect(sstvChannelForDial(PLAN, 14.236)?.dialMhz).toBe(14.236)
    expect(sstvChannelForDial(PLAN, 14.231)?.dialMhz).toBe(14.23)
    expect(sstvChannelForDial(PLAN, 145.8)?.dialMhz).toBe(145.8)
  })

  it('returns null rather than guessing when the band has no SSTV channel', () => {
    expect(sstvChannelForDial(PLAN, 7.171)).toBeNull()
    expect(sstvChannelForDial([], 14.23)).toBeNull()
    expect(sstvChannelForDial(PLAN, undefined)).toBeNull()
  })
})

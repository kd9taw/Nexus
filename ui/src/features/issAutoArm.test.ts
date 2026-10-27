import { describe, it, expect, vi, beforeEach } from 'vitest'

// The module only touches '../toast' at runtime (SatPass is a type). Mock it so
// no window/timer shim is needed.
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
import { pushToast } from '../toast'
const toasts = vi.mocked(pushToast)

import { tickIssAutoArm, resetIssAutoArm, type IssRadio } from './issAutoArm'
import { t } from '../i18n'
import type { SatPass } from '../types'

function pass(aosSecs: number, minutes = 10): SatPass {
  return {
    name: 'ISS',
    aosUnix: aosSecs,
    losUnix: aosSecs + minutes * 60,
    maxElDeg: 45,
    aosAzDeg: 200,
    losAzDeg: 40,
  }
}

const OFF_FREQ: IssRadio = { dialMhz: 14.074, band: '20m', sideband: 'USB' }
const ON_CHANNEL: IssRadio = { dialMhz: 145.8, band: '2m', sideband: 'FM' }

function deps() {
  return {
    tuneChannel: vi.fn(),
    setFrequency: vi.fn(),
    sstvArm: vi.fn(),
    sstvAutoDisarm: vi.fn(),
  }
}

beforeEach(() => {
  resetIssAutoArm()
  toasts.mockClear()
})

describe('tickIssAutoArm', () => {
  it('arms once at AOS: saves the dial, tunes 145.800 FM, arms SSTV', () => {
    const d = deps()
    const now = 1_900_000_000
    // Rose 1 min ago, still up — an in-progress pass.
    tickIssAutoArm(pass(now - 60), OFF_FREQ, true, d, now, 'sstv')
    // ⭐ tuneChannel, NOT setFrequency: 145.800 is the APP's channel. Arriving as an
    // operator tune it would be banked as the operator's 2 m dial by the per-(band, mode)
    // dial memory the moment they switched modes inside the armed window.
    expect(d.tuneChannel).toHaveBeenCalledTimes(1)
    expect(d.tuneChannel).toHaveBeenCalledWith(145.8, '2m', 'FM')
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).toHaveBeenCalledTimes(1)
    expect(d.sstvArm).toHaveBeenCalledWith(true)
    expect(String(toasts.mock.calls[0][0])).toContain('145.800 FM')
  })

  it('is idempotent within a pass — a second tick does nothing', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now, 'sstv')
    d.tuneChannel.mockClear()
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    d.sstvAutoDisarm.mockClear()
    // 30 s later, still in the same pass, rig now on-channel — must NOT re-arm.
    tickIssAutoArm(p, ON_CHANNEL, true, d, now + 30, 'sstv')
    expect(d.tuneChannel).not.toHaveBeenCalled()
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).not.toHaveBeenCalled()
  })

  it('at LOS disarms SSTV and restores the saved dial', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now, 'sstv') // arm; savedDial = OFF_FREQ
    d.tuneChannel.mockClear()
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    d.sstvAutoDisarm.mockClear()
    // Pass ended; the rig is still parked on 145.800 FM → restore is safe.
    tickIssAutoArm(p, ON_CHANNEL, true, d, p.losUnix + 60, 'sstv')
    expect(d.sstvAutoDisarm).toHaveBeenCalledTimes(1)
    // The RESTORE is an ordinary operator tune — that dial really is theirs.
    expect(d.setFrequency).toHaveBeenCalledTimes(1)
    expect(d.setFrequency).toHaveBeenCalledWith(14.074, '20m', 'USB')
    expect(d.tuneChannel).not.toHaveBeenCalled()
  })

  it('does not restore against the operator — a mid-pass manual QSY is left alone', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now, 'sstv') // arm
    d.tuneChannel.mockClear()
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    d.sstvAutoDisarm.mockClear()
    // At LOS the operator has since tuned to 40 m — disarm, but DON'T yank them back.
    tickIssAutoArm(p, OFF_FREQ, true, d, p.losUnix + 60, 'sstv')
    expect(d.sstvAutoDisarm).toHaveBeenCalledTimes(1)
    expect(d.setFrequency).not.toHaveBeenCalled()
  })

  it('when disabled, unwinds an arm still in flight (restores the dial)', () => {
    const d = deps()
    const now = 1_900_000_000
    tickIssAutoArm(pass(now - 60), OFF_FREQ, true, d, now, 'sstv') // arm while enabled
    d.tuneChannel.mockClear()
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    d.sstvAutoDisarm.mockClear()
    // Opt-in turned off mid-pass; the disabled tick ignores the pass and unwinds.
    tickIssAutoArm(null, ON_CHANNEL, false, d, now + 30, 'sstv')
    expect(d.sstvAutoDisarm).toHaveBeenCalledTimes(1)
    expect(d.setFrequency).toHaveBeenCalledWith(14.074, '20m', 'USB')
  })

  it('unwinds when the pass is no longer reported while enabled (the 30 d refusal path)', () => {
    const d = deps()
    const now = 1_900_000_000
    tickIssAutoArm(pass(now - 60), OFF_FREQ, true, d, now, 'sstv') // arm mid-pass
    d.tuneChannel.mockClear()
    d.setFrequency.mockClear()
    d.sstvArm.mockClear()
    d.sstvAutoDisarm.mockClear()
    // getIssPass now REJECTS (elements crossed the 30 d refusal mid-pass);
    // App.tsx routes the rejection to a null-pass tick. Still enabled, so the
    // LOS unwind must run — otherwise the rig strands on 145.800, SSTV armed.
    tickIssAutoArm(null, ON_CHANNEL, true, d, now + 30, 'sstv')
    expect(d.sstvAutoDisarm).toHaveBeenCalledTimes(1)
    expect(d.setFrequency).toHaveBeenCalledWith(14.074, '20m', 'USB')
  })

  // ⭐ THE UNWIND MUST NOT LOOK LIKE AN OPERATOR STOP. `sstvArm(false)` is an
  // explicit stop and the engine remembers it for the whole session: every later
  // entry to the SSTV view would then refuse to start the receiver, so the first
  // ISS pass of the day would silently kill SSTV decoding until a restart — the
  // very bug the view's auto-arm exists to fix. Asserted on BOTH unwind paths
  // (LOS, and the opt-in going off mid-pass) because they share one helper.
  it('⭐ never disarms as if the operator had pressed Stop', () => {
    const now = 1_900_000_000
    for (const unwindTick of [
      (d: ReturnType<typeof deps>, p: SatPass) =>
        tickIssAutoArm(p, ON_CHANNEL, true, d, p.losUnix + 60, 'sstv'), // LOS
      (d: ReturnType<typeof deps>) => tickIssAutoArm(null, ON_CHANNEL, false, d, now + 30, 'sstv'), // opt-in off
    ]) {
      resetIssAutoArm()
      const d = deps()
      const p = pass(now - 60)
      tickIssAutoArm(p, OFF_FREQ, true, d, now, 'sstv')
      d.sstvArm.mockClear()
      unwindTick(d, p)
      expect(d.sstvAutoDisarm).toHaveBeenCalledTimes(1)
      expect(d.sstvArm).not.toHaveBeenCalled()
    }
  })

  it('a disabled tick with nothing armed is a no-op', () => {
    const d = deps()
    tickIssAutoArm(pass(1_900_000_000), ON_CHANNEL, false, d, 1_900_000_000, 'sstv')
    expect(d.tuneChannel).not.toHaveBeenCalled()
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).not.toHaveBeenCalled()
    expect(d.sstvAutoDisarm).not.toHaveBeenCalled()
    expect(toasts).not.toHaveBeenCalled()
  })
})

// ⭐ THE SCREEN IN USE (the 2026-10-13 report: "I will be working ft8, I will get a pop up that a
// satellite is passing. My 20m band changes from 20m to 20m custom"). The opt-in used to move the
// radio wherever the operator was. Now the pass takes it only on the SSTV or Satellites screen;
// anywhere else another screen is using the radio, so the pass is announced and nothing moves.
describe('tickIssAutoArm and the screen in use', () => {
  const overhead = () => t('sat.iss.overhead', { dial: '145.800', mode: 'FM' })

  it('on another screen announces the pass once and touches nothing', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now, 'operate')
    tickIssAutoArm(p, OFF_FREQ, true, d, now + 30, 'operate') // the next 30 s poll
    tickIssAutoArm(p, OFF_FREQ, true, d, p.losUnix + 60, 'operate') // LOS: nothing was armed
    expect(d.tuneChannel).not.toHaveBeenCalled()
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).not.toHaveBeenCalled()
    expect(d.sstvAutoDisarm).not.toHaveBeenCalled()
    expect(toasts.mock.calls.map((c) => c[0])).toEqual([overhead()])
  })

  it('arms when the operator opens SSTV during the pass', () => {
    const d = deps()
    const now = 1_900_000_000
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now, 'operate')
    expect(d.tuneChannel).not.toHaveBeenCalled()
    tickIssAutoArm(p, OFF_FREQ, true, d, now + 30, 'sstv')
    expect(d.tuneChannel).toHaveBeenCalledWith(145.8, '2m', 'FM')
    expect(d.sstvArm).toHaveBeenCalledWith(true)
  })

  it('arms on the Satellites screen as on SSTV', () => {
    const d = deps()
    const now = 1_900_000_000
    tickIssAutoArm(pass(now - 60), OFF_FREQ, true, d, now, 'sats')
    expect(d.tuneChannel).toHaveBeenCalledWith(145.8, '2m', 'FM')
    expect(d.sstvArm).toHaveBeenCalledWith(true)
  })

  it('announces every pass, each once', () => {
    const d = deps()
    const now = 1_900_000_000
    const first = pass(now - 60)
    tickIssAutoArm(first, OFF_FREQ, true, d, now, 'cw')
    tickIssAutoArm(first, OFF_FREQ, true, d, now + 30, 'cw')
    const next = pass(now + 5_400) // the next orbit, about 90 minutes on
    tickIssAutoArm(next, OFF_FREQ, true, d, now + 5_460, 'cw')
    tickIssAutoArm(next, OFF_FREQ, true, d, now + 5_490, 'cw')
    expect(toasts.mock.calls.map((c) => c[0])).toEqual([overhead(), overhead()])
    expect(d.tuneChannel).not.toHaveBeenCalled()
  })
})

// A SATELLITE PASS THE OPERATOR IS WORKING HAS THE RADIO TOO. With a transponder held, Doppler steers
// the dial back to that bird within seconds, so the park on 145.800 was taken straight back, after
// interrupting the bird. Under a held pass the auto-arm moves nothing on any screen, announces the ISS
// as it does on another screen, and arms if the hold ends while the ISS is still up.
describe('tickIssAutoArm under a held satellite pass', () => {
  const overhead = () => t('sat.iss.overhead', { dial: '145.800', mode: 'FM' })
  const now = 1_900_000_000

  it('on SSTV or Satellites announces the pass once and touches nothing', () => {
    // With SSTV already open, the notice says why the radio stayed rather than asking for SSTV.
    const onSstv = 'ISS overhead: SSTV on 145.800 FM. A satellite pass has the radio, so it was not retuned.'
    for (const [screen, notice] of [
      ['sstv', onSstv],
      ['sats', overhead()],
    ] as const) {
      resetIssAutoArm()
      toasts.mockClear()
      const d = deps()
      const p = pass(now - 60)
      tickIssAutoArm(p, OFF_FREQ, true, d, now, screen, false, true)
      tickIssAutoArm(p, OFF_FREQ, true, d, now + 30, screen, false, true) // the next 30 s poll
      tickIssAutoArm(p, OFF_FREQ, true, d, p.losUnix + 60, screen, false, true) // LOS
      expect({
        screen,
        tuned: d.tuneChannel.mock.calls,
        armed: d.sstvArm.mock.calls,
        restored: d.setFrequency.mock.calls,
        disarmed: d.sstvAutoDisarm.mock.calls.length,
        notices: toasts.mock.calls.map((c) => c[0]),
      }).toEqual({ screen, tuned: [], armed: [], restored: [], disarmed: 0, notices: [notice] })
    }
  })

  it('arms when the hold ends while the ISS is still up', () => {
    const d = deps()
    const p = pass(now - 60)
    tickIssAutoArm(p, OFF_FREQ, true, d, now, 'sats', false, true)
    expect(d.tuneChannel, 'held: nothing tuned').not.toHaveBeenCalled()
    tickIssAutoArm(p, OFF_FREQ, true, d, now + 30, 'sats', false, false)
    expect(d.tuneChannel).toHaveBeenCalledWith(145.8, '2m', 'FM')
    expect(d.sstvArm).toHaveBeenCalledWith(true)
  })
})

// ⭐ NO RADIO HERE CAN RECEIVE 145.800 (operator, 2026-10-13: "say it once per session"). An FT-710
// alone, HF + 6 m: every pass announced the ISS and that the radio was not retuned, and on SSTV or
// Satellites the auto-arm tuned a channel the radio refuses. Now one notice a session says the
// radio cannot receive 145.800 MHz and where the switch is, and the auto-arm does nothing else.
describe('tickIssAutoArm where no radio here can receive 145.800 MHz', () => {
  // The FT-710's probed coverage (HF + 6 m) and an IC-9700's, in the snapshot's own shape (MHz).
  const FT710: IssRadio = { ...OFF_FREQ, rxRangesMhz: [[0.03, 56]] }
  const IC9700: IssRadio = { ...OFF_FREQ, rxRangesMhz: [[144, 148], [430, 450]] }
  const overhead = () => t('sat.iss.overhead', { dial: '145.800', mode: 'FM' })
  const cannotReceive = () => t('sat.iss.cannotReceive', { dial: '145.800' })
  const notices = () => toasts.mock.calls.map((c) => c[0])
  const now = 1_900_000_000
  // Two ISS passes about 90 minutes apart.
  const passes = [pass(now - 60), pass(now + 5_400)]

  /** Two polls into each pass, on `screen`, with `radio` the station's only radio or not. */
  function twoPasses(
    d: ReturnType<typeof deps>,
    radio: IssRadio,
    screen: 'operate' | 'sstv' | 'sats',
    soleRadio: boolean,
    tick = tickIssAutoArm,
  ) {
    for (const p of passes) {
      tick(p, radio, true, d, p.aosUnix + 60, screen, soleRadio)
      tick(p, radio, true, d, p.aosUnix + 90, screen, soleRadio)
    }
  }

  function expectNothingMoved(d: ReturnType<typeof deps>) {
    expect(d.tuneChannel).not.toHaveBeenCalled()
    expect(d.setFrequency).not.toHaveBeenCalled()
    expect(d.sstvArm).not.toHaveBeenCalled()
    expect(d.sstvAutoDisarm).not.toHaveBeenCalled()
  }

  it('says so once a session, across passes, and touches nothing', () => {
    const d = deps()
    twoPasses(d, FT710, 'operate', true)
    expect(notices()).toEqual([cannotReceive()])
    expectNothingMoved(d)
  })

  it('says so again after a restart', async () => {
    const d = deps()
    twoPasses(d, FT710, 'operate', true)
    expect(notices(), 'the first session').toEqual([cannotReceive()])
    toasts.mockClear()
    // A restart is a fresh module: the session is the page's lifetime, and nothing outlives it.
    vi.resetModules()
    const restarted = await import('./issAutoArm')
    const { pushToast: restartedToast } = await import('../toast')
    twoPasses(d, FT710, 'operate', true, restarted.tickIssAutoArm)
    expect(vi.mocked(restartedToast).mock.calls.map((c) => c[0]), 'the next session').toEqual([
      cannotReceive(),
    ])
    expectNothingMoved(d)
  })

  it('tunes nothing on the SSTV or Satellites screen either', () => {
    const d = deps()
    twoPasses(d, FT710, 'sstv', true)
    twoPasses(d, FT710, 'sats', true)
    expect(notices()).toEqual([cannotReceive()])
    expectNothingMoved(d)
  })

  it('with another enabled radio, announces every pass and tunes on SSTV as before (control)', () => {
    const d = deps()
    twoPasses(d, FT710, 'operate', false)
    expect(notices()).toEqual([overhead(), overhead()])
    expectNothingMoved(d)
    tickIssAutoArm(passes[1], FT710, true, d, passes[1].aosUnix + 120, 'sstv', false)
    expect(d.tuneChannel).toHaveBeenCalledWith(145.8, '2m', 'FM')
  })

  it('with the coverage unknown, announces every pass as before (control)', () => {
    // An empty list and an absent one both mean the probe has not answered: never "cannot".
    for (const unknown of [{ ...OFF_FREQ, rxRangesMhz: [] }, OFF_FREQ]) {
      resetIssAutoArm()
      toasts.mockClear()
      const d = deps()
      twoPasses(d, unknown, 'operate', true)
      expect(notices()).toEqual([overhead(), overhead()])
      expectNothingMoved(d)
    }
  })

  it('arms a radio that covers 145.800 as before (control)', () => {
    const d = deps()
    tickIssAutoArm(passes[0], IC9700, true, d, passes[0].aosUnix + 60, 'sstv', true)
    expect(d.tuneChannel).toHaveBeenCalledWith(145.8, '2m', 'FM')
    expect(d.sstvArm).toHaveBeenCalledWith(true)
  })
})

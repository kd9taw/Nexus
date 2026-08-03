import { describe, it, expect, vi, beforeEach } from 'vitest'

// Node test env: in-memory localStorage + a window with timers (the
// satAlarm.test.ts shim) — satAlarm's fired-key persistence needs storage.
class MemoryStorage {
  private m = new Map<string, string>()
  get length() { return this.m.size }
  clear() { this.m.clear() }
  getItem(k: string) { return this.m.has(k) ? (this.m.get(k) as string) : null }
  key(i: number) { return [...this.m.keys()][i] ?? null }
  removeItem(k: string) { this.m.delete(k) }
  setItem(k: string, v: string) { this.m.set(k, String(v)) }
}
const memStore = new MemoryStorage() as unknown as Storage
globalThis.localStorage = memStore
vi.stubGlobal('window', {
  localStorage: memStore,
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
} as unknown as Window & typeof globalThis)

vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../alerts', () => ({
  aosEarcon: vi.fn(),
  losEarcon: vi.fn(),
  doubleBeep: vi.fn(), // satAlarm.ts (the ⏰ dedupe tests) imports this
}))
import { pushToast } from '../toast'
import { aosEarcon, losEarcon } from '../alerts'
const toasts = vi.mocked(pushToast)
const aosTone = vi.mocked(aosEarcon)
const losTone = vi.mocked(losEarcon)

import { tickSatPassAlert, resetSatPassAlerts } from './satPassAlert'
import { checkSatAlarms, toggleSatAlarm, resetSatAlarms } from './satAlarm'
import type { SatPass, SatTrackStatus } from '../types'

const AOS = 1_900_000_000
const LOS = AOS + 720 // a 12 min pass

function track(over: Partial<SatTrackStatus> = {}): SatTrackStatus {
  return {
    name: 'RS-44',
    state: 'tracking',
    mode: 'doppler-only',
    dopplerDownlink: true,
    dopplerUplink: false,
    uplinkOffer: 'none',
    uplinkOfferMap: null,
    uplinkRadio: '',
    uplinkRadioId: 0,
    txMode: null,
    azDeg: null,
    elDeg: null,
    aosAzDeg: 205,
    satAzDeg: null,
    satElDeg: null,
    rangeKm: null,
    rangeRateKmS: null,
    altKm: null,
    downlinkHz: null,
    uplinkHz: null,
    downlinkShiftHz: null,
    uplinkShiftHz: null,
    transponder: null,
    transponderIndex: null,
    inverting: false,
    offsetHz: null,
    halfWidthHz: null,
    maxElDeg: 45,
    elementAgeDays: 1.2,
    elementEpochUnix: 1_785_442_400,
    aosUnix: AOS,
    losUnix: LOS,
    ...over,
  }
}

beforeEach(() => {
  memStore.clear()
  resetSatPassAlerts()
  resetSatAlarms()
  toasts.mockClear()
  aosTone.mockClear()
  losTone.mockClear()
})

describe('AOS', () => {
  it('the armed→tracking transition fires exactly one prominent toast + the rising tone', () => {
    tickSatPassAlert(track({ state: 'armed' }), AOS - 10)
    expect(toasts).not.toHaveBeenCalled() // armed is a wait, not an event
    tickSatPassAlert(track(), AOS)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(aosTone).toHaveBeenCalledTimes(1)
    expect(losTone).not.toHaveBeenCalled()
    const [msg, kind, ttl, opts] = toasts.mock.calls[0]
    // The pass facts the operator needs at the rotor: bird, az to point at,
    // max el, duration.
    expect(String(msg)).toContain('RS-44')
    expect(String(msg)).toContain('205')
    expect(String(msg)).toContain('45')
    expect(String(msg)).toContain('12 min')
    expect(kind).toBe('success')
    expect(ttl).toBe(30_000)
    expect(opts).toMatchObject({ prominent: true })
    // Repeated tracking polls are the same pass, not new events.
    tickSatPassAlert(track(), AOS + 2)
    tickSatPassAlert(track(), AOS + 4)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(aosTone).toHaveBeenCalledTimes(1)
  })

  it('armed → prepositioning → tracking still fires exactly once, at AOS', () => {
    tickSatPassAlert(track({ state: 'armed' }), AOS - 400)
    tickSatPassAlert(track({ state: 'prepositioning' }), AOS - 200)
    expect(toasts).not.toHaveBeenCalled()
    tickSatPassAlert(track(), AOS + 1)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(aosTone).toHaveBeenCalledTimes(1)
  })

  it('no track armed → nothing, ever', () => {
    tickSatPassAlert(null, AOS - 10)
    tickSatPassAlert(null, AOS)
    tickSatPassAlert(null, AOS + 10)
    expect(toasts).not.toHaveBeenCalled()
    expect(aosTone).not.toHaveBeenCalled()
    expect(losTone).not.toHaveBeenCalled()
  })

  it('a mid-pass wake says "pass in progress", never a stale "AOS now"', () => {
    // First observation IS a live pass (app restarted / poll started late):
    // the transition happened while nobody watched — report the truth.
    tickSatPassAlert(track(), AOS + 300)
    expect(toasts).toHaveBeenCalledTimes(1)
    const msg = String(toasts.mock.calls[0][0])
    expect(msg).toMatch(/in progress/i)
    expect(msg).not.toMatch(/AOS now|rising now/i)
    expect(msg).toContain('7 min') // 420 s of the pass left
    expect(aosTone).toHaveBeenCalledTimes(1) // still audible — that is the ask
  })

  it('a late-observed transition (suspended through AOS) also reports in progress', () => {
    tickSatPassAlert(track({ state: 'armed' }), AOS - 600)
    tickSatPassAlert(track(), AOS + 300) // woke to find it tracking
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(String(toasts.mock.calls[0][0])).toMatch(/in progress/i)
  })
})

describe('LOS', () => {
  it('fires once with the falling tone and the dial-handback fact — one toast total, never two', () => {
    tickSatPassAlert(track(), LOS - 4)
    toasts.mockClear() // drop the AOS alert; LOS is under test
    tickSatPassAlert(null, LOS + 5)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(losTone).toHaveBeenCalledTimes(1)
    const [msg, , , opts] = toasts.mock.calls[0]
    expect(String(msg)).toMatch(/RS-44 pass complete — LOS\./)
    expect(String(msg)).toMatch(/Dial handed back\./)
    expect(opts).toMatchObject({ prominent: true })
    // The next null polls are the same ended pass — silence.
    tickSatPassAlert(null, LOS + 7)
    tickSatPassAlert(null, LOS + 9)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(losTone).toHaveBeenCalledTimes(1)
  })

  it('an uplink-only pass reports the split released, never a dial that never left', () => {
    tickSatPassAlert(track({ dopplerDownlink: false, dopplerUplink: true }), LOS - 4)
    toasts.mockClear()
    tickSatPassAlert(null, LOS + 5)
    const msg = String(toasts.mock.calls[0][0])
    expect(msg).not.toMatch(/Dial handed back/)
    expect(msg).toMatch(/split released/i)
  })

  it('a rotor pass under the park policy says the mast is about to move', () => {
    tickSatPassAlert(track({ mode: 'rotor+doppler' }), LOS - 4)
    toasts.mockClear()
    tickSatPassAlert(null, LOS + 5, { rotPostPass: 'park' })
    expect(String(toasts.mock.calls[0][0])).toMatch(/Rotor parking\./)
  })

  it('a manual stop is silent — the operator did that; there is nothing to announce', () => {
    tickSatPassAlert(track(), AOS + 60)
    toasts.mockClear()
    losTone.mockClear()
    tickSatPassAlert(null, AOS + 90) // vanished long before losUnix = a stop
    expect(toasts).not.toHaveBeenCalled()
    expect(losTone).not.toHaveBeenCalled()
  })

  it('a wake long after LOS is silent — the moment has passed', () => {
    tickSatPassAlert(track(), LOS - 4)
    toasts.mockClear()
    tickSatPassAlert(null, LOS + 400) // past the 5 min honesty window
    expect(toasts).not.toHaveBeenCalled()
    expect(losTone).not.toHaveBeenCalled()
  })

  it('a straggler re-seeding the dead pass as live cannot re-announce the handback', () => {
    tickSatPassAlert(track(), LOS - 4)
    toasts.mockClear()
    tickSatPassAlert(null, LOS + 5) // the real LOS
    tickSatPassAlert(track(), LOS + 8) // a lapped in-flight answer lands late
    tickSatPassAlert(null, LOS + 10)
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(losTone).toHaveBeenCalledTimes(1)
  })
})

describe('a rotator that quits mid-pass', () => {
  // The backend deliberately keeps the track alive here — the mast and the
  // dial are independent surfaces — so nothing about the pass ending tells the
  // operator his antenna stopped following the bird. This is the only thing
  // that does.
  it('says so once, and does not end the pass', () => {
    tickSatPassAlert(track({ mode: 'rotor+doppler' }), AOS)
    toasts.mockClear()
    tickSatPassAlert(
      track({ mode: 'doppler-only', rotorLost: true }),
      AOS + 120,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    const [msg, kind, ttl, opts] = toasts.mock.calls[0]
    expect(String(msg)).toContain('RS-44')
    expect(String(msg)).toMatch(/rotator stopped answering/i)
    // What the operator now owns, and what he still does not have to think
    // about — the whole reason the track is allowed to outlive the mast.
    expect(String(msg)).toMatch(/point it yourself/i)
    expect(String(msg)).toMatch(/Doppler keep(s)? running/i)
    expect(kind).toBe('error')
    expect(ttl).toBe(30_000)
    expect(opts).toMatchObject({ prominent: true })
    // Every later poll of the same live track carries the same flag: one mast,
    // one announcement.
    tickSatPassAlert(track({ mode: 'doppler-only', rotorLost: true }), AOS + 123)
    tickSatPassAlert(track({ mode: 'doppler-only', rotorLost: true }), AOS + 126)
    expect(toasts).toHaveBeenCalledTimes(1)
  })

  it('a track that never had a rotator announces nothing', () => {
    tickSatPassAlert(track({ mode: 'doppler-only' }), AOS)
    toasts.mockClear()
    tickSatPassAlert(track({ mode: 'doppler-only' }), AOS + 120)
    expect(toasts).not.toHaveBeenCalled()
  })
})

describe('the sound setting', () => {
  it('soundOff silences the tones, never the popups', () => {
    tickSatPassAlert(track(), AOS, { soundOff: true })
    tickSatPassAlert(null, LOS + 5, { soundOff: true })
    expect(toasts).toHaveBeenCalledTimes(2) // AOS + LOS both still shown
    expect(aosTone).not.toHaveBeenCalled()
    expect(losTone).not.toHaveBeenCalled()
  })
})

describe('coexistence with the ⏰ per-pass alarm', () => {
  const pass = (aosUnix: number): SatPass => ({
    name: 'RS-44',
    aosUnix,
    losUnix: aosUnix + 720,
    maxElDeg: 45,
    aosAzDeg: 205,
    losAzDeg: 40,
  })

  it('the AOS alert marks the pass fired so ⏰ cannot fire a second thing at/after AOS', () => {
    toggleSatAlarm('RS-44')
    tickSatPassAlert(track(), AOS + 2) // the transition alert fires…
    expect(toasts).toHaveBeenCalledTimes(1)
    checkSatAlarms([pass(AOS)], (AOS + 10) * 1000) // …then the 30 s ⏰ tick lands
    expect(toasts).toHaveBeenCalledTimes(1) // no "is UP now" double
  })

  it('a ⏰ lead alarm before AOS and the AOS alert are different moments — both fire', () => {
    toggleSatAlarm('RS-44') // default 15 min lead
    checkSatAlarms([pass(AOS)], (AOS - 600) * 1000) // 10 min out: ⏰ fires
    expect(toasts).toHaveBeenCalledTimes(1)
    tickSatPassAlert(track({ state: 'armed' }), AOS - 600)
    tickSatPassAlert(track(), AOS) // AOS: the transition alert still fires
    expect(toasts).toHaveBeenCalledTimes(2)
  })
})

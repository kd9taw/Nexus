// @vitest-environment jsdom
//
// ⭐ THE RENDER-SIDE HALF OF THE FLASHING BUG.
//
// Operators reported APRS map icons flashing on and off constantly with the internet feed running.
// There were TWO independent causes, and fixing only the store would have left half the flash:
//
//  1. STORE CHURN. The packet log is capped at 300 entries by COUNT with no age expiry, so a busy
//     feed cycled it every few minutes and stations that were still active were evicted. Fixed by
//     the per-station store (tempo-app engine tests).
//
//  2. THIS ONE. The cockpit polls every 2 s and called `setHeard(h)` unconditionally. Tauri returns
//     a FRESH array every call, so the reference changed on every tick even when not a single
//     packet had arrived. That invalidated the memo feeding MapView's `aprs` prop, which re-ran the
//     draw effect, which reassigns `canvas.width` — and assigning canvas width RESETS THE BITMAP.
//     The entire map, relief and coastlines included, was torn down and repainted every two
//     seconds regardless of whether anything changed. Stations that were continuously present still
//     appeared to blink.
//
// These tests pin cause 2: identical backend data must not produce a new prop identity.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getAprsHeard, getAprsStations, type AprsStation } from '../api'

/** Records the identity of the `aprs` array MapView is handed on every render. */
const handed: (AprsStation[] | undefined)[] = []
vi.mock('./MapView', () => ({
  MapView: ({ aprs }: { aprs?: AprsStation[] }) => {
    handed.push(aprs)
    return <div data-testid="map" />
  },
}))

vi.mock('../api', () => ({
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({
    arm: 'explicit' as const,
    audioPeak: 0.3,
    lastAudioUnix: Math.floor(Date.now() / 1000),
    framesSeen: 2,
    framesDecoded: 2,
    lastDecodeUnix: Math.floor(Date.now() / 1000),
  })),
  getAprsIsStatus: vi.fn(async () => ({
    enabled: true,
    connected: true,
    verified: false,
    packets: 100,
    lastPacketUnix: Math.floor(Date.now() / 1000),
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
  })),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
}))

const T = 1_700_000_000

function stn(call: string, lastHeardUnix = T): AprsStation {
  return {
    call,
    lat: 41.9,
    lon: -87.6,
    symbolTable: '/',
    symbolCode: '>',
    kind: 'position',
    text: 'rolling',
    speedKnots: null,
    courseDeg: null,
    path: ['WIDE1-1'],
    raw: `${call}>APRS:!hi`,
    lastHeardUnix,
    lastRfUnix: lastHeardUnix,
    lastInetUnix: null,
    sourceKind: 'rf',
    packets: 1,
    firstHeardUnix: lastHeardUnix,
    wx: null,
  }
}

/** Serve this roster, mount, and let the first poll settle. */
async function mountWith(stations: AprsStation[]) {
  vi.mocked(getAprsStations).mockResolvedValue({ stations, ttlMin: 60, fadeAfterMin: 20 })
  render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await settle()
}

async function settle() {
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}

/** One poll tick, with the backend returning a NEW array carrying identical content. */
async function pollAgain(stations: AprsStation[]) {
  vi.mocked(getAprsStations).mockResolvedValue({
    stations: stations.map((s) => ({ ...s })),
    ttlMin: 60,
    fadeAfterMin: 20,
  })
  vi.mocked(getAprsHeard).mockResolvedValue([])
  await act(async () => {
    vi.advanceTimersByTime(2000)
  })
  await settle()
}

beforeEach(() => {
  handed.length = 0
  vi.useFakeTimers({ shouldAdvanceTime: true })
})
afterEach(() => {
  vi.useRealTimers()
  cleanup()
})

describe('an unchanged station never repaints the map', () => {
  it('identical poll data hands MapView the SAME array reference', async () => {
    const stations = [stn('W9AA-1'), stn('W9BB-2')]
    await mountWith(stations)
    const before = handed[handed.length - 1]
    expect(before).toHaveLength(2)

    // Three more polls, each returning freshly-allocated objects with identical content — exactly
    // what a live feed does when nothing new has arrived.
    await pollAgain(stations)
    await pollAgain(stations)
    await pollAgain(stations)

    const after = handed[handed.length - 1]
    expect(after).toBe(before)
  })

  it('a genuinely new station DOES produce a new reference, or the map would never update', async () => {
    // The other half of the contract: stability must not become staleness.
    const stations = [stn('W9AA-1')]
    await mountWith(stations)
    const before = handed[handed.length - 1]

    await pollAgain([stn('W9AA-1'), stn('W9NEW-3')])

    const after = handed[handed.length - 1]
    expect(after).not.toBe(before)
    expect(after).toHaveLength(2)
  })

  it('a station that MOVED produces a new reference', async () => {
    await mountWith([stn('W9AA-1')])
    const before = handed[handed.length - 1]
    const moved = stn('W9AA-1')
    moved.lat = 42.5
    await pollAgain([moved])
    expect(handed[handed.length - 1]).not.toBe(before)
  })

  it('a fresh beacon from the same station produces a new reference, so the age column ticks', async () => {
    await mountWith([stn('W9AA-1', T)])
    const before = handed[handed.length - 1]
    await pollAgain([stn('W9AA-1', T + 300)])
    expect(handed[handed.length - 1]).not.toBe(before)
  })
})

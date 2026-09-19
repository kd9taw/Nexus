// @vitest-environment jsdom
//
// #320: "the waterfall stalls, then the new little line shows up in Band Activity, then the
// waterfall resumes" (CW cockpit, reported 2026-09-16 at 19:24Z, during the 19Z CWT).
//
// The spot list is re-read every 15 s and arrives as fresh rows, sorted by frequency. Each flag
// was keyed `${call}-${freq}-${index}`, so one new station low in the band shifted the index of
// every flag above it, and React threw those buttons away and built them again. Replaying the RBN
// archive for the report minute through the spot buffer gives 618 CW flags on 20 m, about 15 in
// and 13 out per poll, and 490-566 of the buttons were rebuilt each time. Measured in a production
// build of this cockpit at a 4x CPU throttle (a slower PC): a spot update took 45 ms and the
// waterfall's worst row gap around it was 105 ms against a quiet 70-90; keeping the flags made
// that 29 ms and 70 ms. A flag is now keyed by the station and its frequency alone.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { BandStrip } from './BandStrip'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, SpotRow } from '../types'

vi.mock('../api', async (importOriginal) => {
  // Every export stubbed from the real module (see CwCockpit.agc.test.tsx for why).
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    previewCw: vi.fn(async (t: string) => t),
    cwDecode: vi.fn(() => new Promise(() => {})),
  }
})
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

function spot(call: string, freqMhz: number, ageSecs = 60): SpotRow {
  return {
    call,
    entity: 'United States',
    zone: 5,
    band: '20m',
    freqMhz,
    mode: 'CW',
    spotter: 'W3LPL-#',
    corroborators: [],
    ageSecs,
    comment: 'CW 12 dB 25 WPM CQ',
    licensed: true,
  }
}

/** A busy 20 m CW segment: forty stations, 1.5 kHz apart. */
const BUSY = Array.from({ length: 40 }, (_, i) => spot(`K${i}AA`, 14.002 + i * 0.0015))

/** The next poll, as the backend sends it: every row a fresh object 15 s older, the station at
 *  the top of the segment gone, and a new one at the bottom, so it sorts ahead of all the rest. */
function nextPoll(rows: SpotRow[]): SpotRow[] {
  return [spot('W1NEW', 14.0005, 5), ...rows.slice(0, -1).map((s) => ({ ...s, ageSecs: s.ageSecs + 15 }))]
}

/** Every flag on the strip, by the call it shows. */
function flags(root: HTMLElement): Map<string, Element> {
  const out = new Map<string, Element>()
  for (const b of root.querySelectorAll('.bandstrip-spot')) {
    out.set(b.querySelector('.bandstrip-spot-call')?.textContent ?? '', b)
  }
  return out
}

afterEach(cleanup)

describe('a spot update on the band strip (#320)', () => {
  it('keeps the flags of the stations still spotted, and shows the new one', () => {
    const strip = (spots: SpotRow[]) => (
      <BandStrip band="20m" dialMhz={14.03} txAllowed spots={spots} spotMode="CW" onWorkSpot={() => {}} />
    )
    const { container, rerender } = render(strip(BUSY))
    const before = flags(container)
    expect(before.size).toBe(40)

    rerender(strip(nextPoll(BUSY)))
    const after = flags(container)

    // The control: the update itself still lands.
    expect(after.has('W1NEW'), 'the new spot is on the strip').toBe(true)
    expect(after.has('K39AA'), 'the station that dropped out is gone').toBe(false)
    expect(after.size).toBe(40)
    // The fix: the 39 stations spotted before and after keep their buttons. Keyed by index, a
    // new station at the bottom of the band rebuilt every one of them.
    const kept = [...after].filter(([call, el]) => before.get(call) === el).length
    expect(kept, 'flags rebuilt although their station did not change').toBe(39)
  })

  it('still draws both of two rows that share a call and a frequency', () => {
    // The spot buffer merges a station's reports within 2 kHz, so the backend never sends
    // this; a key built from the row alone must still not collide if it ever does. React
    // reports a colliding key through console.error.
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    const { container } = render(
      <BandStrip
        band="20m"
        dialMhz={14.03}
        txAllowed
        spots={[spot('K1DUP', 14.025), spot('K1DUP', 14.025, 300)]}
        spotMode="CW"
        onWorkSpot={() => {}}
      />,
    )
    expect(container.querySelectorAll('.bandstrip-spot')).toHaveLength(2)
    expect(err.mock.calls.flat().join(' ')).not.toMatch(/same key/)
    err.mockRestore()
  })
})

describe('a spot update in the CW cockpit (#320)', () => {
  const snap = {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.03, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', refusedAgc: null,
      nb: true, nr: true, notch: null, filterWidthHz: 500, splitTxMhz: null, smeterDb: null,
    },
  } as unknown as AppSnapshot

  it('keeps the Band Activity flags and leaves the waterfall canvas alone', async () => {
    const cockpit = (spots: SpotRow[]) => (
      <CwCockpit snap={snap} theme="dark" onWorkSpot={() => {}} spots={spots} />
    )
    const { container, rerender } = render(cockpit(BUSY))
    await act(async () => {})
    const canvas = container.querySelector('canvas')
    const before = flags(container)
    expect(canvas, 'the scope is mounted').not.toBeNull()
    expect(before.size).toBe(40)

    rerender(cockpit(nextPoll(BUSY)))
    await act(async () => {})
    const after = flags(container)

    expect(after.has('W1NEW'), 'the new spot reached Band Activity').toBe(true)
    // A spot update must never remount the scope: that would wipe the waterfall's history.
    expect(container.querySelector('canvas'), 'the waterfall canvas was replaced').toBe(canvas)
    const kept = [...after].filter(([call, el]) => before.get(call) === el).length
    expect(kept, 'Band Activity flags rebuilt although their station did not change').toBe(39)
  })
})

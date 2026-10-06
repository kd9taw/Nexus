// @vitest-environment jsdom
//
// THE GETTING OUT BOX FOLLOWS THE UNITS SETTING.
//
// Reported 2026-10-06: with Settings ▸ Units on Imperial, and the operating system on Imperial too,
// the Getting Out box still read its distances in km, and a restart changed nothing. The summary's
// catalog sentence carried a literal "km" and the call site handed it the raw kilometres; the
// compass, the receiver list and the direction line each printed kilometres of their own. None of
// them asked the units setting.
//
// Rendered through the real ConnectView (the box is in its default layout) with the setting stored
// the way App stores it (`setUnitsMirror`) and the OS locale stubbed. The cases make the setting and
// the locale DISAGREE, so a box that read only one of them cannot pass: Imperial on a British
// locale, Metric on a US one, and Automatic on each.
//
// The distances are under 1000 km on purpose: the old code formatted them with toLocaleString, and a
// four-digit figure would have changed the Metric control's text as well ("1,450" → "1450"), hiding
// whether the units or the grouping moved.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'
import type { GettingOut } from '../../types'
import { setUnitsMirror } from '../../units'

vi.mock('../MapView', () => ({ MapView: () => <div data-testid="map" /> }))
vi.mock('../Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

// 965 km is 600 mi and 644 km is 400 mi, rounded the way the app rounds both.
const GETOUT: GettingOut = {
  count: 2,
  maxKm: 965,
  reports: [
    { call: 'DL1AA', grid: 'JN58', band: '20m', snr: -10, bearingDeg: 45, km: 965, octant: 'NE', ageSecs: 30 },
    { call: 'G4XYZ', grid: 'IO91', band: '20m', snr: -14, bearingDeg: 90, km: 644, octant: 'E', ageSecs: 90 },
  ],
}

vi.mock('../../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getSettings: vi.fn(async () => null),
    getDxccEntityLocations: vi.fn(async () => []),
    getGettingOut: vi.fn(),
    getBandOutlook: vi.fn(async () => ({ bands: [], mufNow: 0 })),
    getSpaceWxScales: vi.fn(async () => ({ scales: { r: 0, s: 0, g: 0 }, alerts: [] })),
    getKc2gMuf: vi.fn(async () => []),
    getXrayNow: vi.fn(async () => ({ flux: 1e-7, asOf: 0 })),
    getDxpedWindows: vi.fn(async () => []),
    getKpForecast: vi.fn(async () => ({ points: [] })),
    getSolarIndices: vi.fn(async () => ({ days: [] })),
    getPathOutlook: vi.fn(async () => null),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
  }
})

import * as api from '../../api'
import { ConnectView } from '../ConnectView'

vi.setConfig({ testTimeout: 15_000 })

const connectProps = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
}

beforeEach(() => {
  localStorage.clear()
  vi.mocked(api.getGettingOut).mockImplementation(async () => GETOUT)
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** Every distance the box shows: the summary's "furthest", each receiver row, each compass spoke's
 *  tooltip and the direction line under the compass. */
async function getoutDistances(setting: 'auto' | 'metric' | 'imperial', locale: string) {
  vi.spyOn(navigator, 'language', 'get').mockReturnValue(locale)
  setUnitsMirror(setting)
  render(<ConnectView {...connectProps} />)
  await waitFor(() => expect(document.querySelector('.getout-summary')).not.toBeNull())
  return {
    // `connect.getout.summary`'s second <b>: the first is the count.
    furthest: document.querySelectorAll('.getout-summary strong')[1]?.textContent,
    rows: [...document.querySelectorAll('.getout-list .go-where')].map((el) => el.textContent),
    spokes: [...document.querySelectorAll('.getout-rose line title')].map((el) => el.textContent),
    direction: document.querySelector('.getout-dir')?.textContent,
  }
}

const MILES = {
  furthest: '600 mi',
  rows: ['NE 600 mi', 'E 400 mi'],
  spokes: ['NE: 1 station, out to 600 mi', 'E: 1 station, out to 400 mi'],
  direction: 'strongest toward NE (~600 mi); little/nothing to the N/SE/S/SW/W/NW',
}
const KILOMETRES = {
  furthest: '965 km',
  rows: ['NE 965 km', 'E 644 km'],
  spokes: ['NE: 1 station, out to 965 km', 'E: 1 station, out to 644 km'],
  direction: 'strongest toward NE (~965 km); little/nothing to the N/SE/S/SW/W/NW',
}

describe('the Getting Out box shows its distances in the units the operator chose', () => {
  it('Imperial in Settings, Imperial OS — the reported case: miles everywhere on the box', async () => {
    expect(await getoutDistances('imperial', 'en-US')).toEqual(MILES)
  })

  it('Imperial in Settings beats a metric OS locale', async () => {
    expect(await getoutDistances('imperial', 'en-GB')).toEqual(MILES)
  })

  it('Metric in Settings beats a US OS locale', async () => {
    expect(await getoutDistances('metric', 'en-US')).toEqual(KILOMETRES)
  })

  it('Automatic follows a US OS locale to miles', async () => {
    expect(await getoutDistances('auto', 'en-US')).toEqual(MILES)
  })

  it('Automatic follows a British OS locale to kilometres', async () => {
    expect(await getoutDistances('auto', 'en-GB')).toEqual(KILOMETRES)
  })
})

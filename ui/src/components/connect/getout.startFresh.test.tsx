// @vitest-environment jsdom
//
// GETTING OUT'S START FRESH, through the real ConnectView: one press sets a start time, the box asks
// the station for the reports from then on (the station leaves the earlier ones out of the count, the
// furthest, the compass and the list), shows "since HH:MM:SS UTC", and the map's "hearing you" dots
// (2-D and 3-D) follow the same time. A start more than 30 minutes old has ended. Show all brings everything back. The station half (what it leaves out) is
// pinned in crates/propagation's gettingout tests; here the stand-in answers as the station does.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { GettingOut, MapSpot, PropagationSnapshot } from '../../types'

const maps = vi.hoisted(() => ({ spots: [] as (MapSpot[] | undefined)[], globe: [] as (MapSpot[] | undefined)[] }))
vi.mock('../MapView', () => ({
  MapView: (p: { prop: PropagationSnapshot | null }) => {
    maps.spots.push(p.prop?.spots)
    return <div data-testid="map" />
  },
}))
// The 3-D globe draws the same dots: stubbed to see the spots it is handed.
vi.mock('../Globe3D', () => ({
  default: (p: { prop: PropagationSnapshot | null }) => {
    maps.globe.push(p.prop?.spots)
    return <div data-testid="globe3d" />
  },
}))
vi.mock('../../gpu', () => ({ gpuCapableForGlobe: () => true, webgl2ForGlobe: () => true }))
vi.mock('../Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))

// 14:05:30 UTC: the press, and the clock the snapshot was taken by.
const NOW_MS = Date.UTC(2026, 9, 30, 14, 5, 30)
const START = NOW_MS / 1000

const ALL: GettingOut = {
  count: 2,
  maxKm: 16000,
  reports: [
    { call: 'VK3AA', grid: 'QF22', band: '20m', snr: -18, bearingDeg: 250, km: 16000, octant: 'W', ageSecs: 600 },
    { call: 'DL1AA', grid: 'JN58', band: '20m', snr: -10, bearingDeg: 45, km: 7000, octant: 'NE', ageSecs: 0 },
  ],
}
// What the station answers with a start time: the report before it is left out.
const SINCE: GettingOut = { count: 1, maxKm: 7000, reports: [ALL.reports[1]] }

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
import { setGetoutSince } from '../../features/getoutSince'
import { setUnitsMirror } from '../../units'

vi.setConfig({ testTimeout: 15_000 })

const spot = (call: string, ageSecs: number, heardMe: boolean): MapSpot => ({
  call, lat: 48, lon: 11, band: '20m', heardMe, ageSecs, time: START - ageSecs, approx: false,
})
// The same two receivers on the map (VK heard before the press, DL after), and a station heard
// before the press that did not hear me: activity, which Start fresh leaves alone.
const PROP = {
  source: 'offline',
  asOf: START,
  advisory: { bands: [] },
  openings: [],
  dxpeditions: { workableNow: [], upcoming: [] },
  spaceWx: {},
  spots: [spot('VK3AA', 600, true), spot('DL1AA', 0, true), spot('JA1AA', 600, false)],
} as unknown as PropagationSnapshot

const connectProps = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: PROP,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
}

const lastMap = (): MapSpot[] => maps.spots[maps.spots.length - 1] ?? []
const lastMapCalls = () => lastMap().filter((s) => s.heardMe).map((s) => s.call)
const listed = () => [...document.querySelectorAll('.getout-list .go-call')].map((el) => el.textContent)

beforeEach(() => {
  localStorage.clear()
  setGetoutSince(null)
  setUnitsMirror('metric')
  maps.spots = []
  maps.globe = []
  vi.mocked(api.getGettingOut).mockImplementation(async (since?: number | null) => (since == null ? ALL : SINCE))
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

describe('Getting Out: Start fresh', () => {
  it('counts only what is heard after the press — box and map — and Show all brings it back', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(NOW_MS)
    render(<ConnectView {...connectProps} />)
    await waitFor(() => expect(listed()).toEqual(['VK3AA', 'DL1AA']))
    expect(vi.mocked(api.getGettingOut)).toHaveBeenLastCalledWith()
    expect(lastMapCalls(), 'before the press the map shows both').toEqual(['VK3AA', 'DL1AA'])
    expect(document.querySelector('.getout-since')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Start fresh' }))

    // The box asks the station with the start time and shows only the later receiver.
    await waitFor(() => expect(listed()).toEqual(['DL1AA']))
    expect(vi.mocked(api.getGettingOut)).toHaveBeenLastCalledWith(START)
    expect(document.querySelectorAll('.getout-summary strong')[0]?.textContent, 'the count').toBe('1')
    expect(document.querySelectorAll('.getout-summary strong')[1]?.textContent, 'the furthest').toBe('7000 km')
    // The time is on the count's line; the heading keeps its one button.
    expect(document.querySelector('.getout-summary .getout-since')?.textContent).toBe('since 14:05:30 UTC')
    expect([...document.querySelectorAll('.getout-head button')].map((b) => b.textContent)).toEqual(['Show all'])
    // The compass draws only the later receiver's direction.
    expect([...document.querySelectorAll('.getout-rose line title')].map((el) => el.textContent)).toEqual([
      'NE: 1 station, out to 7000 km',
    ])
    // The map's "hearing you" dots follow; other activity stays on the map.
    expect(lastMapCalls()).toEqual(['DL1AA'])
    expect(lastMap().map((s) => s.call)).toContain('JA1AA')
    // The start is kept for a restart.
    expect(localStorage.getItem('nexus.connect.getout.since')).toBe(String(START))

    fireEvent.click(screen.getByRole('button', { name: 'Show all' }))

    await waitFor(() => expect(listed()).toEqual(['VK3AA', 'DL1AA']))
    expect(vi.mocked(api.getGettingOut)).toHaveBeenLastCalledWith()
    expect(document.querySelector('.getout-since')).toBeNull()
    expect(screen.getByRole('button', { name: 'Start fresh' })).toBeTruthy()
    expect(lastMapCalls()).toEqual(['VK3AA', 'DL1AA'])
    expect(localStorage.getItem('nexus.connect.getout.since')).toBe('')
  })

  it('says when nobody has heard you since the start', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(NOW_MS)
    vi.mocked(api.getGettingOut).mockImplementation(async (since?: number | null) =>
      since == null ? ALL : { count: 0, maxKm: 0, reports: [] },
    )
    setGetoutSince(START)
    render(<ConnectView {...connectProps} />)
    await waitFor(() =>
      expect(document.querySelector('.connect-getout .cp-none')?.textContent).toBe(
        'No reception reports since 14:05:30 UTC yet — call CQ, then watch who hears you.',
      ),
    )
    expect(screen.getByRole('button', { name: 'Show all' })).toBeTruthy()
  })

  it('the 3-D globe\'s "hearing you" dots follow the start too', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(NOW_MS)
    localStorage.setItem('nexus.connect.intent', 'dx')
    localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { map: '3d' } }))
    setGetoutSince(START)
    render(<ConnectView {...connectProps} />)
    await waitFor(() => expect(maps.globe.length).toBeGreaterThan(0))
    const last = maps.globe[maps.globe.length - 1] ?? []
    expect(last.filter((s) => s.heardMe).map((s) => s.call)).toEqual(['DL1AA'])
    expect(last.map((s) => s.call), 'other activity stays').toContain('JA1AA')
  })

  // The box shows the station's last 30 minutes: a start older than that has ended, so a box opened
  // (or a window restarted) after it reads as it does with no start — and one inside it keeps it.
  it('a start more than 30 minutes old has ended: the box asks for every report', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(NOW_MS)
    setGetoutSince(START - 1801)
    render(<ConnectView {...connectProps} />)
    await waitFor(() => expect(listed()).toEqual(['VK3AA', 'DL1AA']))
    expect(vi.mocked(api.getGettingOut)).not.toHaveBeenCalledWith(START - 1801)
    expect(screen.getByRole('button', { name: 'Start fresh' })).toBeTruthy()
    expect(document.querySelector('.getout-since')).toBeNull()
    expect(lastMapCalls(), 'the map shows every receiver again').toEqual(['VK3AA', 'DL1AA'])
  })

  it('a start exactly 30 minutes old still counts from the start', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(NOW_MS)
    setGetoutSince(START - 1800)
    render(<ConnectView {...connectProps} />)
    await waitFor(() => expect(vi.mocked(api.getGettingOut)).toHaveBeenCalledWith(START - 1800))
    expect(screen.getByRole('button', { name: 'Show all' })).toBeTruthy()
  })
})

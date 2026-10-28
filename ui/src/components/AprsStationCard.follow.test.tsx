// @vitest-environment jsdom
//
// The station card's map half: how far a moving station has come along its track, and the Follow
// button that keeps it in the middle of the map. A station with no position cannot be followed.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { AprsStationCard } from './AprsStationCard'
import type { AprsStation } from '../api'

vi.mock('../api', () => ({ openQrzPage: vi.fn(async () => {}) }))

const NOW = 1_700_000_000
function stn(over: Partial<AprsStation> = {}): AprsStation {
  return {
    call: 'W9AA-9',
    lat: 0.1,
    lon: 0.1,
    symbolTable: '/',
    symbolCode: '>',
    kind: 'position',
    text: '',
    speedKnots: 30,
    courseDeg: 45,
    path: [],
    raw: '',
    lastHeardUnix: NOW - 60,
    lastRfUnix: NOW - 60,
    lastInetUnix: null,
    sourceKind: 'rf',
    packets: 3,
    firstHeardUnix: NOW - 3600,
    wx: null,
    ...over,
  }
}
const TRACK = [
  { lat: 0, lon: 0, atUnix: NOW - 960 },
  { lat: 0, lon: 0.1, atUnix: NOW - 600 },
  { lat: 0.1, lon: 0.1, atUnix: NOW - 60 },
]

afterEach(cleanup)
beforeEach(() => localStorage.setItem('nexus.units', 'metric'))

describe('the APRS station card on the map', () => {
  it('says how far the station has come along its track, and over how long', () => {
    render(<AprsStationCard station={stn({ track: TRACK })} nowSec={NOW} me={null} onClose={() => {}} />)
    expect(screen.getByText('Track')).toBeTruthy()
    // 2 × 11.1 km over 15 minutes.
    expect(screen.getByText(/^22(\.\d)? km in 15 min$/)).toBeTruthy()
  })

  it('has no Track row for a station that has not moved', () => {
    render(<AprsStationCard station={stn()} nowSec={NOW} me={null} onClose={() => {}} />)
    expect(screen.queryByText('Track')).toBeNull()
  })

  it('Follow asks the map to follow this station, and Stop following asks it to stop', () => {
    const onFollow = vi.fn()
    const { rerender } = render(
      <AprsStationCard station={stn()} nowSec={NOW} me={null} onClose={() => {}} onFollow={onFollow} />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Follow' }))
    expect(onFollow).toHaveBeenLastCalledWith(true)
    rerender(<AprsStationCard station={stn()} nowSec={NOW} me={null} onClose={() => {}} onFollow={onFollow} following />)
    const stop = screen.getByRole('button', { name: 'Stop following' })
    expect(stop.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(stop)
    expect(onFollow).toHaveBeenLastCalledWith(false)
  })

  it('offers no Follow for a station with no position', () => {
    render(
      <AprsStationCard station={stn({ lat: null, lon: null })} nowSec={NOW} me={null} onClose={() => {}} onFollow={() => {}} />,
    )
    expect(screen.queryByRole('button', { name: 'Follow' })).toBeNull()
  })
})

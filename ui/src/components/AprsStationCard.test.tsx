// @vitest-environment jsdom
//
// The station detail card. The substrate is the merged station record, so the interesting cases are
// the states a real roster actually contains: a station heard only on RF, one only via the internet,
// one heard both ways, a weather station, and the minimal station that has sent nothing but a
// position. Each must render something honest rather than a blank or an invented value.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import {
  AprsStationCard,
  ageText,
  altitudeFt,
  pathSummary,
  sourceLines,
  wxRows,
} from './AprsStationCard'
import { openQrzPage, type AprsStation } from '../api'

vi.mock('../api', () => ({ openQrzPage: vi.fn(async () => {}) }))

const NOW = 1_700_000_000

function stn(over: Partial<AprsStation> = {}): AprsStation {
  return {
    call: 'W9AA-9',
    lat: 41.9,
    lon: -87.6,
    symbolTable: '/',
    symbolCode: '>',
    kind: 'position',
    text: '',
    speedKnots: null,
    courseDeg: null,
    path: [],
    raw: 'W9AA-9>APRS,WIDE1-1:!4154.00N/08736.00W>',
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

const ME = { lat: 41.0, lon: -87.0 }
const card = (over: Partial<AprsStation> = {}, me: typeof ME | null = ME) =>
  render(<AprsStationCard station={stn(over)} nowSec={NOW} me={me} onClose={() => {}} />)

afterEach(cleanup)
beforeEach(() => {
  vi.clearAllMocks()
  localStorage.setItem('nexus.units', 'imperial')
})

describe('pure helpers', () => {
  it('altitude comes off the /A= comment token, including below sea level', () => {
    expect(altitudeFt('/A=001244Lora Tracker')).toBe(1244)
    expect(altitudeFt('279/015/A=001041 APRS via LoRa')).toBe(1041)
    expect(altitudeFt('/A=-00050 dead sea')).toBe(-50)
    // Not an altitude token: must not invent one.
    expect(altitudeFt('no altitude here')).toBeNull()
    expect(altitudeFt('/A=123')).toBeNull()
  })

  it('ages read naturally across the scales', () => {
    expect(ageText(NOW - 20, NOW)).toBe('20 s')
    expect(ageText(NOW - 240, NOW)).toBe('4 min')
    expect(ageText(NOW - 7200, NOW)).toBe('2 h')
    expect(ageText(NOW + 10, NOW)).toBe('0 s') // clock skew must not read as negative
  })

  it('tells a direct packet from a digipeated one', () => {
    expect(pathSummary([])).toMatch(/direct/)
    // Requested but unused hops are still direct — the `*` is what marks a hop that repeated it.
    expect(pathSummary(['WIDE1-1', 'WIDE2-1'])).toMatch(/direct/)
    expect(pathSummary(['W9XYZ-1*', 'WIDE2-1'])).toMatch(/digipeated via W9XYZ-1\*/)
  })

  it('omits weather rows the station has no sensor for', () => {
    const rows = wxRows({
      windDirDeg: 220,
      windMph: 4,
      gustMph: null,
      tempF: 85,
      rain1hIn100: null,
      rain24hIn100: null,
      rainMidnightIn100: null,
      humidityPct: 68,
      pressureTenthHpa: 10156,
    })
    const keys = rows.map(([k]) => k)
    expect(keys).toContain('Temperature')
    expect(keys).toContain('Wind')
    expect(keys).toContain('Pressure')
    // ⭐ No gauge means NO ROW. A "0.00 in" row would be an invented measurement.
    expect(keys).not.toContain('Gust')
    expect(keys).not.toContain('Rain, last hour')
  })
})

describe('the honesty line — per-source ages, never collapsed', () => {
  it('an RF-only station says your receiver heard it, and says nothing about the internet', () => {
    const lines = sourceLines(stn({ lastRfUnix: NOW - 240, lastInetUnix: null }), NOW)
    expect(lines).toHaveLength(1)
    expect(lines[0].label).toMatch(/RF/)
    expect(lines[0].detail).toMatch(/your receiver/)
    expect(lines[0].detail).toMatch(/4 min/)
  })

  it('an internet-only station never claims your receiver heard it', () => {
    const lines = sourceLines(
      stn({ sourceKind: 'inet', lastRfUnix: null, lastInetUnix: NOW - 20 }),
      NOW,
    )
    expect(lines).toHaveLength(1)
    expect(lines[0].label).toMatch(/APRS-IS/)
    expect(lines[0].detail).not.toMatch(/your receiver/)
  })

  it('a station heard both ways gets BOTH ages separately', () => {
    // The whole point: "your receiver 4 min ago; APRS-IS 20 s ago" is two different facts, and one
    // combined "last heard 20 s" would hide the only one that says anything about your antenna.
    const lines = sourceLines(
      stn({ sourceKind: 'both', lastRfUnix: NOW - 240, lastInetUnix: NOW - 20 }),
      NOW,
    )
    expect(lines).toHaveLength(2)
    expect(lines[0].detail).toMatch(/4 min/)
    expect(lines[1].detail).toMatch(/20 s/)
  })
})

describe('render states', () => {
  it('an RF car shows its symbol meaning, position, and distance from you', () => {
    card({ text: 'rolling along' })
    expect(screen.getByText('W9AA-9')).toBeTruthy()
    expect(screen.getByText('Car')).toBeTruthy()
    expect(screen.getByText(/41\.9000, -87\.6000/)).toBeTruthy()
    expect(screen.getByText(/mi /)).toBeTruthy()
    expect(screen.getByText('rolling along')).toBeTruthy()
  })

  it('a minimal station with nothing but a position still renders', () => {
    card({ text: '', path: [], packets: 1, speedKnots: null })
    expect(screen.getByText('W9AA-9')).toBeTruthy()
    // No comment row when there is no comment, rather than an empty one.
    expect(screen.queryByText('Comment')).toBeNull()
    expect(screen.getByText('Path')).toBeTruthy()
  })

  it('a station heard with NO position says so rather than showing blanks', () => {
    card({ lat: null, lon: null })
    expect(screen.getByText(/nothing to plot/)).toBeTruthy()
  })

  it('omits the distance line when the operator has set no grid', () => {
    // Distance from an unknown origin is not a number we can honestly show.
    card({}, null)
    expect(screen.queryByText('From you')).toBeNull()
    expect(screen.getByText('Position')).toBeTruthy()
  })

  it('a weather station shows its readings', () => {
    card({
      symbolCode: '_',
      wx: {
        windDirDeg: 220,
        windMph: 4,
        gustMph: 11,
        tempF: 85,
        rain1hIn100: 0,
        rain24hIn100: null,
        rainMidnightIn100: null,
        humidityPct: 68,
        pressureTenthHpa: 10156,
      },
    })
    expect(screen.getByText('Weather')).toBeTruthy()
    expect(screen.getByText('85°F')).toBeTruthy()
    expect(screen.getByText('11 mph')).toBeTruthy()
    expect(screen.getByText('1015.6 hPa')).toBeTruthy()
    expect(screen.getByText(/SW 220/)).toBeTruthy()
  })

  it('a non-weather station has no weather section at all', () => {
    card()
    expect(screen.queryByText('Weather')).toBeNull()
  })

  it('an unrecognised symbol says so instead of naming something it is not', () => {
    card({ symbolCode: '' })
    expect(screen.getByText('Unrecognised symbol')).toBeTruthy()
  })

  it('shows motion and altitude when the station is moving', () => {
    card({ speedKnots: 36, courseDeg: 88, text: 'mobile /A=001041' })
    expect(screen.getByText(/36 kn/)).toBeTruthy()
    expect(screen.getByText(/1,041 ft/)).toBeTruthy()
  })
})

describe('raw packet and actions', () => {
  it('the raw packet is collapsed until asked for', () => {
    card()
    expect(screen.queryByText(/!4154\.00N/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /raw packet/i }))
    expect(screen.getByText(/!4154\.00N/)).toBeTruthy()
  })

  it('QRZ opens through the existing verb', () => {
    card()
    fireEvent.click(screen.getByRole('button', { name: 'QRZ' }))
    expect(openQrzPage).toHaveBeenCalledWith('W9AA-9')
  })

  it('the aprs.fi link points at the callsign and opens externally', () => {
    card()
    const link = screen.getByRole('link', { name: /aprs\.fi/i })
    expect(link.getAttribute('href')).toBe('https://aprs.fi/#!call=W9AA-9')
    expect(link.getAttribute('rel')).toMatch(/noreferrer/)
    // Named as third-party, so nobody mistakes it for Nexus data.
    expect(link.getAttribute('title')).toMatch(/third-party/i)
  })
})

describe('keyboard and a11y', () => {
  it('is a labelled dialog that takes focus on open', () => {
    card()
    const dlg = screen.getByRole('dialog', { name: /W9AA-9/ })
    expect(dlg).toBeTruthy()
    expect(document.activeElement).toBe(dlg)
  })

  it('Escape closes it', () => {
    const onClose = vi.fn()
    render(<AprsStationCard station={stn()} nowSec={NOW} me={ME} onClose={onClose} />)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })

  it('the close button closes it', () => {
    const onClose = vi.fn()
    render(<AprsStationCard station={stn()} nowSec={NOW} me={ME} onClose={onClose} />)
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))
    expect(onClose).toHaveBeenCalled()
  })
})

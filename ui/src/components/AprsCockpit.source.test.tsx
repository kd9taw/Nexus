// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import {
  getAprsHeard,
  getAprsIsStatus,
  getAprsStations,
  type AprsIsStatus,
  type AprsStation,
} from '../api'

// APRS now has TWO inlets — this station's receiver and the APRS-IS internet feed — and the whole
// design rests on the operator always being able to tell which is which. "My antenna hears this"
// and "a server told me about it" are different facts: the first is evidence about this station's
// RF chain and reach, the second is evidence about nothing local at all. These tests pin that the
// distinction survives the list, the counts, and the filter.

vi.mock('./MapView', () => ({
  // Report what the map was actually handed, so the list and the map can be checked for agreement.
  MapView: ({ aprs }: { aprs?: AprsStation[] }) => (
    <div data-testid="map" data-calls={(aprs ?? []).map((a) => a.call).join(',')} />
  ),
}))

vi.mock('../api', () => ({
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({
    arm: 'explicit' as const,
    audioPeak: 0.3,
    lastAudioUnix: Math.floor(Date.now() / 1000),
    framesSeen: 4,
    framesDecoded: 4,
    lastDecodeUnix: Math.floor(Date.now() / 1000),
  })),
  getAprsIsStatus: vi.fn(async () => inetOff()),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
}))

function inetOff(): AprsIsStatus {
  return {
    enabled: false,
    connected: false,
    verified: false,
    packets: 0,
    lastPacketUnix: null,
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
  }
}

let clock = 1_700_000_000

function stn(
  call: string,
  sourceKind: AprsStation['sourceKind'],
  over: Partial<AprsStation> = {},
): AprsStation {
  return {
    call,
    lat: 41.9,
    lon: -87.6,
    symbolTable: '/',
    symbolCode: '>',
    kind: 'position',
    text: 'hello',
    speedKnots: null,
    courseDeg: null,
    path: ['WIDE1-1'],
    raw: `${call}>APRS,WIDE1-1:!4154.00N/08736.00W>hello`,
    lastHeardUnix: clock,
    lastRfUnix: sourceKind === 'inet' ? null : clock,
    lastInetUnix: sourceKind === 'rf' ? null : clock,
    sourceKind,
    packets: 1,
    firstHeardUnix: clock,
    wx: null,
    ...over,
  }
}

async function mount(stations: AprsStation[], inet?: Partial<AprsIsStatus>) {
  vi.mocked(getAprsHeard).mockResolvedValue([])
  vi.mocked(getAprsStations).mockResolvedValue({ stations, ttlMin: 60, fadeAfterMin: 20 })
  vi.mocked(getAprsIsStatus).mockResolvedValue({ ...inetOff(), ...inet })
  const view = render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
  })
  return view
}

const mapCalls = () => screen.getByTestId('map').getAttribute('data-calls') ?? ''

beforeEach(() => {
  vi.clearAllMocks()
  clock = 1_700_000_000
})
afterEach(cleanup)

describe('APRS source tagging', () => {
  it('tags each station with how it reached us', async () => {
    await mount([stn('W9RF-1', 'rf'), stn('W9NET-2', 'inet')])
    const rf = screen.getByText('W9RF-1').closest('tr')!
    const net = screen.getByText('W9NET-2').closest('tr')!
    expect(rf.querySelector('.aprs-src-rf')).toBeTruthy()
    expect(net.querySelector('.aprs-src-inet')).toBeTruthy()
  })

  it('a station heard BOTH ways is never demoted by whichever packet arrived last', async () => {
    // The backend now derives this from when each channel last carried the station, so an
    // internet copy arriving after our own reception cannot relabel it — the timestamps are both
    // still there. This asserts the UI renders that faithfully.
    await mount([
      stn('W9BOTH-3', 'both', { lastRfUnix: clock - 60, lastInetUnix: clock }),
    ])
    const row = screen.getByText('W9BOTH-3').closest('tr')!
    expect(row.querySelector('.aprs-src-both')).toBeTruthy()
    expect(row.querySelector('.aprs-src-inet')).toBeFalsy()
  })
})

describe('the show-internet toggle', () => {
  it('hides internet-only stations from the list AND the map together', async () => {
    await mount([stn('W9RF-1', 'rf'), stn('W9NET-2', 'inet'), stn('W9BOTH-3', 'both')])
    expect(mapCalls()).toContain('W9NET-2')

    // The status chip is also a button now, so match the show/hide toggle specifically: it is the
    // one that reports a pressed state.
    const toggle = screen
      .getAllByRole('button', { name: /internet/i })
      .find((b) => b.hasAttribute('aria-pressed'))!
    fireEvent.click(toggle)

    // Gone from the list...
    expect(screen.queryByText('W9NET-2')).toBeNull()
    // ...and from the map, which must never disagree with the list about which stations exist.
    expect(mapCalls()).not.toContain('W9NET-2')
    // A station our own antenna heard stays, however the internet also reported it.
    expect(screen.getByText('W9RF-1')).toBeTruthy()
    expect(screen.getByText('W9BOTH-3')).toBeTruthy()
    expect(mapCalls()).toContain('W9BOTH-3')
  })

  it('names how many stations it will hide before you click it', async () => {
    await mount([stn('W9RF-1', 'rf'), stn('W9NET-2', 'inet'), stn('W9NET-4', 'inet')])
    expect(screen.getByRole('button', { name: /internet 2/i })).toBeTruthy()
  })

  it('does not appear at all when nothing came from the internet', async () => {
    await mount([stn('W9RF-1', 'rf')])
    // The status chip remains (it is how the feed is switched on); the show/hide TOGGLE does not.
    const pressable = screen
      .getAllByRole('button', { name: /internet/i })
      .filter((b) => b.hasAttribute('aria-pressed'))
    expect(pressable).toHaveLength(0)
  })
})

describe('station symbols in the list', () => {
  it('draws the station\'s own symbol, not a uniform dot', async () => {
    await mount([
      stn('W9CAR-9', 'rf', { symbolTable: '/', symbolCode: '>' }),
      stn('W9WX-1', 'rf', { symbolTable: '/', symbolCode: '_' }),
    ])
    // The title carries the meaning; the glyph is decorative and hidden from assistive tech.
    expect(screen.getByTitle('Car')).toBeTruthy()
    expect(screen.getByTitle('Weather station')).toBeTruthy()
  })

  it('shows the overlay character an operator put on an alternate-table symbol', async () => {
    // `R&` is the receive-only-iGate convention — the R is the whole point of the symbol.
    await mount([stn('W9GATE-1', 'rf', { symbolTable: 'R', symbolCode: '&' })])
    const cell = screen.getByTitle('Gateway / iGate')
    expect(cell.querySelector('.aprs-sym-overlay')?.textContent).toBe('R')
  })

  it('an unrecognised symbol still draws a glyph rather than a blank cell', async () => {
    await mount([stn('W9ODD-1', 'rf', { symbolTable: '/', symbolCode: '\u0001' })])
    const cell = screen.getByTitle('Unknown symbol')
    expect(cell.classList.contains('aprs-sym-unknown')).toBe(true)
    expect(cell.querySelector('svg path')?.getAttribute('d')).toBeTruthy()
  })
})

describe('the APRS-IS status chip', () => {
  it('stays visible when the feed is off, because it is how the feed gets turned ON', async () => {
    // It began as a pure status readout that hid itself when the feed was off. It is now also the
    // control, so hiding it would strand an operator with no way to switch the feed on from the
    // board — which was the whole request.
    await mount([])
    const chip = document.querySelector('.aprs-inet')
    expect(chip).toBeTruthy()
    expect(chip?.textContent).toMatch(/internet off/i)
  })

  it('shows beside the RF decode chip once the feed is on, without replacing it', async () => {
    // Two chips, never one. A green internet chip next to a silent RF chip is the diagnostic that
    // proves the fault is in the radio chain rather than the app.
    await mount([], { enabled: true, connected: true, packets: 40, lastPacketUnix: Math.floor(Date.now() / 1000) })
    expect(document.querySelector('.aprs-inet-live')).toBeTruthy()
    expect(document.querySelector('.aprs-health')).toBeTruthy()
  })
})

// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getAprsHeard, getAprsIsStatus, type AprsHeard, type AprsIsStatus } from '../api'

// APRS now has TWO inlets — this station's receiver and the APRS-IS internet feed — and the whole
// design rests on the operator always being able to tell which is which. "My antenna hears this"
// and "a server told me about it" are different facts: the first is evidence about this station's
// RF chain and reach, the second is evidence about nothing local at all. These tests pin that the
// distinction survives the list, the counts, and the filter.

vi.mock('./MapView', () => ({
  // Report what the map was actually handed, so the list and the map can be checked for agreement.
  MapView: ({ aprs }: { aprs?: AprsHeard[] }) => (
    <div data-testid="map" data-calls={(aprs ?? []).map((a) => a.source).join(',')} />
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

function pkt(source: string, sourceKind: AprsHeard['sourceKind'], over: Partial<AprsHeard> = {}): AprsHeard {
  return {
    source,
    dest: 'APRS',
    path: ['WIDE1-1'],
    lat: 41.9,
    lon: -87.6,
    symbolTable: '/',
    symbolCode: '>',
    kind: 'position',
    text: 'hello',
    speedKnots: null,
    courseDeg: null,
    addressee: null,
    msgId: null,
    atUnix: clock,
    sourceKind,
    raw: `${source}>APRS,WIDE1-1:!4154.00N/08736.00W>hello`,
    ...over,
  }
}

async function mount(heard: AprsHeard[], inet?: Partial<AprsIsStatus>) {
  vi.mocked(getAprsHeard).mockResolvedValue(heard)
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
    await mount([pkt('W9RF-1', 'rf'), pkt('W9NET-2', 'inet')])
    const rf = screen.getByText('W9RF-1').closest('tr')!
    const net = screen.getByText('W9NET-2').closest('tr')!
    expect(rf.querySelector('.aprs-src-rf')).toBeTruthy()
    expect(net.querySelector('.aprs-src-inet')).toBeTruthy()
  })

  it('a station heard BOTH ways is never demoted by whichever packet arrived last', async () => {
    // THE BUG THIS PREVENTS: collapsing to the newest packet would relabel a station our antenna
    // genuinely hears as "internet only" the moment an iGate reported it — erasing the one fact
    // the operator actually needs. The tag accumulates over every packet from the callsign.
    clock = 1_700_000_000
    const rf = pkt('W9BOTH-3', 'rf')
    clock += 10
    const net = pkt('W9BOTH-3', 'inet')
    await mount([rf, net])
    const row = screen.getByText('W9BOTH-3').closest('tr')!
    expect(row.querySelector('.aprs-src-both')).toBeTruthy()
    expect(row.querySelector('.aprs-src-inet')).toBeFalsy()
  })

  it('order does not matter — internet first, then RF, still reads as both', async () => {
    clock = 1_700_000_000
    const net = pkt('W9BOTH-3', 'inet')
    clock += 10
    const rf = pkt('W9BOTH-3', 'rf')
    await mount([net, rf])
    expect(screen.getByText('W9BOTH-3').closest('tr')!.querySelector('.aprs-src-both')).toBeTruthy()
  })
})

describe('the show-internet toggle', () => {
  it('hides internet-only stations from the list AND the map together', async () => {
    await mount([pkt('W9RF-1', 'rf'), pkt('W9NET-2', 'inet'), pkt('W9BOTH-3', 'both')])
    expect(mapCalls()).toContain('W9NET-2')

    const toggle = screen.getByRole('button', { name: /internet/i })
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
    await mount([pkt('W9RF-1', 'rf'), pkt('W9NET-2', 'inet'), pkt('W9NET-4', 'inet')])
    expect(screen.getByRole('button', { name: /internet 2/i })).toBeTruthy()
  })

  it('does not appear at all when nothing came from the internet', async () => {
    await mount([pkt('W9RF-1', 'rf')])
    expect(screen.queryByRole('button', { name: /internet/i })).toBeNull()
  })
})

describe('the APRS-IS status chip', () => {
  it('stays hidden while the feed is switched off', async () => {
    await mount([])
    expect(document.querySelector('.aprs-inet')).toBeNull()
  })

  it('shows beside the RF decode chip once the feed is on, without replacing it', async () => {
    // Two chips, never one. A green internet chip next to a silent RF chip is the diagnostic that
    // proves the fault is in the radio chain rather than the app.
    await mount([], { enabled: true, connected: true, packets: 40, lastPacketUnix: Math.floor(Date.now() / 1000) })
    expect(document.querySelector('.aprs-inet-live')).toBeTruthy()
    expect(document.querySelector('.aprs-health')).toBeTruthy()
  })
})

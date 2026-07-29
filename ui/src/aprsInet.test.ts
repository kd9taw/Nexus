import { describe, expect, it } from 'vitest'
import { aprsInetStatus, SOURCE_LABEL, SOURCE_TITLE } from './components/AprsCockpit'
import type { AprsIsStatus } from './api'

// The APRS-IS feed has the same failure mode the RF decoder had before the health chip: three
// different situations — switched off, unable to connect, and connected but filtered down to
// nothing — all present as an empty list. These pin that each says which one it is.

const NOW = 1_700_000_000

function st(over: Partial<AprsIsStatus> = {}): AprsIsStatus {
  return {
    enabled: true,
    connected: true,
    verified: false,
    packets: 12,
    lastPacketUnix: NOW - 5,
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
    ...over,
  }
}

describe('aprsInetStatus', () => {
  it('says nothing is running when the feed is off', () => {
    expect(aprsInetStatus(null, NOW).state).toBe('off')
    expect(aprsInetStatus(st({ enabled: false }), NOW).state).toBe('off')
  })

  it('separates "cannot connect" from "connected but quiet"', () => {
    const down = aprsInetStatus(st({ connected: false }), NOW)
    expect(down.state).toBe('connecting')
    expect(down.detail).toMatch(/retrying/i)

    const quiet = aprsInetStatus(st({ lastPacketUnix: NOW - 3600 }), NOW)
    expect(quiet.state).toBe('quiet')
    // The likely cause is a filter that is too tight, not a fault — say so, and say what to do.
    expect(quiet.detail).toMatch(/filter/i)
    expect(quiet.detail).toMatch(/radius|watched/i)
  })

  it('a feed that has never delivered a packet is quiet, not live', () => {
    const s = aprsInetStatus(st({ packets: 0, lastPacketUnix: null }), NOW)
    expect(s.state).toBe('quiet')
    expect(s.detail).toMatch(/yet/i)
  })

  it('reports the packet count while traffic is arriving', () => {
    const s = aprsInetStatus(st(), NOW)
    expect(s.state).toBe('live')
    expect(s.label).toContain('12')
  })

  it('a read-only feed is a healthy feed, not a degraded one', () => {
    // `pass -1` is unverified by design and receives normally. It must never read as a fault.
    const s = aprsInetStatus(st({ verified: false }), NOW)
    expect(s.state).toBe('live')
    expect(s.detail).toMatch(/read-only/i)
  })

  it('surfaces what the iGate has contributed and what it held back', () => {
    const s = aprsInetStatus(
      st({ uplinkEnabled: true, uploaded: 7, gateRejected: 3, lastReject: 'TCPIP in path' }),
      NOW,
    )
    expect(s.detail).toMatch(/7 contributed/)
    expect(s.detail).toMatch(/3 held back/)
    expect(s.detail).toMatch(/TCPIP in path/)
  })

  it('says nothing about an iGate that is switched off', () => {
    expect(aprsInetStatus(st(), NOW).detail).not.toMatch(/iGate/i)
  })
})

describe('source tags', () => {
  it('names all three sources, and only RF claims the antenna heard it', () => {
    expect(Object.keys(SOURCE_LABEL).sort()).toEqual(['both', 'inet', 'rf'])
    expect(SOURCE_TITLE.rf).toMatch(/off the air/i)
    // The internet tag must be explicit that this is NOT evidence about our own station.
    expect(SOURCE_TITLE.inet).toMatch(/has not heard/i)
    expect(SOURCE_TITLE.both).toMatch(/off the air/i)
  })
})

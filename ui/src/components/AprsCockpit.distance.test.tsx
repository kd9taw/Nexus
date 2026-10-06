// @vitest-environment jsdom
//
// The APRS station list states each station's distance in the units the operator chose. The
// column printed kilometres whatever Settings ▸ Units said, while the station card beside it
// (AprsStationCard) already followed the setting, so one station read two ways on one screen.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getAprsStations, type AprsStation } from '../api'
import { setUnitsMirror } from '../units'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({
    arm: 'explicit' as const,
    audioPeak: 0.3,
    lastAudioUnix: Math.floor(Date.now() / 1000),
    framesSeen: 1,
    framesDecoded: 1,
    lastDecodeUnix: Math.floor(Date.now() / 1000),
  })),
  getAprsIsStatus: vi.fn(async () => ({
    enabled: false,
    connected: false,
    verified: false,
    packets: 0,
    lastPacketUnix: null,
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
  })),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  getAprsTxNotice: vi.fn(async () => null),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
}))

afterEach(() => {
  cleanup()
  localStorage.clear()
})

const NOW = Math.floor(Date.now() / 1000)

// Due north of EM28's centre (38.5° N, 95° W) by 7.5° of latitude: 834 km, which is 518 mi.
const STATION: AprsStation = {
  call: 'W0ABC-9',
  lat: 46,
  lon: -95,
  symbolTable: '/',
  symbolCode: '>',
  kind: 'position',
  text: 'rolling',
  speedKnots: null,
  courseDeg: null,
  path: ['WIDE1-1'],
  raw: 'W0ABC-9>APRS:!hi',
  lastHeardUnix: NOW,
  lastRfUnix: NOW,
  lastInetUnix: null,
  sourceKind: 'rf',
  packets: 1,
  firstHeardUnix: NOW,
  wx: null,
}

async function distanceCell(units: 'metric' | 'imperial'): Promise<string | null | undefined> {
  setUnitsMirror(units)
  vi.mocked(getAprsStations).mockResolvedValue({ stations: [STATION], ttlMin: 60, fadeAfterMin: 20 })
  const { container } = render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
  return container.querySelector('.aprs-table .aprs-dist')?.textContent
}

describe('the APRS station list follows the units setting', () => {
  it('Imperial: the distance reads in miles', async () => {
    expect(await distanceCell('imperial')).toBe('518 mi N')
  })

  it('Metric: the distance reads in kilometres', async () => {
    expect(await distanceCell('metric')).toBe('834 km N')
  })
})

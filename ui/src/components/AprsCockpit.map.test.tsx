// @vitest-environment jsdom
//
// The APRS cockpit's map controls, as the cockpit wires them into the shared MapView: the map
// picker (Connect's, without 3D), the age and kind filters — applied ONCE, so the station list and
// the map always show the same stations — trails and labels, and Follow from the station card.
// Every choice is kept for this window. The map itself is a stand-in that shows what it was handed.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { AprsCockpit } from './AprsCockpit'
import { getAprsStations, type AprsStation } from '../api'

const seen = vi.hoisted(() => ({ props: null as null | Record<string, unknown> }))
vi.mock('./MapView', () => ({
  MapView: (p: Record<string, unknown> & { aprsToolbar?: ReactNode; aprsOverlay?: ReactNode }) => {
    seen.props = p
    return (
      <div data-testid="map">
        {p.aprsToolbar}
        {p.aprsOverlay}
      </div>
    )
  },
}))

vi.mock('../api', () => ({
  isTauri: () => false,
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
    enabled: false, connected: false, verified: false, packets: 0, lastPacketUnix: null,
    uplinkEnabled: false, uploaded: 0, gateRejected: 0, lastReject: null,
  })),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  getAprsTxNotice: vi.fn(async () => null),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
  setSettings: vi.fn(async () => ({})),
}))

const NOW = Math.floor(Date.now() / 1000)
function stn(call: string, over: Partial<AprsStation> = {}): AprsStation {
  return {
    call,
    lat: 39,
    lon: -95.7,
    symbolTable: '/',
    symbolCode: '-',
    kind: 'position',
    text: '',
    speedKnots: null,
    courseDeg: null,
    path: [],
    raw: '',
    lastHeardUnix: NOW - 30,
    lastRfUnix: NOW - 30,
    lastInetUnix: null,
    sourceKind: 'rf',
    packets: 1,
    firstHeardUnix: NOW - 600,
    wx: null,
    ...over,
  }
}
const CAR = stn('W9CAR-9', { symbolCode: '>' })
const HOUSE = stn('W9FIX')
const STALE = stn('W9OLD', { lastHeardUnix: NOW - 45 * 60 })

async function mount() {
  const view = render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
  return view.container
}
const listed = (c: HTMLElement) => [...c.querySelectorAll('.aprs-table tbody .aprs-from')].map((e) => e.textContent)
const mapped = () => ((seen.props?.aprs as AprsStation[]) ?? []).map((s) => s.call)

beforeEach(() => {
  localStorage.clear()
  seen.props = null
  vi.mocked(getAprsStations).mockResolvedValue({ stations: [CAR, HOUSE, STALE], ttlMin: 60, fadeAfterMin: 20 })
})
afterEach(cleanup)

describe('the APRS map controls', () => {
  it('opens on the Globe with trails and labels on, offering Connect’s 2-D maps (no 3D)', async () => {
    await mount()
    expect(seen.props?.projection).toBe('globe')
    expect(seen.props?.aprsTracks).toBe(true)
    expect(seen.props?.aprsLabels).toBe(true)
    const picker = screen.getByRole('group', { name: /map/i })
    expect([...picker.querySelectorAll('button')].map((b) => b.textContent)).toEqual(['Globe', 'Flat', 'Beam'])
    fireEvent.click(screen.getByRole('button', { name: 'Flat' }))
    expect(seen.props?.projection).toBe('world')
    expect(JSON.parse(localStorage.getItem('nexus.aprs.map')!).pick).toBe('world')
  })

  it('a kind filter narrows the list and the map alike', async () => {
    const c = await mount()
    expect(listed(c)).toEqual(['W9CAR-9', 'W9FIX', 'W9OLD'])
    fireEvent.change(screen.getByRole('combobox', { name: 'Show one kind of station' }), { target: { value: 'mobile' } })
    expect(listed(c)).toEqual(['W9CAR-9'])
    expect(mapped()).toEqual(['W9CAR-9'])
    expect(JSON.parse(localStorage.getItem('nexus.aprs.map')!).type).toBe('mobile')
  })

  it('an age filter drops stations heard before it, from the list and the map, and holds the trails to it', async () => {
    const c = await mount()
    fireEvent.change(screen.getByRole('combobox', { name: 'Show stations heard within this time' }), {
      target: { value: '30' },
    })
    expect(listed(c)).toEqual(['W9CAR-9', 'W9FIX'])
    expect(mapped()).toEqual(['W9CAR-9', 'W9FIX'])
    expect(seen.props?.aprsTrailMin).toBe(30)
  })

  it('Tracks and Labels switch off and stay off for this window', async () => {
    await mount()
    fireEvent.click(screen.getByRole('button', { name: 'Tracks' }))
    fireEvent.click(screen.getByRole('button', { name: 'Labels' }))
    expect(seen.props?.aprsTracks).toBe(false)
    expect(seen.props?.aprsLabels).toBe(false)
    cleanup()
    await mount()
    expect(seen.props?.aprsTracks).toBe(false)
    expect(seen.props?.aprsLabels).toBe(false)
  })

  it('Follow on the card hands the map the station, and the toolbar chip or Centre stops it', async () => {
    const c = await mount()
    fireEvent.click([...c.querySelectorAll('.aprs-table tbody tr')].find((r) => r.textContent?.includes('W9CAR-9'))!)
    fireEvent.click(screen.getByRole('button', { name: 'Follow' }))
    expect(seen.props?.aprsFollow).toBe('W9CAR-9')
    fireEvent.click(screen.getByRole('button', { name: 'Following W9CAR-9' }))
    expect(seen.props?.aprsFollow).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Follow' }))
    act(() => (seen.props?.onAprsCentre as () => void)())
    expect(seen.props?.aprsFollow).toBeNull()
  })
})

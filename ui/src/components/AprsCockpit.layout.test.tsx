// @vitest-environment jsdom
//
// APRS's body layout (layout L7), as the cockpit wires it: the station list width divider sits in
// the body beside the rail and the map, a stored width reaches the body as the clamped token, the
// map-side switch flips the body and remembers the side for THIS window, and a window that never
// touched either opens exactly as before. The divider's own keys and drag are pinned in
// AprsRailSeam.test.tsx; the geometry is Chrome's (the layout harness) and the cascade's
// (view-grids.test.ts).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import defaultSettings from './__fixtures__/defaultSettings.json'

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
    enabled: false, connected: false, verified: false, packets: 0, lastPacketUnix: null,
    uplinkEnabled: false, uploaded: 0, gateRejected: 0, lastReject: null,
  })),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  getAprsTxNotice: vi.fn(async () => null),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ ...defaultSettings })),
  setSettings: vi.fn(async () => ({})),
}))

async function mount() {
  const view = render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
  return view.container
}

const body = (c: HTMLElement) => c.querySelector<HTMLElement>('.aprs-body')!
const sideSwitch = () => screen.getByRole('button', { name: 'Map on the left' })

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
})
afterEach(cleanup)

describe('APRS’s station list width divider and map side', () => {
  it('a window that never set either opens exactly as before: no width token, the map on the right', async () => {
    const c = await mount()
    expect(body(c).style.getPropertyValue('--aprs-rail-w')).toBe('')
    expect(body(c).hasAttribute('data-map')).toBe(false)
    expect(sideSwitch().getAttribute('aria-pressed')).toBe('false')
  })

  it('the divider is the body’s own child beside the rail and the map, named for what it sizes', async () => {
    const c = await mount()
    const sep = screen.getByRole('separator', { name: 'Station list column width' })
    expect(sep.parentElement).toBe(body(c))
    expect(sep.className).toContain('aprs-railseam')
    // The rail and the map are still the body's first two children, in that order: the map moves
    // by CSS `order`, never in the tree.
    expect([...body(c).children].slice(0, 2).map((e) => e.className)).toEqual(['aprs-rail', 'aprs-map'])
  })

  it('a stored width reaches the body as the token the layout clamps', async () => {
    localStorage.setItem('nexus.aprs.railWidth', '1500')
    const c = await mount()
    // Wider than any body: the sheet caps it at half the body, and the preference stays stored.
    expect(body(c).style.getPropertyValue('--aprs-rail-w')).toBe('min(1500px, max(50%, 420px))')
    expect(localStorage.getItem('nexus.aprs.railWidth')).toBe('1500')
  })

  it('a stored width that is not one reads as the stock width', async () => {
    localStorage.setItem('nexus.aprs.railWidth', 'wide')
    const c = await mount()
    expect(body(c).style.getPropertyValue('--aprs-rail-w')).toBe('')
  })

  it('the map-side switch puts the map on the left and back, and remembers it for this window', async () => {
    const c = await mount()
    fireEvent.click(sideSwitch())
    expect(body(c).getAttribute('data-map')).toBe('left')
    expect(sideSwitch().getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem('nexus.aprs.mapSide')).toBe('left')
    // The tree did not move: only the sheet's `order` puts the map first.
    expect([...body(c).children].slice(0, 2).map((e) => e.className)).toEqual(['aprs-rail', 'aprs-map'])
    fireEvent.click(sideSwitch())
    expect(body(c).hasAttribute('data-map')).toBe(false)
    expect(localStorage.getItem('nexus.aprs.mapSide')).toBe('right')
  })

  it('a stored side opens with the map on the left', async () => {
    localStorage.setItem('nexus.aprs.mapSide', 'left')
    const c = await mount()
    expect(body(c).getAttribute('data-map')).toBe('left')
    expect(sideSwitch().getAttribute('aria-pressed')).toBe('true')
  })

  it('the switch sits over the map, not in the header', async () => {
    const c = await mount()
    expect(sideSwitch().closest('.aprs-map')).not.toBeNull()
    expect(c.querySelector('.np-head')!.contains(sideSwitch())).toBe(false)
  })
})

// The transmitter alarm (TxAlarmBanner, which App passes in): APRS has no CockpitHeader, so its
// header line carries the alarm where every other cockpit's header does, as that line's last row.
describe('the transmitter alarm on APRS', () => {
  it('is the last row of the header line', async () => {
    const view = render(
      <AprsCockpit
        active
        theme="dark"
        myGrid="EM28"
        onTune={() => {}}
        txAlarm={<div className="ch-txalarm">alarm</div>}
      />,
    )
    await act(async () => {
      for (let i = 0; i < 6; i++) await Promise.resolve()
    })
    const alarm = view.container.querySelector('.ch-txalarm')
    expect(alarm, 'the alarm passed in is drawn').not.toBeNull()
    expect(view.container.querySelector('.np-head')!.lastElementChild).toBe(alarm)
  })
})

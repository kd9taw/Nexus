// @vitest-environment jsdom
//
// The ISS channel's Doppler correction in the APRS cockpit: the health chip accepts a dial a pass
// has shifted (hand-corrected or Nexus-corrected), the switch saves, and the state is said in
// plain words. The behaviour itself is the engine's (`engine/aprs_iss.rs`).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit, aprsDecodeStatus, issDopplerLine } from './AprsCockpit'
import { getAprsHealth, getSettings, setSettings, type AprsHealth, type AprsIssDoppler } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
  isTauri: () => false,
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({})),
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
  getSettings: vi.fn(async () => defaultSettings),
  setSettings: vi.fn(async () => ({})),
}))

const NOW = 1_700_000_000

function health(over: Partial<AprsHealth> = {}): AprsHealth {
  return {
    arm: 'explicit',
    audioPeak: 0.4,
    lastAudioUnix: NOW,
    drains: 500,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
    lastFrameSeenUnix: null,
    framePeak: 0,
    maxFramePeak: 0,
    frameClippedSamples: 0,
    radioName: 'IC-9700',
    bandRadioCount: 1,
    ...over,
  }
}

const fm = (dialMhz: number) => ({ dialMhz, sideband: 'FM' })
const chip = (dialMhz: number, want: number) =>
  aprsDecodeStatus(health(), NOW, fm(dialMhz), want).state

describe('the health chip on a Doppler-shifted ISS dial', () => {
  it('accepts a dial within 4 kHz either side of 145.825', () => {
    for (const d of [145.821, 145.8225, 145.825, 145.8275, 145.829]) {
      expect(chip(d, 145.825), `dial ${d}`).not.toBe('wrongfreq')
    }
  })

  it('still calls a dial well off the ISS channel the wrong frequency', () => {
    expect(chip(145.835, 145.825)).toBe('wrongfreq')
    expect(chip(145.8, 145.825)).toBe('wrongfreq')
  })

  it('terrestrial channels are judged exactly as before', () => {
    expect(chip(144.39, 144.39)).not.toBe('wrongfreq')
    expect(chip(144.394, 144.39)).not.toBe('wrongfreq')
    expect(chip(144.4, 144.39)).toBe('wrongfreq')
    expect(chip(145.829, 144.39)).toBe('wrongfreq')
  })
})

describe('the correction in plain words', () => {
  it('names both frequencies while it steers', () => {
    const line = issDopplerLine({ on: true, whyOff: null, rxHz: 145_827_900, txHz: 145_822_100 })
    expect(line).toContain('145.8279')
    expect(line).toContain('145.8221')
  })

  it('says why it is off, and that the radio stays on the channel', () => {
    const off = (whyOff: AprsIssDoppler['whyOff']) =>
      issDopplerLine({ on: false, whyOff, rxHz: null, txHz: null })
    expect(off('belowHorizon')).toMatch(/below the horizon.*145\.825/)
    expect(off('noElements')).toMatch(/orbital elements/)
    expect(off('staleElements')).toMatch(/too old/)
    expect(off('noGrid')).toMatch(/grid/)
    expect(off('noCat')).toMatch(/CAT/)
    expect(off('switchedOff')).toMatch(/off/)
    expect(off('satTracker')).toMatch(/Satellites/)
    expect(issDopplerLine(null)).toBe('')
  })
})

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

async function mount(over: Record<string, unknown>, h: Partial<AprsHealth> = {}) {
  vi.mocked(getSettings).mockResolvedValue({ ...defaultSettings, ...over } as never)
  vi.mocked(getAprsHealth).mockResolvedValue(health({ lastAudioUnix: Math.floor(Date.now() / 1000), ...h }))
  render(<AprsCockpit active theme="dark" myGrid="EN52" onTune={() => {}} />)
  await settle()
}

const theSwitch = () =>
  screen.queryByRole('checkbox', { name: /correct the iss doppler shift/i }) as HTMLInputElement | null

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

describe('the ISS Doppler switch', () => {
  it('is on by default on the ISS channel, and turning it off saves', async () => {
    await mount({ aprsChannelMhz: 145.825, mygrid: 'EN52' })
    expect(theSwitch()?.checked).toBe(true)
    fireEvent.click(theSwitch()!)
    await settle()
    const calls = vi.mocked(setSettings).mock.calls
    const written = calls[calls.length - 1][0] as unknown as Record<string, unknown>
    expect(written.aprsIssDoppler).toBe(false)
  })

  it('keeps a stored off', async () => {
    await mount({ aprsChannelMhz: 145.825, mygrid: 'EN52', aprsIssDoppler: false })
    expect(theSwitch()?.checked).toBe(false)
  })

  it('is not offered on a terrestrial channel', async () => {
    await mount({ aprsChannelMhz: 144.39, mygrid: 'EN52' })
    expect(theSwitch()).toBeNull()
  })

  it("shows the engine's state beside it", async () => {
    await mount(
      { aprsChannelMhz: 145.825, mygrid: 'EN52' },
      { issDoppler: { on: false, whyOff: 'belowHorizon', rxHz: null, txHz: null } },
    )
    expect(screen.getByText(/below the horizon/i)).toBeTruthy()
  })
})

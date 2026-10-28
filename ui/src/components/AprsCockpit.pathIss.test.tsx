// @vitest-environment jsdom
//
// The beacon path quick-pick and the ISS channel. The path box stays the one source of truth:
// a pick only fills it and saves it as typing would. Choosing the ISS never changes the path by
// itself; it offers ARISS, one click away.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getSettings, setSettings } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'

vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
  // The map's Street choice reads the street map's state (features/streetMaps): not in a shell here.
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

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

/** Mount with a given stored settings shape, and report every frequency it tuned to. */
async function mount(over: Record<string, unknown> = {}, myGrid = 'EM28') {
  const tuned: number[] = []
  vi.mocked(getSettings).mockResolvedValue({ ...defaultSettings, ...over } as never)
  const view = render(
    <AprsCockpit active theme="dark" myGrid={myGrid} onTune={(f) => tuned.push(f)} />,
  )
  await settle()
  return { tuned, view }
}

// By its class, not its title: the title changes when the ISS is selected.
const channelSelect = () => document.querySelector('select.aprs-freq') as HTMLSelectElement
const pathBox = () => screen.getByLabelText(/^Path$/) as HTMLInputElement
const pathPick = () => screen.getByLabelText(/Pick a digipeater path/i) as HTMLSelectElement
const savedPath = () => {
  const calls = vi.mocked(setSettings).mock.calls
  return (calls[calls.length - 1][0] as unknown as Record<string, unknown>).aprsPath
}
const issHint = () => screen.queryByText(/answers to the path/i)

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

describe('the beacon path quick-pick', () => {
  it('fills the box and saves the path exactly as typing it would', async () => {
    await mount({ mygrid: 'EN52', aprsPath: ['WIDE2-1'] })
    expect(pathPick().value).toBe('WIDE2-1')
    fireEvent.change(pathPick(), { target: { value: 'WIDE1-1,WIDE2-1' } })
    await settle()
    expect(pathBox().value).toBe('WIDE1-1, WIDE2-1')
    expect(savedPath()).toEqual(['WIDE1-1', 'WIDE2-1'])
  })

  it('direct empties the box and saves no digipeaters', async () => {
    await mount({ mygrid: 'EN52', aprsPath: ['WIDE2-1'] })
    fireEvent.change(pathPick(), { target: { value: 'direct' } })
    await settle()
    expect(pathBox().value).toBe('')
    expect(savedPath()).toEqual([])
  })

  it('offers direct, WIDE1-1, WIDE2-1, WIDE1-1,WIDE2-1 and ARISS', async () => {
    await mount({ mygrid: 'EN52' })
    const values = Array.from(pathPick().options).map((o) => o.value).filter(Boolean)
    expect(values).toEqual(['direct', 'WIDE1-1', 'WIDE2-1', 'WIDE1-1,WIDE2-1', 'ARISS'])
  })

  it('the box still takes any path, and the pick then reads as your own', async () => {
    await mount({ mygrid: 'EN52', aprsPath: ['WIDE2-1'] })
    fireEvent.change(pathBox(), { target: { value: 'k9abc-1, wide2-2' } })
    fireEvent.blur(pathBox())
    await settle()
    expect(savedPath()).toEqual(['K9ABC-1', 'WIDE2-2'])
    expect(pathPick().value).toBe('')
  })
})

describe('the ISS on the channel picker', () => {
  it('is offered, labelled as the space station, and picking it tunes 145.825 and pins it', async () => {
    const { tuned } = await mount({ aprsChannelMhz: 144.39, mygrid: 'EN52', aprsPath: ['WIDE2-1'] })
    const iss = Array.from(channelSelect().options).find((o) => o.value === '145.825')
    expect(iss?.textContent).toMatch(/ISS \(space station\)/)
    fireEvent.change(channelSelect(), { target: { value: '145.825' } })
    await settle()
    expect(tuned).toEqual([144.39, 145.825])
    const calls = vi.mocked(setSettings).mock.calls
    const written = calls[calls.length - 1][0] as unknown as Record<string, unknown>
    expect(written.aprsChannelMhz).toBe(145.825)
    // Never changes the path by itself.
    expect(written.aprsPath).toEqual(['WIDE2-1'])
    expect(pathBox().value).toBe('WIDE2-1')
  })

  it('the picker tooltip says the pass shifts the frequency when the ISS is selected', async () => {
    await mount({ aprsChannelMhz: 145.825, mygrid: 'EN52', aprsPath: ['ARISS'] })
    expect(channelSelect().title).toMatch(/Doppler/)
  })

  it('with a non-ARISS path, a hint offers ARISS, and its button sets it', async () => {
    const { tuned } = await mount({ aprsChannelMhz: 145.825, mygrid: 'EN52', aprsPath: ['WIDE2-1'] })
    expect(issHint()).not.toBeNull()
    expect(issHint()?.textContent).toMatch(/Doppler/)
    fireEvent.click(screen.getByRole('button', { name: /use ariss/i }))
    await settle()
    expect(pathBox().value).toBe('ARISS')
    expect(savedPath()).toEqual(['ARISS'])
    expect(issHint()).toBeNull()
    expect(tuned).toEqual([145.825])
  })

  it('no hint when the path is already ARISS', async () => {
    await mount({ aprsChannelMhz: 145.825, mygrid: 'EN52', aprsPath: ['ARISS'] })
    expect(issHint()).toBeNull()
  })

  it('no hint on a terrestrial channel, whatever the path', async () => {
    await mount({ aprsChannelMhz: 144.39, mygrid: 'EN52', aprsPath: ['WIDE2-1'] })
    expect(issHint()).toBeNull()
  })
})

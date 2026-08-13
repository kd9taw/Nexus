// @vitest-environment jsdom
//
// ⚠️ THE APRS CHANNEL COMMANDS THE RADIO ON VIEW ENTRY. The cockpit's auto-tune effect calls
// `onTune` the first time the view becomes active, and it LATCHES — one tune per entry. So the
// value it reads has to be the operator's real channel, not a placeholder that a later fetch
// corrects, because by then the latch has closed and the rig is parked on the wrong frequency
// with nothing on screen having asked. That is the bug this whole persisted-channel change
// exists to end, and these tests are what stop it coming back.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getSettings, setSettings } from '../api'
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

const channelSelect = () =>
  screen.getByTitle(/APRS frequency by region/i) as HTMLSelectElement

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

describe('the APRS channel on view entry', () => {
  it('⭐ tunes to the operator’s PINNED channel — once, and never to a placeholder first', async () => {
    // The regression that matters. A `freq` initialised to 144.39 tuned the rig to the US channel
    // a tick before the stored 144.800 arrived, and the auto-tune latch then blocked the
    // correction — a European operator's rig retuned to the wrong continent on every launch.
    const { tuned } = await mount({ aprsChannelMhz: 144.8 })
    expect(tuned).toEqual([144.8])
    expect(channelSelect().value).toBe('144.8')
  })

  it('with no pin, follows the grid — and a European grid does NOT get the US channel', async () => {
    const { tuned } = await mount({ aprsChannelMhz: null, mygrid: 'IO91' }, 'IO91')
    expect(tuned).toEqual([144.8])
  })

  it('with no pin and a US grid, that is 144.390 — the positive control for the case above', async () => {
    // Without this, a derivation stuck at 144.800 would pass the test above and prove nothing.
    const { tuned } = await mount({ aprsChannelMhz: null, mygrid: 'EN52' }, 'EN52')
    expect(tuned).toEqual([144.39])
  })

  it('a pin outranks the grid', async () => {
    const { tuned } = await mount({ aprsChannelMhz: 145.57, mygrid: 'IO91' }, 'IO91')
    expect(tuned).toEqual([145.57])
  })
})

describe('picking a channel from the header', () => {
  it('retunes AND pins it, so the pick survives a restart', async () => {
    await mount({ aprsChannelMhz: null, mygrid: 'EN52' }, 'EN52')
    fireEvent.change(channelSelect(), { target: { value: '145.175' } })
    await settle()
    expect(vi.mocked(setSettings)).toHaveBeenCalled()
    const calls = vi.mocked(setSettings).mock.calls
    const written = calls[calls.length - 1][0] as unknown as Record<string, unknown>
    expect(written.aprsChannelMhz).toBe(145.175)
    // …and the whole struct rides along: a partial post would blank ~170 fields.
    expect(written.mycall).toBe(defaultSettings.mycall)
  })

  it('the picker is dead until the stored channel has been read', async () => {
    // A pick made against a placeholder would move the radio and not persist, with nothing
    // saying so. Better to be visibly not-ready for the one tick it takes.
    vi.mocked(getSettings).mockImplementation(() => new Promise(() => {})) // never resolves
    render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
    await settle()
    expect(channelSelect().disabled).toBe(true)
  })
})

describe('fixing your grid updates the derived channel', () => {
  it('⭐ without a restart — which is what “follow my grid” promises', async () => {
    // The prefill runs once per session, so an operator who corrected a wrong grid on the
    // Station tab kept the old derived channel until they relaunched.
    const { view, tuned } = await mount({ aprsChannelMhz: null, mygrid: 'EN52' }, 'EN52')
    expect(tuned).toEqual([144.39])

    vi.mocked(getSettings).mockResolvedValue({
      ...defaultSettings,
      aprsChannelMhz: null,
      mygrid: 'IO91',
    } as never)
    view.rerender(<AprsCockpit active theme="dark" myGrid="IO91" onTune={() => {}} />)
    await settle()
    expect(channelSelect().value).toBe('144.8')
  })

  it('but a pinned channel is NOT re-derived when the grid changes', async () => {
    const { view } = await mount({ aprsChannelMhz: 145.57, mygrid: 'EN52' }, 'EN52')
    view.rerender(<AprsCockpit active theme="dark" myGrid="IO91" onTune={() => {}} />)
    await settle()
    expect(channelSelect().value).toBe('145.57')
  })
})

describe('the beacon identity is persisted, not session state', () => {
  it('the beacon sends the stored symbol, comment and path', async () => {
    const { aprsSendBeacon } = await import('../api')
    await mount({
      mygrid: 'EN52',
      aprsSymbolTable: '\\',
      aprsSymbolCode: '#',
      aprsComment: 'Club digi',
      aprsPath: ['WIDE2-1'],
    })
    fireEvent.click(screen.getByRole('button', { name: /send beacon/i }))
    await settle()
    expect(vi.mocked(aprsSendBeacon)).toHaveBeenCalledWith(
      expect.any(Number),
      expect.any(Number),
      '\\',
      '#',
      'Club digi',
      ['WIDE2-1'],
    )
  })

  it('⭐ an empty stored path beacons DIRECT — it is a real choice, not a missing value', async () => {
    const { aprsSendBeacon } = await import('../api')
    await mount({ mygrid: 'EN52', aprsPath: [] })
    fireEvent.click(screen.getByRole('button', { name: /send beacon/i }))
    await settle()
    const calls = vi.mocked(aprsSendBeacon).mock.calls
    expect(calls[calls.length - 1][5]).toEqual([])
  })
})

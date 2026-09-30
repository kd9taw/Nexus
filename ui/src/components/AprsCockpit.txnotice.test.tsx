// @vitest-environment jsdom
//
// ⛔ WHAT APRS HAD QUEUED IS DROPPED, WITH A NOTICE (the operator, 2026-09-30: "a refused picture
// or beacon is dropped with a notice, never sent later"). The engine drops a queued beacon,
// message or automatic ack when TX goes off or the dial leaves the licence privileges, and says
// why through `get_aprs_tx_notice`. These pin what the APRS cockpit does with that: the notice
// in the status line, once — a later status is not stamped over by the same standing notice.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getAprsTxNotice } from '../api'
import { EN } from '../i18n'
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
  getAprsTxNotice: vi.fn(async () => null),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => defaultSettings),
  setSettings: vi.fn(async () => ({})),
}))

/** The engine's own sentence, as `APRS_REFUSED_TX_OFF` words it. */
const NOTICE =
  'APRS stopped: transmit was turned off, so what was still queued was dropped, not held for ' +
  'later. Send it again when you are ready.'

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })
}

/** One 2 s poll tick answering `notice`. */
async function poll(notice: string | null) {
  vi.mocked(getAprsTxNotice).mockResolvedValue(notice)
  await act(async () => {
    vi.advanceTimersByTime(2000)
  })
  await settle()
}

const statusLine = () => document.querySelector('.aprs-status')?.textContent ?? null

beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers({ shouldAdvanceTime: true })
  vi.mocked(getAprsTxNotice).mockResolvedValue(null)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('AprsCockpit: what the engine dropped', () => {
  it('says so in the status line', async () => {
    render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
    await settle()
    expect(statusLine()).toBeNull()
    await poll(NOTICE)
    expect(statusLine()).toBe(NOTICE)
  })

  it('shows a standing notice once, and again only when it comes back after a frame keyed', async () => {
    vi.mocked(getAprsTxNotice).mockResolvedValue(NOTICE)
    render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
    await settle()
    expect(statusLine()).toBe(NOTICE)
    // The operator's next action writes its own status; the same notice, still standing, must
    // not stamp over it at the next poll.
    fireEvent.click(screen.getByRole('button', { name: EN['aprs.msg.send'] }))
    expect(statusLine()).toBe(EN['aprs.status.msg.missing'])
    await poll(NOTICE)
    expect(statusLine()).toBe(EN['aprs.status.msg.missing'])
    // A frame keys (the engine clears the notice), then a new drop: shown again.
    await poll(null)
    await poll(NOTICE)
    expect(statusLine()).toBe(NOTICE)
  })

  it('reads nothing while the board is hidden', async () => {
    render(<AprsCockpit active={false} theme="dark" myGrid="EM28" onTune={() => {}} />)
    await settle()
    expect(getAprsTxNotice).not.toHaveBeenCalled()
  })
})

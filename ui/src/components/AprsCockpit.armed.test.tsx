// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { aprsArm, getAprsHealth, getAprsHeard, getSettings, type AprsHealth } from '../api'

// THE BUG THIS EXISTS FOR: the Monitor button was free-running local React state seeded to
// `false`, and nothing ever read the engine's actual `aprs_armed`. The engine's flag is SESSION
// state that outlives this component, so the two drifted:
//
//   - remount with the decoder still running -> the button said "Monitor" and the empty state
//     said "arm it to decode APRS", while packets were decoding into the list beside it;
//   - a failed arm still flipped the button to "● Monitoring", because the optimistic set
//     happened before the call and was never rolled back — the UI claimed to be decoding when
//     nothing was.
//
// The fix makes the engine the single source of truth for armed-ness, carried on the health poll
// that already runs. These tests pin that the button reports the ENGINE, never a local guess.

// The map is irrelevant here and drags in canvas — stub it to nothing.
vi.mock('./MapView', () => ({ MapView: () => <div data-testid="map" /> }))

vi.mock('../api', () => ({
  aprsArm: vi.fn(async () => []),
  getAprsHeard: vi.fn(async () => []),
  getAprsHealth: vi.fn(async () => ({
    armed: false,
    audioPeak: 0,
    lastAudioUnix: null,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
  })),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
}))

function health(over: Partial<AprsHealth> = {}): AprsHealth {
  return {
    armed: false,
    audioPeak: 0.3,
    lastAudioUnix: Math.floor(Date.now() / 1000),
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
    ...over,
  }
}

/** Render and let the mount-time polls settle. */
async function mount() {
  const view = render(
    <AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return view
}

const monitorButton = () => screen.getByRole('button', { name: /monitor/i })

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(getSettings).mockResolvedValue({ mygrid: 'EM28' } as never)
  vi.mocked(getAprsHeard).mockResolvedValue([])
  vi.mocked(getAprsHealth).mockResolvedValue(health())
  vi.mocked(aprsArm).mockResolvedValue([])
})
afterEach(cleanup)

describe('the Monitor button reports the engine, not a local guess', () => {
  it('shows MONITORING when the engine says the decoder is already armed', async () => {
    // The remount case: the operator armed the decoder, navigated away and back. The engine is
    // still decoding; the button used to come back up saying "Monitor".
    vi.mocked(getAprsHealth).mockResolvedValue(health({ armed: true }))
    await mount()
    expect(monitorButton().getAttribute('aria-pressed')).toBe('true')
    expect(monitorButton().textContent).toMatch(/monitoring/i)
  })

  it('shows MONITOR when the engine says the decoder is not armed', async () => {
    await mount()
    expect(monitorButton().getAttribute('aria-pressed')).toBe('false')
  })

  it('does not claim to be monitoring when arming FAILED', async () => {
    vi.mocked(aprsArm).mockRejectedValue(new Error('engine busy'))
    await mount()
    expect(monitorButton().getAttribute('aria-pressed')).toBe('false')
    await act(async () => {
      fireEvent.click(monitorButton())
      await Promise.resolve()
      await Promise.resolve()
    })
    // The engine never armed, so the button must not say it did.
    expect(monitorButton().getAttribute('aria-pressed')).toBe('false')
  })

  it('picks up the engine state the arm call produced, without waiting for the next poll', async () => {
    await mount()
    expect(monitorButton().getAttribute('aria-pressed')).toBe('false')
    // Arming succeeds and the engine now reports armed.
    vi.mocked(getAprsHealth).mockResolvedValue(health({ armed: true }))
    await act(async () => {
      fireEvent.click(monitorButton())
      await Promise.resolve()
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(aprsArm).toHaveBeenCalledWith(true)
    expect(monitorButton().getAttribute('aria-pressed')).toBe('true')
  })

  it('toggles OFF from an engine-armed state rather than re-arming', async () => {
    // With local state seeded false, a remount made the first click send arm(TRUE) at an
    // already-armed engine — which now also resets the health counters, throwing away the
    // session's decode history for no reason.
    vi.mocked(getAprsHealth).mockResolvedValue(health({ armed: true }))
    await mount()
    await act(async () => {
      fireEvent.click(monitorButton())
      await Promise.resolve()
    })
    expect(aprsArm).toHaveBeenCalledWith(false)
  })

  it('the empty state never says "monitor is off" while the engine is decoding', async () => {
    vi.mocked(getAprsHealth).mockResolvedValue(
      health({ armed: true, framesSeen: 4, framesDecoded: 4, lastDecodeUnix: 1 }),
    )
    await mount()
    expect(document.body.textContent).not.toMatch(/monitor is off/i)
  })
})

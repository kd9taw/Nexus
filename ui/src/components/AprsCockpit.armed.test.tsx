// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import {
  aprsArm,
  aprsAutoArm,
  getAprsHealth,
  getAprsHeard,
  getSettings,
  type AprsHealth,
} from '../api'

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
    arm: 'off' as const,
    audioPeak: 0,
    lastAudioUnix: null,
    drains: 0,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
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
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ mygrid: 'EM28' })),
}))

function health(over: Partial<AprsHealth> = {}): AprsHealth {
  return {
    arm: 'off' as const,
    audioPeak: 0.3,
    lastAudioUnix: Math.floor(Date.now() / 1000),
    drains: 500,
    framesSeen: 0,
    framesDecoded: 0,
    lastDecodeUnix: null,
    ...over,
  }
}

/** Render and let the mount-time polls settle. */
async function mount(active = true) {
  const view = render(
    <AprsCockpit active={active} theme="dark" myGrid="EM28" onTune={() => {}} />,
  )
  await act(async () => {
    await Promise.resolve()
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
  vi.mocked(aprsAutoArm).mockResolvedValue(true)
})
afterEach(cleanup)

describe('auto-arm on entering the view is receive-only and never fights the operator', () => {
  it('arms on entry so APRS does not open on a dead screen', async () => {
    await mount()
    expect(aprsAutoArm).toHaveBeenCalledTimes(1)
  })

  it('does NOT arm while the view is inactive', async () => {
    // The cockpit is kept alive hidden across navigation, so mounting is not entering.
    await mount(false)
    expect(aprsAutoArm).not.toHaveBeenCalled()
  })

  it('arms on the active RISING EDGE, once per entry, not on every render', async () => {
    const { rerender } = await mount()
    expect(aprsAutoArm).toHaveBeenCalledTimes(1)
    await act(async () => {
      rerender(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
      await Promise.resolve()
    })
    expect(aprsAutoArm).toHaveBeenCalledTimes(1)
    // Leave and come back — that is a new entry.
    await act(async () => {
      rerender(<AprsCockpit active={false} theme="dark" myGrid="EM28" onTune={() => {}} />)
      await Promise.resolve()
    })
    await act(async () => {
      rerender(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
      await Promise.resolve()
    })
    expect(aprsAutoArm).toHaveBeenCalledTimes(2)
  })

  it('an auto-armed decoder is labelled as such and disclaims automatic acks', async () => {
    // ⚠️ The honesty requirement: auto-armed must never LOOK ack-capable. The engine refuses to
    // ack in this state (aprs_auto_ack's gate); the operator has to be able to see that.
    vi.mocked(getAprsHealth).mockResolvedValue(health({ arm: 'auto' }))
    await mount()
    const btn = monitorButton()
    expect(btn.textContent).toMatch(/auto/i)
    expect(btn.getAttribute('title')).toMatch(/receive only/i)
    expect(btn.getAttribute('title')).toMatch(/never send an automatic ack/i)
  })

  it('an explicitly armed decoder says acks ARE allowed, and is not labelled auto', async () => {
    vi.mocked(getAprsHealth).mockResolvedValue(health({ arm: 'explicit' }))
    await mount()
    const btn = monitorButton()
    expect(btn.textContent).not.toMatch(/auto/i)
    expect(btn.getAttribute('title')).toMatch(/automatic acks are allowed/i)
  })

  it('clicking while AUTO-armed stops the decoder — it never silently upgrades to ack-capable', async () => {
    // The button reads "● Monitoring", so a click is the operator reaching for STOP. Turning that
    // same click into "grant unattended-transmit capability" would be the worst surprise here.
    vi.mocked(getAprsHealth).mockResolvedValue(health({ arm: 'auto' }))
    await mount()
    await act(async () => {
      fireEvent.click(monitorButton())
      await Promise.resolve()
    })
    expect(aprsArm).toHaveBeenCalledWith(false)
  })
})

describe('the Monitor button reports the engine, not a local guess', () => {
  it('shows MONITORING when the engine says the decoder is already armed', async () => {
    // The remount case: the operator armed the decoder, navigated away and back. The engine is
    // still decoding; the button used to come back up saying "Monitor".
    vi.mocked(getAprsHealth).mockResolvedValue(health({ arm: 'explicit' }))
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
    vi.mocked(getAprsHealth).mockResolvedValue(health({ arm: 'explicit' }))
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
    vi.mocked(getAprsHealth).mockResolvedValue(health({ arm: 'explicit' }))
    await mount()
    await act(async () => {
      fireEvent.click(monitorButton())
      await Promise.resolve()
    })
    expect(aprsArm).toHaveBeenCalledWith(false)
  })

  it('the empty state never says "monitor is off" while the engine is decoding', async () => {
    vi.mocked(getAprsHealth).mockResolvedValue(
      health({ arm: 'explicit', framesSeen: 4, framesDecoded: 4, lastDecodeUnix: 1 }),
    )
    await mount()
    expect(document.body.textContent).not.toMatch(/monitor is off/i)
  })
})

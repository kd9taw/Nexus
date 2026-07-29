// @vitest-environment jsdom
//
// The operator asked to be able to switch the internet feed off from the APRS board itself, without
// a trip to Settings. The hazard in granting that is a SECOND SOURCE OF TRUTH: a board control with
// its own local state that drifts from the Settings panel, so the two surfaces disagree about
// whether the feed is on. These tests pin that the board writes the same `Settings` fields the panel
// reads — one state, two surfaces — and that the controls which do NOT belong on a cockpit stay off
// it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsCockpit } from './AprsCockpit'
import { getSettings, setSettings, type AprsIsStatus } from '../api'
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
  getAprsIsStatus: vi.fn(async () => inet()),
  getAprsStations: vi.fn(async () => ({ stations: [], ttlMin: 60, fadeAfterMin: 20 })),
  aprsAutoArm: vi.fn(async () => true),
  aprsSendBeacon: vi.fn(async () => {}),
  aprsSendMessage: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({ ...defaultSettings, aprsIsEnabled: true })),
  setSettings: vi.fn(async () => ({})),
}))

function inet(over: Partial<AprsIsStatus> = {}): AprsIsStatus {
  return {
    enabled: true,
    connected: true,
    verified: false,
    packets: 40,
    lastPacketUnix: Math.floor(Date.now() / 1000),
    uplinkEnabled: false,
    uploaded: 0,
    gateRejected: 0,
    lastReject: null,
    ...over,
  }
}

async function mount(settingsOver: Record<string, unknown> = {}) {
  vi.mocked(getSettings).mockResolvedValue({
    ...defaultSettings,
    aprsIsEnabled: true,
    ...settingsOver,
  } as never)
  render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
  await act(async () => {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  })
}

/** Open the internet control panel off the status chip. */
const openPanel = () => fireEvent.click(screen.getByRole('button', { name: /internet/i }))
const panel = () => screen.getByRole('dialog', { name: /internet feed/i })

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

describe('the internet feed is switchable from the board', () => {
  it('the status chip opens a control panel', async () => {
    await mount()
    expect(screen.queryByRole('dialog')).toBeNull()
    openPanel()
    expect(panel()).toBeTruthy()
  })

  it('switching the feed off writes the SAME setting the Settings panel reads', async () => {
    // The one-source-of-truth pin. Not a local flag, not a bespoke command — the same
    // `aprsIsEnabled` field, through the same `set_settings`.
    await mount({ aprsIsEnabled: true })
    openPanel()
    const sw = panel().querySelector('[role="switch"]') as HTMLElement
    expect(sw.getAttribute('aria-checked')).toBe('true')
    await act(async () => {
      fireEvent.click(sw)
    })
    expect(setSettings).toHaveBeenCalledTimes(1)
    const sent = vi.mocked(setSettings).mock.calls[0][0] as unknown as Record<string, unknown>
    expect(sent.aprsIsEnabled).toBe(false)
  })

  it('sends the WHOLE settings object, so a board write cannot blank the other 170 fields', async () => {
    // `set_settings` replaces the struct wholesale. Posting a partial would wipe everything the
    // cockpit does not know about — callsign, radios, the lot.
    await mount()
    openPanel()
    await act(async () => {
      fireEvent.click(panel().querySelector('[role="switch"]') as HTMLElement)
    })
    const sent = vi.mocked(setSettings).mock.calls[0][0] as unknown as Record<string, unknown>
    expect(Object.keys(sent).length).toBeGreaterThan(150)
    expect(sent.mycall).toBe(defaultSettings.mycall)
    expect(sent.aprsIsHost).toBe(defaultSettings.aprsIsHost)
  })

  it('the radius is adjustable where the advice to widen it is given', async () => {
    // The chip's own quiet-feed guidance says "widen the radius", so the control has to be one
    // click from the diagnosis rather than in another window.
    await mount({ aprsIsRadiusKm: 150 })
    openPanel()
    const radius = panel().querySelector('input[type="number"]') as HTMLInputElement
    expect(radius.value).toBe('150')
    await act(async () => {
      fireEvent.change(radius, { target: { value: '400' } })
    })
    const sent = vi.mocked(setSettings).mock.calls[0][0] as unknown as Record<string, unknown>
    expect(sent.aprsIsRadiusKm).toBe(400)
  })

  it('watched calls commit on blur, not per keystroke', async () => {
    // Every write reconnects the feed, so committing mid-callsign would drop the session on each
    // character typed.
    await mount()
    openPanel()
    const calls = panel().querySelector('input[type="text"]') as HTMLInputElement
    await act(async () => {
      fireEvent.change(calls, { target: { value: 'w9xyz-9, kd9abc' } })
    })
    expect(setSettings).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.blur(calls)
    })
    const sent = vi.mocked(setSettings).mock.calls[0][0] as unknown as Record<string, unknown>
    expect(sent.aprsIsWatchCalls).toEqual(['W9XYZ-9', 'KD9ABC'])
  })

  it('says out loud that a filter change reconnects the feed', async () => {
    // The radius and watched calls are server-side subscription terms, so changing them really does
    // drop and re-establish the session. Better stated than discovered.
    await mount()
    openPanel()
    expect(panel().textContent).toMatch(/reconnects the feed/i)
  })
})

describe('what deliberately stays OUT of the cockpit', () => {
  it('the iGate uplink is NOT on the board', async () => {
    // Contributing to a global network under the operator's callsign is a considered commitment,
    // not an operating-time flick. A stray click on a cockpit must never start publishing.
    await mount()
    openPanel()
    expect(panel().textContent).not.toMatch(/receive-only igate/i)
    expect(panel().querySelectorAll('[role="switch"]').length).toBe(1)
  })

  it('set-once configuration stays in Settings, and the panel says where', async () => {
    await mount()
    openPanel()
    const text = panel().textContent ?? ''
    // No server/port/type-filter controls here...
    expect(text).not.toMatch(/rotate\.aprs2\.net/)
    expect(text).not.toMatch(/weather stations/i)
    // ...but the operator is told where they live rather than left to hunt.
    expect(text).toMatch(/Settings/)
    expect(text).toMatch(/APRS/)
  })
})

describe('dismissal', () => {
  it('Escape closes the panel', async () => {
    await mount()
    openPanel()
    expect(screen.queryByRole('dialog')).toBeTruthy()
    await act(async () => {
      fireEvent.keyDown(window, { key: 'Escape' })
    })
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('a click outside closes it, but the opening click does not', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    await mount()
    openPanel()
    // The deferred listener means the click that opened it cannot immediately close it.
    expect(screen.queryByRole('dialog')).toBeTruthy()
    await act(async () => {
      vi.advanceTimersByTime(1)
      fireEvent.mouseDown(document.body)
    })
    expect(screen.queryByRole('dialog')).toBeNull()
    vi.useRealTimers()
  })

  it('the chip reports its expanded state to assistive tech', async () => {
    await mount()
    const chip = screen.getByRole('button', { name: /internet/i })
    expect(chip.getAttribute('aria-expanded')).toBe('false')
    openPanel()
    expect(screen.getByRole('button', { name: /internet/i }).getAttribute('aria-expanded')).toBe(
      'true',
    )
  })
})

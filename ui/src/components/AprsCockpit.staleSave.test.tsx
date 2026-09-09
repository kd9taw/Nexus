// @vitest-environment jsdom
//
// ⭐⭐ **A PERMANENTLY-MOUNTED BOARD MUST NOT REVERT A SETTING IT NEVER TOUCHED.**
//
// `set_settings` replaces the whole `Settings` struct. `AprsCockpit` is rendered by App
// under a `hidden` host whenever the APRS view is enabled — so it is mounted for the
// whole session — and its own settings read sits behind a once-only ref guard. Post its
// copy and every field the operator has changed since it mounted, on any tab, goes back
// to what it was.
//
// **This is not a hypothetical.** It is how the beta-updates opt-in came back off after
// the operator turned it on: nothing was wrong with the beta control, and nothing looked
// wrong afterwards. The batch that added the contest station-data block (`contestQth*`,
// `contestCheck`, the zones, `contestPower`) inherits the same exposure, and six more
// silently-revertible fields is not a thing to ship and hope about.
//
// The fix is that the patch is applied to a FRESHLY-READ struct. These tests are what say
// so, and the first one fails against the shape that shipped.
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
    for (let i = 0; i < 12; i++) await Promise.resolve()
  })
}

const channelSelect = () =>
  screen.getByTitle(/APRS frequency by region/i) as HTMLSelectElement

/** What the board posted to `set_settings`, or `null` if it posted nothing. */
const posted = () => {
  const calls = vi.mocked(setSettings).mock.calls
  return calls.length ? (calls[calls.length - 1][0] as unknown as Record<string, unknown>) : null
}

beforeEach(() => vi.clearAllMocks())
afterEach(cleanup)

describe('a whole-struct save from a permanently-mounted board', () => {
  /**
   * The sequence, and it is the real one: the board mounts and reads settings; the
   * operator later opens Settings ▸ Contesting, fills in their county and turns the beta
   * channel on, and saves; then they touch an APRS control.
   *
   * The board's own copy still holds the OLD values — it has no way to know and no
   * subscription that would tell it. What it posts is what decides whether those two
   * settings survive.
   */
  it('carries the values saved AFTER it mounted, not the copy it holds', async () => {
    // v1 — what the board reads on mount.
    vi.mocked(getSettings).mockResolvedValue({
      ...defaultSettings,
      contestQthCounty: '',
      contestCheck: '',
      betaUpdates: false,
    } as never)
    render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
    await settle()

    // v2 — the operator saves from Settings. The board is never told.
    vi.mocked(getSettings).mockResolvedValue({
      ...defaultSettings,
      contestQthCounty: 'FRAN',
      contestCheck: '74',
      betaUpdates: true,
    } as never)

    // The operator touches an APRS control. Any `writeSettings` caller will do; the
    // channel select is the one that is always on screen.
    fireEvent.change(channelSelect(), { target: { value: '145.175' } })
    await settle()

    const sent = posted()
    expect(sent, 'the board must have posted something to save the channel').not.toBeNull()
    // ⭐ The assertion. Against the shape that shipped, all three of these are the v1
    // values and the operator's contest data and beta opt-in are gone.
    expect(sent?.contestQthCounty).toBe('FRAN')
    expect(sent?.contestCheck).toBe('74')
    expect(sent?.betaUpdates).toBe(true)
    // …and the board's own change is in the same payload, or the write did nothing.
    expect(sent?.aprsChannelMhz).toBe(145.175)
  })

  /**
   * POSITIVE CONTROL. A test that only ever sees one settings value cannot tell a fresh
   * read from a stale copy — so here the value does NOT change between mount and write,
   * and the same assertions must still hold. If this one ever fails, the test above is
   * passing for a reason other than the fix.
   */
  it('still posts the right values when nothing changed underneath it', async () => {
    vi.mocked(getSettings).mockResolvedValue({
      ...defaultSettings,
      contestQthCounty: 'DAVI',
      betaUpdates: false,
    } as never)
    render(<AprsCockpit active theme="dark" myGrid="EM28" onTune={() => {}} />)
    await settle()

    fireEvent.change(channelSelect(), { target: { value: '145.175' } })
    await settle()

    const sent = posted()
    expect(sent?.contestQthCounty).toBe('DAVI')
    expect(sent?.betaUpdates).toBe(false)
    expect(sent?.aprsChannelMhz).toBe(145.175)
  })
})

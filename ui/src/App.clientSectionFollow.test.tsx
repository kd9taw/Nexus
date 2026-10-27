// @vitest-environment jsdom
//
// A COMPUTER DRIVING A STATION SHOWS THE STATION'S SECTION. The screen this computer last showed is
// restored from its own storage, and the station may be in another section by then: a client that
// came up on Phone while the station sat in FT8 drew the Phone cockpit, and a band picked there
// went to the station as a phone band in the digital section, so the rig came up in DATA.
//
//   · On a client, an operating screen FOLLOWS the station's live section (`radio.operatingMode`),
//     read-only: it sends nothing, on connect or after, so connecting never moves the station.
//   · Picking a section there is a real change sent to the station. A refusal is shown, and the
//     screen stays where it was.
//   · CONTROL: at the station itself nothing changes. Launch stays read-only and the restored
//     screen stays as it was.
//
// AT THE WIRE, IN THE REAL App (App.sstvToPhone.test.tsx's idiom).
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import type { AppSnapshot } from './types'
import App from './App'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { pastTheSwitch } from './components/ConnectView.testkit'
import { setRemoteStation } from './features/durableStore'
import { EN } from './i18n/en'

// Each test mounts the whole App and waits on the real 300 ms snapshot poll.
const MOUNTS = 20_000

type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []

/** The station's live section, as its snapshot reports it. */
let section = 'digital'
/** The Keyboard section's modem on the air, as the station names it (`radio.keyboardMode`); absent
 *  from a station older than it. */
let keyboardMode: string | undefined
/** What the station answers to a section change: the snapshot, or this refusal. */
let refuseEntry: string | null = null

function snapshot(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    mode: 'Normal',
    radio: {
      dialMhz: 14.074,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      operatingMode: section,
      keyboardMode,
      transmitting: false,
      txEnabled: false,
      txAllowed: true,
      tuning: false,
      rxOffsetHz: 1500,
      txOffsetHz: 1500,
      txLevel: 0.5,
      slot: 0,
    },
    aiCw: { enabled: false, status: '', text: '' },
    link: { tier: 'Ft8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
    stations: [],
    conversations: [],
    activePeer: null,
    qso: null,
    fieldDay: null,
    recentDecodes: [],
    harqRescues: 0,
  } as unknown as AppSnapshot
}

function respond(cmd: string, args?: Record<string, unknown>): unknown {
  if (cmd === 'set_operating_mode') {
    if (refuseEntry) throw refuseEntry
    section = String(args?.mode)
    return snapshot()
  }
  if (/^(get_snapshot|set_area|set_tx_enabled)$/.test(cmd)) return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, operatingMode: 'phone' }
  if (/^(get_sstv_state|sstv_)/.test(cmd)) return { sending: false, rxLine: 0, txLine: 0 }
  if (/^(cw_decode|get_cw_state)$/.test(cmd)) {
    return { text: '', wpm: 20, sent: [], keyerError: null, candidates: [], rst: null, name: null, state: 'listening',
      headline: '', prompt: '', recommended: null, workedCall: null }
  }
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  if (cmd === 'client_status') return null
  return {}
}

async function settle(): Promise<void> {
  await act(async () => {
    for (let i = 0; i < 20; i++) await new Promise((r) => setTimeout(r, 0))
  })
}

/** Let the 300 ms snapshot poll deliver the station's current state. */
async function nextPoll(): Promise<void> {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 450))
  })
  await settle()
}

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 10_000 })
  await waitFor(() => expect(bridgeCalls.some((c) => c.cmd === 'get_settings')).toBe(true), { timeout: 10_000 })
  await settle()
}

/** The screen in front of the operator: the navigation's current button. */
const shown = () => document.querySelector('.mode-nav .mode-btn[aria-current="page"] .mode-label')?.textContent
const navButton = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')].find(
    (b) => b.querySelector('.mode-label')?.textContent === label,
  )
const entries = () => bridgeCalls.filter((c) => c.cmd === 'set_operating_mode')

let phoneLabel = ''
let ftLabel = ''
let cwLabel = ''

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  bridgeCalls.length = 0
  section = 'digital'
  keyboardMode = undefined
  refuseEntry = null
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd, args) as T
    },
  }
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { operate: true, phone: true, cw: true } }),
  )
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  setRemoteStation(false)
  delete window.__TAURI_INTERNALS__
})

/** The nav labels, read off the real navigation rather than assumed. */
function readLabels(): void {
  const byAria = (re: RegExp) =>
    [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')]
      .find((b) => re.test(b.getAttribute('aria-label') ?? ''))
      ?.querySelector('.mode-label')?.textContent ?? ''
  phoneLabel = byAria(/phone/i)
  cwLabel = byAria(/^cw|\bcw\b/i)
  ftLabel = byAria(/ft8/i)
  expect(phoneLabel && cwLabel && ftLabel, 'the navigation names Phone, CW and FT').toBeTruthy()
}

describe('a computer driving a station', () => {
  it('shows the station section it connected to, and sends nothing', async () => {
    setRemoteStation(true)
    await mountOn('phone')
    readLabels()
    await nextPoll()
    expect(shown(), 'the restored Phone screen follows the station into FT').toBe(ftLabel)
    expect(entries(), 'connecting moves nothing at the station').toEqual([])
  }, MOUNTS)

  it('follows a section the station changes to, still sending nothing', async () => {
    setRemoteStation(true)
    await mountOn('operate')
    readLabels()
    await nextPoll()
    expect(shown()).toBe(ftLabel)
    section = 'cw'
    await nextPoll()
    expect(shown()).toBe(cwLabel)
    expect(entries()).toEqual([])
  }, MOUNTS)

  it('sends the operator’s pick to the station, which then shows it', async () => {
    setRemoteStation(true)
    await mountOn('operate')
    readLabels()
    await nextPoll()
    fireEvent.click(navButton(phoneLabel)!)
    await settle()
    await nextPoll()
    expect(entries().map((c) => c.args?.mode)).toEqual(['phone'])
    expect(shown()).toBe(phoneLabel)
  }, MOUNTS)

  it('shows a refused pick and keeps the screen it was on', async () => {
    setRemoteStation(true)
    await mountOn('operate')
    readLabels()
    await nextPoll()
    refuseEntry = 'nexusClient:notLive'
    fireEvent.click(navButton(phoneLabel)!)
    await settle()
    await nextPoll()
    expect(entries().map((c) => c.args?.mode)).toEqual(['phone'])
    expect(shown(), 'a refused pick does not switch the screen').toBe(ftLabel)
    expect(document.body.textContent).toContain(EN['client.refused.notLive'])
    expect(entries(), 'going back sends nothing').toHaveLength(1)
  }, MOUNTS)
})

describe('a computer driving a station in the Keyboard section', () => {
  // PSK and JTTY are one section ('keyboard'); the station names the modem on the air
  // (`keyboardMode`), and the screen follows that.
  const byAria = (re: RegExp) =>
    [...document.querySelectorAll<HTMLButtonElement>('.mode-nav .mode-btn')]
      .find((b) => re.test(b.getAttribute('aria-label') ?? ''))
      ?.querySelector('.mode-label')?.textContent ?? ''
  const keyboardFeatures = () =>
    localStorage.setItem(
      'nexus.features.v1',
      JSON.stringify({ profile: 'custom', enabled: { operate: true, phone: true, cw: true, psk: true, jtty: true } }),
    )

  it('lands on JTTY when the station is on JTTY, and on PSK when it changes to PSK, sending nothing', async () => {
    keyboardFeatures()
    setRemoteStation(true)
    section = 'keyboard'
    keyboardMode = 'JTTY'
    await mountOn('operate')
    const jttyLabel = byAria(/jtty/i)
    const pskLabel = byAria(/psk/i)
    expect(jttyLabel && pskLabel, 'the navigation names JTTY and PSK').toBeTruthy()
    await nextPoll()
    expect(shown(), 'a station on JTTY: the JTTY screen').toBe(jttyLabel)
    keyboardMode = 'QPSK31'
    await nextPoll()
    expect(shown(), 'the station changes to PSK: the PSK screen').toBe(pskLabel)
    keyboardMode = 'JTTY'
    await nextPoll()
    expect(shown(), 'and back to JTTY').toBe(jttyLabel)
    expect(entries(), 'following moves nothing at the station').toEqual([])
  }, MOUNTS)

  it('a station too old to name its modem: PSK’s screen from elsewhere, and a JTTY screen is left alone', async () => {
    keyboardFeatures()
    setRemoteStation(true)
    section = 'keyboard'
    await mountOn('jtty')
    const jttyLabel = byAria(/jtty/i)
    const pskLabel = byAria(/psk/i)
    await nextPoll()
    expect(shown(), 'the JTTY screen is in the station’s section').toBe(jttyLabel)
    section = 'cw'
    await nextPoll()
    section = 'keyboard'
    await nextPoll()
    expect(shown(), 'the section’s own landing').toBe(pskLabel)
    expect(entries()).toEqual([])
  }, MOUNTS)
})

describe('CONTROL: at the station itself', () => {
  it('keeps the restored screen and sends nothing at launch', async () => {
    await mountOn('phone')
    readLabels()
    await nextPoll()
    expect(shown()).toBe(phoneLabel)
    expect(entries()).toEqual([])
  }, MOUNTS)
})

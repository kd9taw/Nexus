// @vitest-environment jsdom
//
// PROGRAMS' SPOTS ON THE CW COCKPIT'S BAND STRIP, IN THE REAL APP (Share by TCI).
//
// The real App on a fake bridge, on the CW cockpit: the station's snapshot says programs' spots
// changed (`radio.tciServer.spots`), so App asks for them (`get_tci_spots`) and the cockpit's band
// strip draws each, dotted, with its source. ⛔ A click on a program's spot sends `click_tci_spot`
// with its call and frequency and never `work_spot`, whose section entry can arm. CONTROL: the
// cluster spot beside it is worked (`work_spot`), and a station with no TCI server line is never
// asked for programs' spots.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import App from './App'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

const CW_DECODE = {
  text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '', prompt: '',
  recommended: null, workedCall: null, rst: null, name: null,
}

const spot = (over: Record<string, unknown>) => ({
  call: 'K1ABC', entity: '', zone: 0, band: '20m', freqMhz: 14.025, mode: 'CW', spotter: '', corroborators: [],
  ageSecs: 30, comment: '', licensed: true, ...over,
})
const CLUSTER = spot({ call: 'W1AW', freqMhz: 14.035, spotter: 'K3LR' })
const PROGRAM = spot({ call: 'K1ABC', freqMhz: 14.025, comment: 'CQ NA', tci: 2 })

let tciServer: unknown = null
const calls: [string, unknown][] = []

const snapshot = () => ({
  mycall: 'W2XYZ',
  mygrid: 'FN32',
  mode: 'qso',
  radio: {
    dialMhz: 14.03,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    rigMode: 'CW',
    operatingMode: 'cw',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    tuning: false,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
    ...(tciServer ? { tciServer } : {}),
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
})

function respond(cmd: string): unknown {
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|select_peer|work_spot|click_tci_spot)$/.test(cmd)) return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, mycall: 'W2XYZ', mygrid: 'FN32' }
  if (cmd === 'cw_decode') return CW_DECODE
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (cmd === 'get_xray_now') return { flux: 1e-7, asOf: 0 }
  if (cmd === 'get_all_spots') return [CLUSTER]
  if (cmd === 'get_tci_spots') return [PROGRAM]
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log|contest_removed|contest_unmerged|get_scp_calls|get_cat_cw_unproven_rig_models)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|contest_place|get_call_history|read_rotator|get_declination|get_sat_transponder)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  if (cmd === 'client_status') return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  calls.length = 0
  tciServer = { port: 40001, error: null, programs: [], spots: 4 }
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: unknown): Promise<T> => {
      calls.push([cmd, args])
      return respond(cmd) as T
    },
  }
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(allFeatureIds().map((id) => [id, true])) }),
  )
  localStorage.setItem('nexus.workspace', 'dx')
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  delete window.__TAURI_INTERNALS__
  window.history.replaceState(null, '', '/')
})

async function mountCw(): Promise<void> {
  window.location.hash = '#cw'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 15_000 })
  await waitFor(() => expect(document.querySelector('main.cw-cockpit')).toBeTruthy(), { timeout: 15_000 })
}

/** The band strip's flag for `call`. */
const flag = (call: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('.bandstrip-spot')].find((b) =>
    b.querySelector('.bandstrip-spot-call')?.textContent?.startsWith(call),
  )

const sent = (cmd: string) => calls.filter(([c]) => c === cmd)

describe("programs' spots on the CW cockpit's band strip, in the app", () => {
  it("draws a program's spot with its source; its click sends click_tci_spot and never work_spot", async () => {
    await mountCw()
    await waitFor(() => expect(flag('K1ABC')).toBeTruthy(), { timeout: 15_000 })
    expect(flag('K1ABC')!.className).toContain('from-tci')
    expect(flag('K1ABC')!.querySelector('.type-tci')?.textContent).toBe('TCI 2')
    await waitFor(() => expect(flag('W1AW')).toBeTruthy(), { timeout: 15_000 })
    fireEvent.click(flag('K1ABC')!)
    await waitFor(() => expect(sent('click_tci_spot')).toEqual([['click_tci_spot', { call: 'K1ABC', freqMhz: 14.025 }]]))
    expect(sent('work_spot'), 'a program spot was worked').toEqual([])
    fireEvent.click(flag('W1AW')!)
    await waitFor(() => expect(sent('work_spot')).toHaveLength(1))
    expect(sent('click_tci_spot'), 'control: the cluster spot is worked').toHaveLength(1)
  })

  it("CONTROL: a station with no TCI server line is never asked for programs' spots", async () => {
    tciServer = null
    await mountCw()
    await waitFor(() => expect(flag('W1AW')).toBeTruthy(), { timeout: 15_000 })
    expect(sent('get_tci_spots')).toEqual([])
    expect(flag('K1ABC')).toBeUndefined()
  })
})

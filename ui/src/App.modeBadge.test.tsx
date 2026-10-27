// @vitest-environment jsdom
//
// THE MODE BADGE ABOVE SETTINGS NAMES THE CONTEST THAT IS RUNNING, IN THE REAL APP (the operator,
// 2026-10-13, with the New York QSO Party set up and saved: "go to like cw just above the settings
// icon it says with the blue dot field day"). In Contest mode the badge read FIELD DAY whatever
// contest ran. It now names the contest the engine runs, by the short form the Contest screen's title
// shows (ModeNav.contest.test.tsx has every one).
//
// The station's snapshot is the engine's after that save: `pick_switch_tests.rs` holds the engine to
// it by value (the party's session, exchange, points, multipliers and Cabrillo CONTEST line). It is
// answered through the real `./api` on a fake bridge, as App.contestScreenOperator.test.tsx does.
//   · The party running: NYQP on the CW cockpit, and on the Contest screen, whose title names it.
//   · CONTROL: ARRL Field Day running still reads FIELD DAY.
//   · CONTROL: Contest mode off with the party still picked: the plain mode's badge.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import App from './App'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import nyqpFieldDay from './components/__fixtures__/nyqpFieldDay.json'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

// ── the station: the New York QSO Party from Albany County ─────────────────────────────────
// The engine's own `fieldDay` for it, captured from pick_switch_tests.rs's station after the save:
// a contact on phone, CW and RTTY, three counties, 1 + 2 + 3 points and four multipliers.
const NYQP = nyqpFieldDay
// The same station at ARRL Field Day: its class and section, no multipliers.
const ARRL_FD = {
  ...NYQP,
  event: 'arrlfd',
  role: '',
  sections: 0,
  workedSections: [],
  multCount: undefined,
  boards: [],
  log: [],
  qsoCount: 0,
  points: 0,
  poweredPoints: 0,
  totalScore: 0,
  sentExchange: '1D ENY',
  composingTextPhone: '1D ENY',
  receives: [
    { key: 'CLASS', kind: 'fdClass', required: true },
    { key: 'SECTION', kind: 'section', required: true },
  ],
  composing: [
    { key: 'CLASS', raw: '1D' },
    { key: 'SECTION', raw: 'ENY' },
  ],
  composingText: '1D ENY',
}
const CW_DECODE = {
  text: '', wpm: 22, sent: [], keyerError: null, candidates: [], state: 'listening', headline: '', prompt: '',
  recommended: null, workedCall: null, rst: null, name: null,
}

let mode = 'fieldDay'
let fieldDay: unknown = NYQP
let settingsAnswer: Record<string, unknown> = {}

const snapshot = () => ({
  mycall: 'W2XYZ',
  mygrid: 'FN32',
  mode,
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
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'Ft8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay,
  recentDecodes: [],
  harqRescues: 0,
})

function respond(cmd: string): unknown {
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|select_peer)$/.test(cmd)) return snapshot()
  if (cmd === 'get_settings') return settingsAnswer
  if (cmd === 'cw_decode') return CW_DECODE
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (cmd === 'get_xray_now') return { flux: 1e-7, asOf: 0 }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log|contest_removed|contest_unmerged|get_scp_calls|get_cat_cw_unproven_rig_models)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|contest_place|get_call_history|read_rotator|get_declination|get_sat_transponder)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  // A computer running its own station: the backend answers None, and the banner asks once. The
  // catch-all's `{}` would read as driving a remote station, asked every half second.
  if (cmd === 'client_status') return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  mode = 'fieldDay'
  fieldDay = NYQP
  settingsAnswer = {
    ...defaultSettings,
    mycall: 'W2XYZ',
    mygrid: 'FN32',
    fdActive: true,
    fdEvent: 'nyqp',
    contestQthState: 'NY',
    contestQthCounty: 'ALB',
  }
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => respond(cmd) as T,
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

/** The real App on `view`, past its boot snapshot, with that view's own control drawn. */
async function mount(view: string, drawn: string): Promise<HTMLElement> {
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 15_000 })
  return waitFor(
    () => {
      const el = document.querySelector<HTMLElement>(drawn)
      expect(el, `control: ${drawn} drew`).toBeTruthy()
      return el!
    },
    { timeout: 15_000 },
  )
}

const badge = () => document.querySelector('.mode-current')?.textContent

describe('the mode badge above Settings, in the app', () => {
  it('reads NYQP with the New York QSO Party running, on the CW cockpit and on the Contest screen titled for it', async () => {
    await mount('cw', 'main.cw-cockpit')
    await waitFor(() => expect(badge()).toBe('NYQP'))
    cleanup()
    const screen = await mount('fieldDay', '.contest-screen section.panel.fieldday')
    expect(screen.querySelector('.conv-peer')!.textContent).toBe('NYQP')
    expect(screen.querySelector('.fd-event-name')!.textContent).toBe('New York QSO Party')
    expect(badge()).toBe('NYQP')
  })

  it('CONTROL: ARRL Field Day running still reads FIELD DAY', async () => {
    fieldDay = ARRL_FD
    settingsAnswer = { ...settingsAnswer, fdEvent: 'arrlfd', fdClass: '1D', fdSection: 'ENY' }
    await mount('cw', 'main.cw-cockpit')
    await waitFor(() => expect(badge()).toBe('FIELD DAY'))
  })

  it('CONTROL: Contest mode off with the party still picked, the plain mode badge', async () => {
    mode = 'qso'
    fieldDay = null
    settingsAnswer = { ...settingsAnswer, fdActive: false }
    await mount('cw', 'main.cw-cockpit')
    await waitFor(() => expect(badge()).toBe('QSO'))
  })
})

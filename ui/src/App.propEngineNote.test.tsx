// @vitest-environment jsdom
//
// THE NOTE AFTER THE MOVE TO P.533, IN THE REAL APP: shown once to a station the move changed, as a
// line above the shell and never over anything in it, and gone for good when dismissed.
//   · CONTROL: not due (a new install, or a station already on P.533): no note.
//   · CONTROL: due, but the operator has since gone back to the heuristic: no note.
//   · CONTROL: dismissed where the settings file could not record it: no note at the next launch.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent } from '@testing-library/react'
import App from './App'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

const snapshot = () => ({
  mycall: 'W2XYZ',
  mygrid: 'FN32',
  mode: 'ft8',
  radio: {
    dialMhz: 14.074,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    rigMode: 'USB',
    operatingMode: 'ft8',
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
  recentDecodes: [],
  harqRescues: 0,
})

let settingsAnswer: Record<string, unknown> = {}
let dismissals = 0

function respond(cmd: string): unknown {
  if (cmd === 'dismiss_prop_engine_note') {
    dismissals += 1
    return snapshot()
  }
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|select_peer)$/.test(cmd)) return snapshot()
  if (cmd === 'get_settings') return settingsAnswer
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (cmd === 'get_xray_now') return { flux: 1e-7, asOf: 0 }
  if (cmd === 'ask_log') throw new Error('no log in this test')
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log|contest_removed|contest_unmerged|get_scp_calls|get_cat_cw_unproven_rig_models)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|contest_place|get_call_history|read_rotator|get_declination|get_sat_transponder)$/.test(cmd)) return null
  // A computer running its own station: the catch-all's `{}` would read as driving a remote one.
  if (cmd === 'client_status') return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  dismissals = 0
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
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, dispatchEvent() { return false }, onchange: null }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  delete window.__TAURI_INTERNALS__
  window.history.replaceState(null, '', '/')
})

async function launch(over: Record<string, unknown>): Promise<void> {
  settingsAnswer = { ...defaultSettings, mycall: 'W2XYZ', mygrid: 'FN32', ...over }
  window.location.hash = '#operate'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 15_000 })
  await waitFor(() => expect(document.querySelector('.shell')).toBeTruthy(), { timeout: 15_000 })
}

/** The note, once the settings have had time to arrive. */
const note = () =>
  [...document.querySelectorAll<HTMLElement>('[role="note"]')].find((n) => n.textContent?.includes('now use P.533'))

/** Long enough for the settings answer to land: a note that is coming has come. */
const settled = () => new Promise((r) => setTimeout(r, 300))

describe('the note after the move to P.533, in the app', () => {
  it('shows above the shell and outside it, and is gone for good once dismissed', async () => {
    await launch({ propEngine: 'p533', propEngineNoteDue: true })
    await waitFor(() => expect(note()).toBeTruthy(), { timeout: 15_000 })
    const line = note()!
    expect(line.closest('.shell'), 'over nothing in the shell').toBeNull()
    expect(line.compareDocumentPosition(document.querySelector('.shell')!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    fireEvent.click(line.querySelector('.onboarding-dismiss')!)
    await waitFor(() => expect(note()).toBeUndefined())
    await waitFor(() => expect(dismissals).toBe(1))
  })

  it('stays away when no note is due', async () => {
    await launch({ propEngine: 'p533', propEngineNoteDue: false })
    await settled()
    expect(note()).toBeUndefined()
  })

  it('stays away once the operator has gone back to the heuristic', async () => {
    await launch({ propEngine: 'heuristic', propEngineNoteDue: true })
    await settled()
    expect(note()).toBeUndefined()
  })

  it('stays away after a dismissal the settings file could not record', async () => {
    localStorage.setItem('nexus.propEngineNoteDismissed', '1')
    await launch({ propEngine: 'p533', propEngineNoteDue: true })
    await settled()
    expect(note()).toBeUndefined()
  })
})

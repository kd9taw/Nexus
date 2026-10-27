// @vitest-environment jsdom
//
// THE CONTEST SCREEN'S LOWER SECTION IN A WINDOW OF ITS OWN (the operator, 2026-10-12: "in the contest
// section, the bottom section that has the contest mode switch and all the county activity, that whole
// window needs to pop out"). The pick: "A Pop out button opens the whole lower section in its own window
// (put it on another monitor). While it's open, the main Contest screen shows just the big entry and last
// contacts; closing the window brings the section back."
//
// AT THE WIRE, IN THE REAL APP AND THE REAL POP-OUT (App.contestScreenOperator.test.tsx's idiom): the real
// `./api` under a recording bridge, and Tauri's event bridge, which is what says a window is gone.
//   · The control is on the section in the main window, and opens the `contestscreen` window.
//   · While that window is open the main Contest screen keeps the entry, its title line with the operator
//     box, and the last contacts; where the section was, one line says where it went, with a button that
//     brings it back by closing the window. The window closing any way brings it back too: Tauri's
//     `tauri://destroyed` for that window, and for no other.
//   · Ctrl+O still lands in the main window's operator box.
//   · A restart brings the section back: nothing remembers that it was out.
//   · The window holds the whole section and works as it does in the main window: the contest switch
//     (whose save the main window follows), Running / S&P, the exports, the club board, the scoreboard,
//     the county or sections board, the bonuses and the log. Its F-keys are held as the Logger window
//     holds them, and its Esc is the station's stop (`halt_tx`), as the main window's is.
//   · The control is in the main window only: not in the Logger window, in the window itself, or on the
//     Remote page.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent, act, within } from '@testing-library/react'
import App from './App'
import { DetachedPanel } from './DetachedPanel'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
import { CONTEST_SCREEN_SAVED_KEY } from './components/ContestScreenWindow'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

// ── the recorder ────────────────────────────────────────────────────────────────────────────
type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []
const mark = (): number => bridgeCalls.length
const sentSince = (from: number, cmd: string) => bridgeCalls.slice(from).filter((c) => c.cmd === cmd).map((c) => c.args)

/** What transmits, or decides whether or what the station transmits: the list the Contest screen's
 *  entry is held to (App.contestScreenEntry.test.tsx), the whole-settings save among them. */
const KEYING = new Set([
  'set_ptt', 'halt_tx', 'set_tune', 'atu_tune', 'set_tx_enabled', 'stop_cw', 'send_cw', 'send_cw_armed',
  'stop_voice', 'play_voice_message', 'set_mode', 'start_running', 'call_station', 'qso_resend', 'qso_freetext',
  'override_next_tx', 'log_current_qso', 'set_tx_even', 'set_tx_cycle_auto', 'set_tx_offset', 'set_tx_level',
  'set_rf_power', 'rtty_send', 'rtty_stop', 'rtty_set_auto', 'rtty_auto_abort', 'psk_send', 'psk_stop',
  'sstv_stop', 'arm_stream_mic', 'release_stream_mic', 'set_settings', 'set_operating_mode', 'set_frequency',
  'set_contest_esm',
])
const keyingSince = (from: number): string[] => bridgeCalls.slice(from).map((c) => c.cmd).filter((c) => KEYING.has(c))

// ── Tauri's event bridge ────────────────────────────────────────────────────────────────────
// `window.__TAURI__.event.listen(event, handler, { target })`, as `withGlobalTauri` exposes it, and the one
// event this feature hears: when a window is destroyed, Tauri sends `tauri://destroyed` to every listener
// whose target names that window, and to every listener with no target (tauri 2, `manager/window.rs`
// `on_window_event` and `event/listener.rs` `match_any_or_filter`). A listener with no target hears every
// window's close, so a test that closes a DIFFERENT window tells the two apart.
type Target = { kind: string; label?: string }
type Listener = { event: string; target: Target; handler: (e: { event: string; id: number; payload: unknown }) => void }
const listeners: Listener[] = []
function installEventBridge() {
  ;(window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    event: {
      listen: async (event: string, handler: Listener['handler'], options?: { target?: string | Target }) => {
        const target: Target =
          typeof options?.target === 'string' ? { kind: 'AnyLabel', label: options.target } : (options?.target ?? { kind: 'Any' })
        const l = { event, target, handler }
        listeners.push(l)
        return () => {
          const i = listeners.indexOf(l)
          if (i >= 0) listeners.splice(i, 1)
        }
      },
    },
  }
}
/** The window labelled `label` is destroyed, closed any way at all. */
async function windowDestroyed(label: string) {
  await act(async () => {
    for (const l of [...listeners]) {
      if (l.event !== 'tauri://destroyed') continue
      const named = (l.target.kind === 'Window' || l.target.kind === 'WebviewWindow') && l.target.label === label
      if (l.target.kind === 'Any' || named) l.handler({ event: 'tauri://destroyed', id: 1, payload: null })
    }
  })
}

// ── the station: the Illinois QSO Party from Kane County, on 20 m phone, a club hosted here ─
const T0 = 1_792_342_800
const row = (mins: number, call: string, qth: string) => ({
  call,
  class: '',
  section: '',
  band: '20m',
  mode: 'PH',
  submode: 'USB',
  whenUnix: T0 - mins * 60,
  rcvd: ['59', qth],
})
const LOG = [row(41, 'N9ZZZ', 'LAKE'), row(29, 'K9AAA', 'KANE'), row(2, 'K9DEF', 'WILL')]
const position = (posid: string, posName: string, band: string, operator: string, qsos: number, link: string) => ({
  posid, posName, band, mode: 'PH', operator, qsos, rate: 0, lastSeenSecs: 0, link,
})
const CLUB = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: true,
  event: 'W9XYZ IL QSO Party',
  hostCall: 'W9XYZ',
  score: 12,
  qsos: 3,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  dkeys: [],
  board: [position('aaaa0001', 'Comms Trailer', '20m', 'W9OPX', 3, 'host'), position('bbbb0002', 'Phone tent', '40m', 'N9OPB', 0, 'connected')],
  clubCode: '0HPZ-9WCN',
}
const ILQP = {
  running: false,
  state: 'Idle',
  qsoCount: LOG.length,
  sections: 0,
  points: LOG.length * 2,
  multCount: 3,
  event: 'ilqp',
  role: 'in_state',
  log: LOG,
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  composing: [
    { key: 'RST', raw: '59' },
    { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
  ],
  composingText: '59 KANE',
  sentExchange: 'KANE',
  assistanceOn: [],
  upload: { enabled: false, destinations: [], available: ['qrz', 'clublog'], hint: 'Off means not queued.' },
  boards: [{ id: 'QTH', slot: 'QTH', domain: 'il_counties', scope: 'perLog', worked: ['KANE', 'LAKE', 'WILL'] }],
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
  club: CLUB,
}
/** ARRL Field Day instead, for its bonuses and its sections board. */
const ARRL_FD = {
  ...ILQP,
  event: 'arrlfd',
  role: undefined,
  multCount: undefined,
  receives: [
    { key: 'CLASS', kind: 'fdClass', required: true },
    { key: 'SECTION', kind: 'oneOf', required: true, domains: ['fd_sections'] },
  ],
  composing: [
    { key: 'CLASS', raw: '3A' },
    { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
  ],
  composingText: '3A IL',
  sentExchange: '3A IL',
  log: LOG.map((r) => ({ ...r, class: '2A', section: 'WI', rcvd: ['2A', 'WI'] })),
  boards: undefined,
  club: undefined,
}

let fieldDay: unknown = ILQP
let transmitting = false
let settingsAnswer: Record<string, unknown> = {}

const snapshot = () => ({
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: fieldDay ? 'fieldDay' : 'Normal',
  radio: {
    dialMhz: 14.25,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    rigMode: 'USB',
    operatingMode: 'phone',
    transmitting,
    txEnabled: true,
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
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|set_mode|select_peer|set_fd_operator|set_settings|contest_log_manual|contest_working|contest_entry_reset)$/.test(cmd)) {
    return snapshot()
  }
  if (cmd === 'contest_log_manual_rows') return [true]
  if (cmd === 'contest_entry_put') return 1
  if (cmd === 'get_settings') return settingsAnswer
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log|contest_removed|contest_unmerged|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|contest_place|get_call_history|fd_club_log_newest)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  // A computer running its own station: the backend answers None, and the banner asks once. The
  // catch-all's `{}` would read as driving a remote station, asked every half second.
  if (cmd === 'client_status') return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  bridgeCalls.length = 0
  listeners.length = 0
  fieldDay = ILQP
  transmitting = false
  settingsAnswer = {
    ...defaultSettings,
    mycall: 'W9XYZ',
    mygrid: 'EN52',
    fdActive: true,
    fdEvent: 'ilqp',
    fdOperator: 'W9OPX',
    fdHostEnable: true,
  }
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd) as T
    },
  }
  installEventBridge()
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(allFeatureIds().map((id) => [id, true])) }),
  )
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
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
  window.history.replaceState(null, '', '/')
})

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))

/** The section's Pop out control, by its accessible name, under `root`. */
const popOutControls = (root: ParentNode) =>
  [...root.querySelectorAll<HTMLButtonElement>('button')].filter((b) => b.getAttribute('aria-label') === t('contestScreen.popOut.aria'))
/** The line that says where the section went. */
const awayLine = (root: ParentNode) => root.querySelector<HTMLElement>('.contest-screen-away')
const bringBack = (root: ParentNode) =>
  within(awayLine(root)!).getByRole('button', { name: t('contestScreen.away.back') })
/** Every operator box in the document, by its accessible name. */
const operatorBoxes = () =>
  [...document.querySelectorAll<HTMLInputElement>('input')].filter((i) => i.getAttribute('aria-label') === t('fieldDay.operator.aria'))

/** The real App on the Contest screen, past its boot snapshot. */
async function mountContestScreen(): Promise<HTMLElement> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = '#fieldDay'
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 15_000 })
  return waitFor(
    () => {
      const host = document.querySelector<HTMLElement>('.contest-screen')
      expect(host?.querySelector('section.panel.fieldday'), 'control: the Contest screen is on show').toBeTruthy()
      if (fieldDay) expect(host!.querySelector('.le-fd-input-call'), 'the Contest screen shows no entry').toBeTruthy()
      return host!
    },
    { timeout: 15_000 },
  )
}

/** Press the section's Pop out, and wait for the main screen to say the section is in its window. */
async function popOut(host: HTMLElement): Promise<void> {
  const [control] = popOutControls(host)
  expect(control, 'the Contest screen has no Pop out for its lower section').toBeTruthy()
  fireEvent.click(control)
  await waitFor(() => expect(awayLine(host), 'the main screen never said the section is in its own window').not.toBeNull())
}

/** A pop-out window, by its panel slug. */
async function mountPopOut(panel: 'contestscreen' | 'contestlog'): Promise<HTMLElement> {
  window.history.replaceState(null, '', `/?panel=${panel}`)
  render(<DetachedPanel panel={panel} />)
  return waitFor(
    () => {
      const app = document.querySelector<HTMLElement>('.app.detached')
      expect(app?.querySelector('section.panel.fieldday'), `control: the ${panel} window drew the contest screen`).toBeTruthy()
      return app!
    },
    { timeout: 15_000 },
  )
}

// ── the main window ─────────────────────────────────────────────────────────────────────────

describe('the main window’s Contest screen: the lower section pops out', () => {
  it('the control is on the lower section and opens the contest screen window', async () => {
    const host = await mountContestScreen()
    const controls = popOutControls(host)
    expect(controls, 'one Pop out for the section').toHaveLength(1)
    expect(controls[0].closest('section.panel.fieldday .fd-event-banner'), 'on the section’s own banner').not.toBeNull()
    expect(controls[0].closest('.contest-logger-entry'), 'not on the entry').toBeNull()
    const from = mark()
    fireEvent.click(controls[0])
    await waitFor(() => expect(sentSince(from, 'open_panel_window')).toEqual([{ panel: 'contestscreen' }]))
    await settle()
    expect(keyingSince(from), 'opening a window transmits nothing').toEqual([])
  })

  it('while it is out the screen keeps the entry, its operator box and the last contacts, and one line with a way back', async () => {
    const host = await mountContestScreen()
    await popOut(host)
    expect(host.querySelector('section.panel.fieldday'), 'the lower section is still in the main window').toBeNull()
    expect(host.querySelector('.fd-event-banner [role="switch"]'), 'the contest switch is still here').toBeNull()
    expect(host.querySelector('.contest-logger-entry .le-fd-input-call'), 'the entry went with it').not.toBeNull()
    expect(host.querySelector('.contest-logger-recent'), 'the last contacts went with it').not.toBeNull()
    const boxes = operatorBoxes()
    expect(boxes, 'one operator box, on the entry’s title line').toHaveLength(1)
    expect(boxes[0].closest('.contest-logger-entry .le-fd-header')).not.toBeNull()
    const line = awayLine(host)!
    expect(line.textContent).toContain(t('contestScreen.away.text'))
    expect(bringBack(host)).toBeTruthy()
    // The line is where the section was: after the last contacts.
    expect(host.querySelector('.contest-logger-recent')!.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('Bring it back closes the window and puts the section back', async () => {
    const host = await mountContestScreen()
    await popOut(host)
    const from = mark()
    fireEvent.click(bringBack(host))
    await waitFor(() => expect(sentSince(from, 'close_panel_window')).toEqual([{ panel: 'contestscreen' }]))
    await waitFor(() => expect(host.querySelector('section.panel.fieldday'), 'the section came back').not.toBeNull())
    expect(awayLine(host)).toBeNull()
    expect(popOutControls(host), 'and its Pop out with it').toHaveLength(1)
    expect(keyingSince(from)).toEqual([])
  })

  it('the window closing any way brings the section back — that window, and no other', async () => {
    const host = await mountContestScreen()
    await popOut(host)
    // Another window closing (the Logger window, the torn-off scoreboard) leaves it out.
    await windowDestroyed('panel-contestlog')
    await windowDestroyed('panel-fieldday')
    await settle()
    expect(awayLine(host), 'another window’s close brought the section back').not.toBeNull()
    expect(host.querySelector('section.panel.fieldday')).toBeNull()
    // Its own close — its title bar, Alt+F4, the quit — brings it back.
    await windowDestroyed('panel-contestscreen')
    await waitFor(() => expect(host.querySelector('section.panel.fieldday'), 'the window closed and the section stayed away').not.toBeNull())
    expect(awayLine(host)).toBeNull()
  })

  it('Ctrl+O from the log line still lands in the main window’s operator box', async () => {
    const host = await mountContestScreen()
    await popOut(host)
    const call = host.querySelector<HTMLInputElement>('.contest-logger-entry .le-fd-input-call')!
    call.focus()
    expect(document.activeElement).toBe(call)
    await act(async () => {
      expect(fireEvent.keyDown(call, { key: 'o', code: 'KeyO', ctrlKey: true }), 'Ctrl+O is the log line’s').toBe(false)
    })
    const [box] = operatorBoxes()
    await waitFor(() => expect(document.activeElement).toBe(box))
    expect(box.closest('.contest-logger-entry .le-fd-header'), 'the box on the entry’s title line').not.toBeNull()
  })

  it('a save in the window (its contest switch) is followed here: the settings and the snapshot are read again', async () => {
    await mountContestScreen()
    await settle()
    const from = mark()
    await act(async () => {
      window.dispatchEvent(new StorageEvent('storage', { key: CONTEST_SCREEN_SAVED_KEY, newValue: String(Date.now()) }))
    })
    await waitFor(() => expect(sentSince(from, 'get_settings').length, 'the main window never read its settings again').toBeGreaterThan(0))
    await waitFor(() => expect(sentSince(from, 'get_snapshot').length).toBeGreaterThan(0))
    // Control: an unrelated key is not a save.
    await settle()
    const quiet = mark()
    await act(async () => {
      window.dispatchEvent(new StorageEvent('storage', { key: 'nexus.waterfall.gain', newValue: '3' }))
    })
    await settle()
    expect(sentSince(quiet, 'get_settings')).toEqual([])
  })

  it('a restart brings the section back: nothing remembers that it was out', async () => {
    const host = await mountContestScreen()
    await popOut(host)
    cleanup()
    const again = await mountContestScreen()
    expect(again.querySelector('section.panel.fieldday'), 'the section stayed away across a restart').not.toBeNull()
    expect(awayLine(again)).toBeNull()
  })

  it('with no contest running it pops out too, and the line stands alone', async () => {
    fieldDay = null
    settingsAnswer = { ...settingsAnswer, fdActive: false }
    const host = await mountContestScreen()
    await popOut(host)
    expect(host.querySelector('section.panel.fieldday')).toBeNull()
    expect(host.querySelector('.contest-logger-entry'), 'control: no entry without a contest').toBeNull()
    expect(bringBack(host)).toBeTruthy()
  })
})

// ── the window ──────────────────────────────────────────────────────────────────────────────

describe('the contest screen window holds the whole lower section', () => {
  it('the contest switch, the controls, the club board, the scoreboard, the county board and the log', async () => {
    const app = await mountPopOut('contestscreen')
    const screen = app.querySelector<HTMLElement>('section.panel.fieldday')!
    expect(screen.querySelector('.fd-event-banner [role="switch"]'), 'the contest switch').not.toBeNull()
    const controls = screen.querySelector<HTMLElement>('.fd-header')!
    expect(within(controls).getByRole('button', { name: t('fieldDay.role.running') })).toBeTruthy()
    expect(within(controls).getByRole('button', { name: t('fieldDay.role.sp') })).toBeTruthy()
    expect(within(controls).getByRole('button', { name: t('fieldDay.export.cabrillo.label') })).toBeTruthy()
    expect(within(controls).getByRole('button', { name: t('fieldDay.logger.label') }), 'the Logger window opens from here too').toBeTruthy()
    expect(screen.querySelector(`[aria-label="${t('fieldDay.club.aria')}"]`), 'the club board').not.toBeNull()
    expect(screen.querySelector('.fd-scoreboard'), 'the scoreboard').not.toBeNull()
    expect(screen.querySelector(`[aria-label="${t('fieldDay.board.aria', { name: 'QTH' })}"]`), 'the county board').not.toBeNull()
    expect(screen.querySelectorAll('.fd-log .fd-log-row'), 'the log').toHaveLength(LOG.length)
    // The operator box is on the main window's entry while a contest runs: the section draws none.
    expect(operatorBoxes(), 'a second operator box').toEqual([])
    // Nothing here pops it out again.
    expect(popOutControls(app)).toEqual([])
  })

  it('ARRL Field Day: the bonuses and the sections board', async () => {
    fieldDay = ARRL_FD
    settingsAnswer = { ...settingsAnswer, fdEvent: 'arrlfd', fdClass: '3A', fdSection: 'IL' }
    const app = await mountPopOut('contestscreen')
    await waitFor(() => expect(app.querySelector('.fd-bonuses-section'), 'the bonuses').not.toBeNull())
    expect(app.querySelector(`[aria-label="${t('fieldDay.sections.aria')}"]`), 'the sections board').not.toBeNull()
  })

  it('Running and S&P work here as on the main screen: the same mode request', async () => {
    const app = await mountPopOut('contestscreen')
    const from = mark()
    fireEvent.click(within(app.querySelector<HTMLElement>('.fd-header')!).getByRole('button', { name: t('fieldDay.role.running') }))
    await waitFor(() => expect(sentSince(from, 'set_mode')).toEqual([{ mode: 'fieldday-run' }]))
  })

  it('its contest switch saves as the main screen’s does, and tells the main window', async () => {
    fieldDay = null
    settingsAnswer = { ...settingsAnswer, fdActive: false }
    const app = await mountPopOut('contestscreen')
    const toggle = app.querySelector<HTMLButtonElement>('.fd-event-banner [role="switch"]')!
    await waitFor(() => expect(toggle.disabled).toBe(false))
    expect(localStorage.getItem(CONTEST_SCREEN_SAVED_KEY), 'control: nothing announced before the save').toBeNull()
    const from = mark()
    fireEvent.click(toggle)
    await waitFor(() => expect(sentSince(from, 'set_settings').map((a) => (a?.settings as { fdActive?: boolean })?.fdActive)).toEqual([true]))
    await waitFor(() => expect(localStorage.getItem(CONTEST_SCREEN_SAVED_KEY), 'the main window was never told').not.toBeNull())
    // This window reads its own settings again too, as the main window does after its own switch.
    await waitFor(() => expect(sentSince(from, 'get_settings').length).toBeGreaterThan(1))
  })

  it('its F-keys are held as the Logger window holds them: F5 does not reload it; Alt+F4 is still the system’s', async () => {
    await mountPopOut('contestscreen')
    for (const key of ['F1', 'F5', 'F12']) {
      expect(fireEvent.keyDown(document.body, { key, code: key }), `${key} reached the webview`).toBe(false)
    }
    expect(fireEvent.keyDown(document.body, { key: 'F4', code: 'F4', altKey: true }), 'Alt+F4 must still close the window').toBe(true)
  })

  it('its Esc is the station’s stop, as the main window’s is: one halt a press, and nothing else that transmits', async () => {
    transmitting = true
    const app = await mountPopOut('contestscreen')
    await settle()
    const from = mark()
    fireEvent.keyDown(app.querySelector('.fd-header')!, { key: 'Escape', code: 'Escape' })
    fireEvent.keyDown(document.body, { key: 'Escape', code: 'Escape' })
    await settle()
    expect(keyingSince(from)).toEqual(['halt_tx', 'halt_tx'])
  })
})

// ── where the control is not ────────────────────────────────────────────────────────────────

describe('the control is the main window’s alone', () => {
  it('not in the Logger window, which is a window of its own already', async () => {
    const app = await mountPopOut('contestlog')
    expect(app.querySelector('.fd-event-banner'), 'control: the Logger window drew the screen’s banner').not.toBeNull()
    expect(popOutControls(app)).toEqual([])
  })
})

// ── the save-failure line: once per window ──────────────────────────────────────────────────
// The contest journal cannot be written: one red line per window, never two. In the main window
// and the Logger window the entry above the screen carries it, so the screen below draws none;
// the popped-out screen has no entry, so it draws its own.

describe('a contest journal that cannot be written is said once in each window', () => {
  const SAVED = T0 - 300
  const since = t('fieldDay.unsaved.since', { time: new Date(SAVED * 1000).toISOString().slice(11, 16) })
  const unsavedLines = (root: ParentNode) => [...root.querySelectorAll<HTMLElement>('.fd-unsaved')]
  beforeEach(() => {
    fieldDay = { ...ILQP, unsaved: { failedWrites: 2, lastSavedUnix: SAVED } }
  })

  it('the main Contest screen: one line, on the entry, with the screen below on show', async () => {
    const host = await mountContestScreen()
    await settle()
    expect(host.querySelector('section.panel.fieldday'), 'control: the screen below the entry is on show').not.toBeNull()
    const lines = unsavedLines(host)
    expect(lines.map((l) => l.textContent)).toEqual([since])
    expect(lines[0].closest('.contest-logger-entry'), 'the line is on the entry').not.toBeNull()
  })

  it('the main Contest screen with its section popped out: still one line, on the entry', async () => {
    const host = await mountContestScreen()
    await popOut(host)
    const lines = unsavedLines(host)
    expect(lines.map((l) => l.textContent)).toEqual([since])
    expect(lines[0].closest('.contest-logger-entry')).not.toBeNull()
  })

  it('the popped-out Contest screen: one line, its own', async () => {
    const app = await mountPopOut('contestscreen')
    await settle()
    const lines = unsavedLines(app)
    expect(lines.map((l) => l.textContent)).toEqual([since])
    expect(lines[0].closest('section.panel.fieldday')).not.toBeNull()
  })

  it('the Logger window: one line, on its entry', async () => {
    const app = await mountPopOut('contestlog')
    await settle()
    const lines = unsavedLines(app)
    expect(lines.map((l) => l.textContent)).toEqual([since])
    expect(lines[0].closest('.contest-logger-entry')).not.toBeNull()
  })

  it('control: a write that lands takes the line away everywhere', async () => {
    fieldDay = ILQP
    const host = await mountContestScreen()
    await settle()
    expect(unsavedLines(host)).toEqual([])
  })
})

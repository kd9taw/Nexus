// @vitest-environment jsdom
//
// THE OPERATOR BOX IS AT THE TOP OF THE CONTEST SCREEN (operator, 2026-10-12, with the Illinois QSO
// Party running: "I want to move the field to change operators up near the top"). The pick: "On the
// title line beside Sending and I moved. Also shows in the Logger window, Ctrl+O lands there, and it
// leaves the scoreboard below (no second box on the screen)."
//
// AT THE WIRE, IN THE REAL APP AND THE REAL POP-OUTS (App.contestScreenEntry.test.tsx's idiom): the
// real `./api` under a recording bridge, so every assertion is about the command and the argument
// object that left the UI.
//   · While the entry shows (a contest running), the one operator box on the screen is on the
//     entry's title line, right beside Sending and I moved — in the main window and in the Logger
//     window — and the scoreboard below draws none. With no contest running the scoreboard keeps its
//     box, and the torn-off scoreboard window keeps its own.
//   · What is typed there is saved through the narrow write (`set_fd_operator`), uppercased, as the
//     box loses focus or Enter is pressed: never the whole-settings save, which a seat swap must not
//     run (ContestView.operator.test.tsx has why). Enter there logs nothing and leaves the contact in
//     the boxes as it was; the per-contact Tab order is still Call, the report, the exchange, Log.
//   · Ctrl+O from the log line puts the caret in it.
//   · Nothing transmits from it.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent, act, within } from '@testing-library/react'
import App from './App'
import { DetachedPanel } from './DetachedPanel'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
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

// ── the station: the Illinois QSO Party from Kane County, on 20 m phone ─────────────────────
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
  assistanceOn: [],
  upload: { enabled: false, destinations: [], available: [], hint: '' },
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
}

let fieldDay: unknown = ILQP
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
    transmitting: false,
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
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|set_mode|select_peer|set_fd_operator|contest_log_manual|contest_working|contest_entry_reset)$/.test(cmd)) {
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
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
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
  fieldDay = ILQP
  settingsAnswer = { ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52', fdActive: true, fdEvent: 'ilqp', fdOperator: 'W9OPX' }
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd) as T
    },
  }
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
  window.history.replaceState(null, '', '/')
})

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))

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

/** A pop-out window: the Logger window (`contestlog`) or the torn-off scoreboard (`fieldday`). */
async function mountPopOut(panel: 'contestlog' | 'fieldday'): Promise<HTMLElement> {
  window.history.replaceState(null, '', `/?panel=${panel}`)
  render(<DetachedPanel panel={panel} />)
  return waitFor(
    () => {
      const app = document.querySelector<HTMLElement>('.app.detached')
      expect(app?.querySelector(panel === 'contestlog' ? '.le-fd-input-call' : '.fd-scoreboard'), `control: the ${panel} window drew`).toBeTruthy()
      return app!
    },
    { timeout: 15_000 },
  )
}

/** Every operator box in the document, by its accessible name. */
const operatorBoxes = () =>
  [...document.querySelectorAll<HTMLInputElement>('input')].filter((i) => i.getAttribute('aria-label') === t('fieldDay.operator.aria'))
/** The one operator box, typable (its settings read). */
async function theBox(): Promise<HTMLInputElement> {
  return waitFor(
    () => {
      const boxes = operatorBoxes()
      expect(boxes, 'one operator box on the screen').toHaveLength(1)
      expect(boxes[0].disabled, 'the box waits for its settings').toBe(false)
      return boxes[0]
    },
    { timeout: 15_000 },
  )
}

/** The box is on the entry's title line, right beside Sending and its I moved. */
function expectOnTheTitleLine(box: HTMLInputElement) {
  const line = box.closest('.contest-logger-entry .le-fd-header')
  expect(line, 'the operator box is not on the entry’s title line').not.toBeNull()
  const sent = line!.querySelector('.le-fd-sent')!
  expect(sent, 'control: the title line holds Sending').toBeTruthy()
  expect(within(sent as HTMLElement).getByRole('button', { name: t('logEntry.sent.moved.label') }), 'control: and I moved').toBeTruthy()
  expect(box.closest('label')!.nextElementSibling, 'the box sits right beside Sending').toBe(sent)
  expect(box.closest('section.panel.fieldday'), 'the scoreboard drew it').toBeNull()
}

const boxesIn = (root: Element) => [...root.querySelectorAll<HTMLInputElement>('.le-fd-big input')]

/** Type a contact the way the boxes take one: a call not in the log, the report as offered, a county. */
async function typeContact(root: Element): Promise<HTMLInputElement[]> {
  const boxes = boxesIn(root)
  fireEvent.change(boxes[0], { target: { value: 'K9NEW' } })
  fireEvent.change(boxes[2], { target: { value: 'COOK' } })
  await settle()
  return boxes
}

// ── the main window ─────────────────────────────────────────────────────────────────────────

describe('the main window’s Contest screen: the operator box is on the entry’s title line', () => {
  it('with a contest running there is one box, beside Sending and I moved, and the scoreboard draws none', async () => {
    const host = await mountContestScreen()
    const box = await theBox()
    expectOnTheTitleLine(box)
    expect(box.value, 'it shows the operator at the key').toBe('W9OPX')
    // Control: the screen below is drawn, its scoreboard's row (Pop out) and score tiles with it.
    const screen = host.querySelector<HTMLElement>('section.panel.fieldday')!
    expect(within(screen).getByRole('button', { name: t('fieldDay.popOut.label') })).toBeTruthy()
    expect(screen.querySelector('.fd-scoreboard')).toBeTruthy()
  })

  it('what is typed there is saved through the narrow write, uppercased, as the box loses focus — and the top bar follows', async () => {
    await mountContestScreen()
    const box = await theBox()
    const from = mark()
    fireEvent.change(box, { target: { value: 'w1abc' } })
    expect(box.value, 'uppercase as it is typed').toBe('W1ABC')
    fireEvent.blur(box)
    await waitFor(() => expect(sentSince(from, 'set_fd_operator')).toEqual([{ call: 'W1ABC' }]))
    await settle()
    expect(keyingSince(from), 'never the whole-settings save, and nothing that transmits').toEqual([])
    expect(document.querySelector('.op-chip')?.textContent, 'the top bar’s operator says the same').toBe('OP W1ABC')
  })

  it('Enter there saves the operator and logs nothing: the contact in the boxes stays as it was', async () => {
    const host = await mountContestScreen()
    const box = await theBox()
    const entry = host.querySelector('.contest-logger-entry')!
    const boxes = await typeContact(entry)
    expect(boxes.map((b) => b.value)).toEqual(['K9NEW', '59', 'COOK'])
    box.focus()
    const from = mark()
    fireEvent.change(box, { target: { value: 'k9op' } })
    fireEvent.keyDown(box, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(sentSince(from, 'set_fd_operator')).toEqual([{ call: 'K9OP' }]))
    await settle()
    expect(sentSince(from, 'contest_log_manual'), 'Enter in the operator box logged a contact').toEqual([])
    expect(boxesIn(entry).map((b) => b.value), 'the contact is untouched').toEqual(['K9NEW', '59', 'COOK'])
    expect(keyingSince(from)).toEqual([])
    // Control: the same keystroke in the county box does log, so the log path is live here.
    fireEvent.keyDown(boxesIn(entry)[2], { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(sentSince(from, 'contest_log_manual')).toHaveLength(1))
  })

  it('Ctrl+O from the log line puts the caret in it', async () => {
    const host = await mountContestScreen()
    const box = await theBox()
    const call = host.querySelector<HTMLInputElement>('.contest-logger-entry .le-fd-input-call')!
    call.focus()
    expect(document.activeElement).toBe(call)
    await act(async () => {
      expect(fireEvent.keyDown(call, { key: 'o', code: 'KeyO', ctrlKey: true }), 'Ctrl+O is the log line’s').toBe(false)
    })
    await waitFor(() => expect(document.activeElement).toBe(box))
    expectOnTheTitleLine(box)
  })

  it('the per-contact Tab order is still Call, the report, the exchange, Log — the box comes before it', async () => {
    const host = await mountContestScreen()
    const box = await theBox()
    const entry = host.querySelector('.contest-logger-entry')!
    const [call, rst, qth] = await typeContact(entry)
    const log = entry.querySelector<HTMLButtonElement>('.le-fd-log-btn')!
    expect(log.disabled, 'control: the contact is complete, so Log takes the key').toBe(false)
    // Tab is plain document order here: no element in the entry carries a tabIndex of its own.
    const tabbable = [...entry.querySelectorAll<HTMLElement>('input, button, select, textarea, [tabindex]')].filter(
      (el) => !(el as HTMLInputElement).disabled && el.tabIndex >= 0,
    )
    const a = tabbable.indexOf(call)
    const b = tabbable.indexOf(log)
    expect(tabbable.slice(a, b + 1).filter((el) => el.tagName === 'INPUT')).toEqual([call, rst, qth])
    expect(tabbable.slice(a, b + 1)).not.toContain(box)
    const at = tabbable.indexOf(box)
    expect(at, 'the box is in the entry, a stop of its own').toBeGreaterThanOrEqual(0)
    expect(at, 'the box is before the Call box, on the line above it').toBeLessThan(a)
  })

  it('with no contest running the scoreboard keeps its box, and it is the only one', async () => {
    fieldDay = null
    settingsAnswer = { ...settingsAnswer, fdActive: false }
    const host = await mountContestScreen()
    const box = await theBox()
    expect(host.querySelector('.contest-logger-entry'), 'control: no entry without a contest').toBeNull()
    expect(box.closest('section.panel.fieldday'), 'the scoreboard’s box').not.toBeNull()
    box.focus()
    const from = mark()
    fireEvent.change(box, { target: { value: 'n9xyz' } })
    fireEvent.keyDown(box, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(sentSince(from, 'set_fd_operator')).toEqual([{ call: 'N9XYZ' }]))
    expect(keyingSince(from)).toEqual([])
  })
})

// ── the Logger window and the torn-off scoreboard ───────────────────────────────────────────

describe('the Logger window: the same box on its own entry’s title line', () => {
  it('one box, beside Sending and I moved; Ctrl+O lands in it; Enter saves it and logs nothing', async () => {
    const app = await mountPopOut('contestlog')
    const box = await theBox()
    expectOnTheTitleLine(box)
    const call = app.querySelector<HTMLInputElement>('.le-fd-input-call')!
    call.focus()
    await act(async () => {
      expect(fireEvent.keyDown(call, { key: 'o', code: 'KeyO', ctrlKey: true })).toBe(false)
    })
    await waitFor(() => expect(document.activeElement).toBe(box))
    const boxes = await typeContact(app)
    box.focus()
    const from = mark()
    fireEvent.change(box, { target: { value: 'kd9op' } })
    fireEvent.keyDown(box, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(sentSince(from, 'set_fd_operator')).toEqual([{ call: 'KD9OP' }]))
    await settle()
    expect(sentSince(from, 'contest_log_manual'), 'Enter in the operator box logged a contact').toEqual([])
    expect(boxes.map((b) => b.value)).toEqual(['K9NEW', '59', 'COOK'])
    expect(keyingSince(from)).toEqual([])
  })
})

describe('the torn-off scoreboard keeps its own box', () => {
  it('one box, in the scoreboard, saved through the same narrow write', async () => {
    await mountPopOut('fieldday')
    const box = await theBox()
    expect(box.closest('.le-fd-header'), 'there is no log line in this window').toBeNull()
    const from = mark()
    fireEvent.change(box, { target: { value: 'w9new' } })
    fireEvent.blur(box)
    await waitFor(() => expect(sentSince(from, 'set_fd_operator')).toEqual([{ call: 'W9NEW' }]))
    expect(keyingSince(from)).toEqual([])
  })
})

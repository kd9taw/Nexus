// @vitest-environment jsdom
//
// THE CONTEST SCREEN TAKES THE CONTACTS (2026-10-11). Rehearsing the Illinois QSO Party on two
// laptops, the operator found nowhere on the main window's Contest screen to type a contact, and
// picked "Big entry on Contest": "The Logger window's large Call / RST / County boxes, dupe warning
// and last contacts at the top of the Contest screen. Same log line, so a contact logs once; it never
// transmits."
//
// AT THE WIRE, IN THE REAL APP (stop-control-wiring.test.tsx's idiom): the real `./api` under a
// recording bridge, so every assertion is about the command and the argument object that left the UI.
//   · The screen's order: the Logger window's entry (its boxes, the dupe warning under them), the last
//     contacts, then the screen; with no contest running, the screen alone, exactly as before.
//   · Esc in the Call box while the rig is keyed sends the station's halt, once, and leaves the
//     contact in the boxes. Every other key in every box, and every button of the entry, sends
//     nothing that transmits; Enter logs and sends no message, Enter Sends Message switched on in
//     every cockpit.
//   · Enter sends the very bytes the Phone screen's LOG pane sends for the same keystrokes.
//   · A− / A+ is remembered on this computer for the main window, under its own key; and the app's
//     Text size reaches the boxes, computed through the real sheets (jsdom lays nothing out, so the
//     font size is walked, as ContestLogger.entryFirst.test.tsx walks the logger window's).
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent, act, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import App from './App'
import { DetachedPanel } from './DetachedPanel'
import { contestName } from './fdEvent'
import { allFeatureIds } from './features/registry'
import { pastTheSwitch } from './components/ConnectView.testkit'
import {
  BASE_MODES,
  MODES,
  chainOf,
  cmpSpec,
  contrast,
  elOf,
  expandWith,
  parseRules,
  reachesChain,
  rgbHex as hex,
  rootTokensFrom,
  toRgb,
  tokensAt,
  winnerAt,
  type El,
  type Mode,
  type Rgb,
  type Rule,
} from './cssCascade'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000, hookTimeout: 30_000 })

// ── the recorder ────────────────────────────────────────────────────────────────────────────
type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []
const mark = (): number => bridgeCalls.length

/** What transmits, or decides whether or what the station transmits: the logger window's reviewed
 *  list (ContestLogger.txfree.test.tsx) with the stop wiring's verbs. */
const KEYING = new Set([
  'set_ptt', 'halt_tx', 'set_tune', 'atu_tune', 'set_tx_enabled', 'stop_cw', 'send_cw', 'send_cw_armed',
  'stop_voice', 'play_voice_message', 'set_mode', 'start_running', 'call_station', 'qso_resend', 'qso_freetext',
  'override_next_tx', 'log_current_qso', 'set_tx_even', 'set_tx_cycle_auto', 'set_tx_offset', 'set_tx_level',
  'set_rf_power', 'rtty_send', 'rtty_stop', 'rtty_set_auto', 'rtty_auto_abort', 'psk_send', 'psk_stop',
  'sstv_stop', 'arm_stream_mic', 'release_stream_mic', 'set_settings', 'set_operating_mode', 'set_frequency',
  'set_contest_esm',
])
/** The transmit-path commands sent since `from`, in order. */
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
/** Oldest first, as the engine keeps it. K9ABC from COOK on 20 m phone is the second contact. */
const LOG = [
  row(41, 'N9ZZZ', 'LAKE'),
  row(37, 'K9ABC', 'COOK'),
  row(29, 'K9AAA', 'KANE'),
  row(18, 'N9AAA', 'DUPG'),
  row(9, 'W0AAA', 'MO'),
  row(2, 'K9DEF', 'WILL'),
]
const ILQP = {
  running: false,
  state: 'Idle',
  qsoCount: LOG.length,
  sections: 0,
  points: LOG.length * 2,
  multCount: 5,
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
const ILQP_RULES = {
  event: 'ilqp',
  rulesYear: 2026,
  bannedModes: ['FT8', 'FT4'],
  spottingAllowed: true,
  clusterAllowed: true,
  enforcement: 'warn',
  role: 'in_state',
  exchange: [],
  problem: '',
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
}

/** Keyed or not; the section the station is on and the FT tier it runs; the contest, or none
 *  (Contest mode off), and its ruleset. */
let transmitting = false
let operatingMode = 'phone'
let tier = 'Ft8'
let fieldDay: unknown = ILQP
let rules: Record<string, unknown> = ILQP_RULES
let settingsAnswer: Record<string, unknown> = {}
/** The satellite the engine holds a transponder of (`get_sat_transponder`), when a case sets one. */
let held: unknown = null

const snapshot = () => ({
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: fieldDay ? 'fieldDay' : 'Normal',
  radio: {
    dialMhz: operatingMode === 'digital' ? 14.074 : 14.25,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    rigMode: operatingMode === 'digital' ? 'PKTUSB' : 'USB',
    operatingMode,
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
  link: { tier, periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay,
  recentDecodes: [],
  harqRescues: 0,
})

function respond(cmd: string): unknown {
  if (cmd === 'halt_tx') transmitting = false
  if (/^(get_snapshot|halt_tx|set_operating_mode|set_area|set_mode|select_peer|contest_log_manual|contest_working|contest_entry_reset)$/.test(cmd)) {
    return snapshot()
  }
  if (cmd === 'contest_log_manual_rows') return [true]
  if (cmd === 'get_settings') return settingsAnswer
  if (cmd === 'get_fd_ruleset') return rules
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log|contest_removed|contest_unmerged|get_scp_calls|get_freq_markers)$/.test(cmd)) return []
  if (/^(get_propagation|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  if (cmd === 'ask_log') throw new Error('no log in this test')
  // A computer running its own station: the backend answers None, and the banner asks once. The
  // catch-all's `{}` would read as driving a remote station, asked every half second.
  if (cmd === 'client_status') return null
  // Only a case that holds a bird answers with one; every other case gets what it always got.
  if (cmd === 'get_sat_transponder' && held) return held
  return {}
}

beforeEach(() => {
  localStorage.clear()
  pastTheSwitch()
  bridgeCalls.length = 0
  transmitting = false
  operatingMode = 'phone'
  tier = 'Ft8'
  fieldDay = ILQP
  rules = ILQP_RULES
  held = null
  settingsAnswer = { ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52', fdActive: true, fdEvent: 'ilqp' }
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
  document.documentElement.removeAttribute('data-text-size')
})

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))

/** The real App on `view`, past its boot snapshot. */
async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 15_000 })
  await settle()
}

/** The Contest screen with its entry on show. */
async function contestScreen(): Promise<HTMLElement> {
  await mountOn('fieldDay')
  return waitFor(
    () => {
      const host = document.querySelector<HTMLElement>('.contest-screen')
      expect(host?.querySelector('.le-fd-input-call'), 'the Contest screen shows no entry').toBeTruthy()
      return host!
    },
    { timeout: 15_000 },
  )
}

const boxesIn = (root: Element) => [...root.querySelectorAll<HTMLInputElement>('.le-fd-big input')]

/** Type a contact the way the boxes take one: the call, the report left as offered, the county. */
async function typeContact(root: Element): Promise<HTMLInputElement[]> {
  const boxes = boxesIn(root)
  fireEvent.change(boxes[0], { target: { value: 'K9ABC' } })
  fireEvent.change(boxes[2], { target: { value: 'COOK' } })
  await settle()
  return boxes
}

// ── the screen ──────────────────────────────────────────────────────────────────────────────

describe('the main window’s Contest screen takes contacts', () => {
  it('the entry first — the boxes, the dupe warning right under them — then the last contacts, then the screen', async () => {
    const host = await contestScreen()
    const tagged = (el: Element) => `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}`
    expect([...host.children].map(tagged)).toEqual([
      'section.contest-logger-entry',
      'section.contest-logger-recent',
      // The divider that sizes the last contacts against the screen under them.
      'div.pane-splitter.horizontal.contest-seam',
      'main.layout.single',
    ])
    const [entry, recent, , screen] = [...host.children] as HTMLElement[]
    const boxes = await typeContact(entry)
    expect(boxes.map((b) => b.value)).toEqual(['K9ABC', '59', 'COOK'])
    // The band and mode the contact logs under, in the header row the Phone LOG pane shows.
    expect(entry.querySelector('.le-fd-mode')!.textContent).toBe('PH')
    expect(entry.querySelector('.le-fd-header')!.textContent).toContain(t('logEntry.contest.hint', { band: '20m' }))
    // The dupe warning, as the log line computes it, after the boxes.
    const dupe = within(entry).getByRole('alert')
    expect(dupe.textContent).toBe(t('logEntry.fd.dupe.own', { call: 'K9ABC', band: '20m', mode: 'PH' }))
    expect(entry.querySelector('.le-fd-big')!.compareDocumentPosition(dupe) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    // The newest ten (here all six), newest first.
    expect([...recent.querySelectorAll('.fd-log-row .fd-col.call')].map((c) => c.textContent)).toEqual([
      'K9DEF',
      'W0AAA',
      'N9AAA',
      'K9AAA',
      'K9ABC',
      'N9ZZZ',
    ])
    // The screen follows, the whole log in it, oldest first.
    expect([...screen.querySelectorAll('.fd-log-row .fd-col.call')].map((c) => c.textContent)).toEqual(LOG.map((q) => q.call))
    // One entry, in the main window as in the logger window.
    expect(document.querySelectorAll('.contest-logger-entry')).toHaveLength(1)
  })

  it('with no contest running the screen is as it was: no entry, no last contacts', async () => {
    fieldDay = null
    await mountOn('fieldDay')
    const host = await waitFor(() => {
      const h = document.querySelector('.contest-screen')
      expect(h?.querySelector('section.panel.fieldday'), 'control: the Contest screen is on show').toBeTruthy()
      return h!
    })
    expect([...host.children].map((c) => c.tagName.toLowerCase() + '.' + [...c.classList].join('.'))).toEqual(['main.layout.single'])
    expect(document.querySelector('.contest-logger-entry, .contest-logger-recent, .le-fd-input-call')).toBeNull()
  })
})

// ── the mode the contest does not permit ────────────────────────────────────────────────────
//
// ⭐ A SMALL CHIP ON THE TOP ROW, NOT A BAND (2026-10-12). Benching the Illinois QSO Party on FT8, the
// operator found the full-width yellow band over the log line's top row "very verbose and yellow and
// distracting", and picked "A small outlined chip beside CONTEST LOG / PH that reads 'FT8 · no
// credit'; hover it for the full sentence. No yellow band." So the row reads, left to right, CONTEST
// LOG · DIG · FT8 · no credit · the band line. The screen's banner, further down, and the FT screen's
// header drew the same sentence as an amber line, and say it with the same chip: "Same small chip with
// the full sentence on hover, in the Contest banner and the FT screen header".

describe('a mode the contest does not permit', () => {
  const banned = (mode: string) =>
    t('fieldDay.advisory.banned', { mode, event: contestName('ilqp'), year: 2026 })
  /** What the entry's top row shows right after its mode chip. */
  const afterMode = (entry: Element) => entry.querySelector('.le-fd-header > .le-fd-mode')!.nextElementSibling!
  /** What the screen's banner shows right after the contest's window. */
  const afterWindow = (host: Element) => host.querySelector('.fd-event-banner > .fd-event-subtitle')!.nextElementSibling!

  it('on FT8 the top row says "FT8 · no credit" right after the mode, the sentence on hover; no band; the contact still logs', async () => {
    operatingMode = 'digital'
    const host = await contestScreen()
    const entry = host.querySelector<HTMLElement>('.contest-logger-entry')!
    expect(entry.querySelector('.le-fd-mode')!.textContent).toBe('DIG')
    const chip = afterMode(entry)
    expect(chip.textContent).toBe('FT8 · no credit')
    expect(chip.getAttribute('title')).toBe(banned('FT8'))
    // A screen reader is told the same: the chip's words, and the whole sentence as its description.
    expect(within(entry).getByRole('status', { name: 'FT8 · no credit', description: banned('FT8') })).toBe(chip)
    // No band: the sentence is not drawn anywhere in the entry.
    expect(entry.querySelector('.fd-advisory')).toBeNull()
    expect(entry.textContent).not.toContain(banned('FT8'))
    // The screen's banner says it with the same chip, where its amber line stood; the line is gone.
    const banner = host.querySelector<HTMLElement>('.fd-event-banner')!
    expect(afterWindow(host).textContent).toBe('FT8 · no credit')
    expect(afterWindow(host).getAttribute('title')).toBe(banned('FT8'))
    expect(within(banner).getByRole('status', { name: 'FT8 · no credit', description: banned('FT8') })).toBe(afterWindow(host))
    expect(banner.querySelector('.fd-advisory')).toBeNull()
    expect(banner.textContent).not.toContain(banned('FT8'))
    // The rules line says what the contest permits, as it always did.
    expect(host.querySelector('.fd-mode-line.banned')?.textContent).toBe(t('fieldDay.mode.rule.banned', { modes: 'FT8, FT4' }))
    // Logging stays allowed: the contact goes, under the digital class, with the tier the station
    // runs named behind it (this fixture's spelling of it).
    const boxes = await typeContact(entry)
    const from = mark()
    fireEvent.keyDown(boxes[2], { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(bridgeCalls.slice(from).map((c) => c.cmd)).toContain('contest_log_manual'))
    expect(bridgeCalls.slice(from).find((c) => c.cmd === 'contest_log_manual')!.args).toEqual({
      call: 'K9ABC',
      fields: [['RST', '599'], ['QTH', 'COOK']],
      mode: 'DIG',
      submode: 'Ft8',
    })
  })

  it('on FT4 the chip names FT4, the mode the station is on', async () => {
    operatingMode = 'digital'
    tier = 'Ft4'
    const host = await contestScreen()
    const chip = afterMode(host.querySelector('.contest-logger-entry')!)
    expect(chip.textContent).toBe('FT4 · no credit')
    expect(chip.getAttribute('title')).toBe(banned('FT4'))
    expect(afterWindow(host).textContent).toBe('FT4 · no credit')
    expect(afterWindow(host).getAttribute('title')).toBe(banned('FT4'))
  })

  it('on FT8 at a contest that credits it there is no chip: the band line follows the mode', async () => {
    operatingMode = 'digital'
    rules = { ...ILQP_RULES, bannedModes: [] }
    const host = await contestScreen()
    const entry = host.querySelector('.contest-logger-entry')!
    expect(afterMode(entry).textContent).toBe(t('logEntry.contest.hint', { band: '20m' }))
    expect(entry.querySelector('.le-fd-header')!.textContent).not.toContain('no credit')
    expect(host.querySelector('.fd-advisory, .le-fd-nocredit')).toBeNull()
  })

  it('on phone neither the entry nor the header warns — the FT tier left at FT8 is not the station’s mode', async () => {
    const host = await contestScreen()
    const entry = host.querySelector('.contest-logger-entry')!
    expect(entry.querySelector('.le-fd-mode')!.textContent).toBe('PH')
    expect(afterMode(entry).textContent).toBe(t('logEntry.contest.hint', { band: '20m' }))
    expect(entry.querySelector('.le-fd-header')!.textContent).not.toContain('no credit')
    expect(host.querySelector('.fd-advisory, .le-fd-nocredit')).toBeNull()
    // The rules line still says what the contest permits.
    expect(host.querySelector('.fd-mode-line.banned')?.textContent).toBe(t('fieldDay.mode.rule.banned', { modes: 'FT8, FT4' }))
  })
})

// ── the FT screen's header ──────────────────────────────────────────────────────────────────
//
// The FT screen is where an FT8 contact is made, and its header drew the same amber sentence while the
// contest ran. It says it with the log line's chip, right after Hound, where the line stood.

describe('the FT screen’s header: a mode the contest does not permit', () => {
  const banned = (mode: string) =>
    t('fieldDay.advisory.banned', { mode, event: contestName('ilqp'), year: 2026 })
  /** The FT screen's header, in the real App. */
  async function ftHeader(): Promise<HTMLElement> {
    await mountOn('operate')
    return waitFor(
      () => {
        const h = document.querySelector<HTMLElement>('.operate-cockpit .cockpit-header')
        expect(h?.querySelector('.cockpit-specialop'), 'control: the FT screen’s header is on show').toBeTruthy()
        return h!
      },
      { timeout: 15_000 },
    )
  }
  /** What the header shows right after Hound. */
  const afterHound = (header: Element) => header.querySelector('.cockpit-specialop')!.nextElementSibling!

  it('on FT8 it says "FT8 · no credit", the sentence on hover; the amber line is gone', async () => {
    operatingMode = 'digital'
    const header = await ftHeader()
    await waitFor(() => expect(afterHound(header).textContent).toBe('FT8 · no credit'))
    expect(afterHound(header).getAttribute('title')).toBe(banned('FT8'))
    expect(within(header).getByRole('status', { name: 'FT8 · no credit', description: banned('FT8') })).toBe(afterHound(header))
    expect(header.querySelector('.fd-advisory')).toBeNull()
    expect(header.textContent).not.toContain(banned('FT8'))
  })

  it('on FT4 the chip names FT4', async () => {
    operatingMode = 'digital'
    tier = 'Ft4'
    const header = await ftHeader()
    await waitFor(() => expect(afterHound(header).textContent).toBe('FT4 · no credit'))
    expect(afterHound(header).getAttribute('title')).toBe(banned('FT4'))
  })

  it('at a contest that credits FT8 there is none', async () => {
    operatingMode = 'digital'
    rules = { ...ILQP_RULES, bannedModes: [] }
    const header = await ftHeader()
    // The ruleset is in, so nothing here is a ruleset not yet read.
    await waitFor(() => expect(bridgeCalls.map((c) => c.cmd)).toContain('get_fd_ruleset'))
    await settle()
    expect(afterHound(header).className).toBe('cockpit-meta')
    expect(document.querySelector('.fd-advisory, .le-fd-nocredit')).toBeNull()
  })
})

// ── transmit ────────────────────────────────────────────────────────────────────────────────

/** Every key a keyboard can send that any cockpit binds, and the rest of the keyboard besides —
 *  ContestLogger.txfree.test.tsx's sweep, less Esc, which is the stop and is pressed on its own. */
function everyKeyButEsc(): KeyboardEventInit[] {
  const out: KeyboardEventInit[] = []
  for (let n = 1; n <= 24; n++) out.push({ key: `F${n}`, code: `F${n}` })
  for (const key of ['Enter', 'Tab', 'Backspace', 'Delete', 'Insert', 'Pause', 'Home', 'End', 'PageUp', 'PageDown', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'ContextMenu']) {
    out.push({ key, code: key })
  }
  const mods: KeyboardEventInit[] = [{}, { ctrlKey: true }, { altKey: true }, { metaKey: true }, { shiftKey: true }]
  for (const m of mods) {
    out.push({ key: ' ', code: 'Space', ...m })
    for (let d = 0; d <= 9; d++) out.push({ key: String(d), code: `Digit${d}`, ...m })
    for (const c of 'abcdefghijklmnopqrstuvwxyz') out.push({ key: c, code: `Key${c.toUpperCase()}`, ...m })
  }
  return out
}

describe('the entry transmits nothing, and Esc is still the stop', () => {
  it('Esc in the Call box while the rig is keyed sends the halt, once, and the contact stays in the boxes', async () => {
    transmitting = true
    const host = await contestScreen()
    const [call] = await typeContact(host)
    call.focus()
    const from = mark()
    fireEvent.keyDown(call, { key: 'Escape', code: 'Escape' })
    fireEvent.keyUp(call, { key: 'Escape', code: 'Escape' })
    await waitFor(() => expect(keyingSince(from)).toEqual(['halt_tx']))
    await settle()
    expect(keyingSince(from), 'one press, one halt').toEqual(['halt_tx'])
    expect(boxesIn(host).map((b) => b.value), 'Esc clears nothing here: it is the stop').toEqual(['K9ABC', '59', 'COOK'])
  })

  it('every other key in every box reaches no command that transmits', async () => {
    const host = await contestScreen()
    await typeContact(host)
    const from = mark()
    for (const box of boxesIn(host)) {
      for (const k of everyKeyButEsc()) {
        fireEvent.keyDown(box, k)
        fireEvent.keyUp(box, k)
      }
      await settle()
    }
    await settle()
    expect(keyingSince(from)).toEqual([])
    // Control: the sweep reached the log line — Enter logged the contact typed before it.
    expect(bridgeCalls.slice(from).map((c) => c.cmd)).toContain('contest_log_manual')
  })

  it('the F-keys send nothing from the boxes', async () => {
    const host = await contestScreen()
    const [call] = await typeContact(host)
    const from = mark()
    for (let n = 1; n <= 12; n++) {
      fireEvent.keyDown(call, { key: `F${n}`, code: `F${n}` })
      fireEvent.keyUp(call, { key: `F${n}`, code: `F${n}` })
    }
    await settle()
    // Nothing but the app's own polls (its reads) reached the bridge: no verb of any kind.
    expect(bridgeCalls.slice(from).map((c) => c.cmd).filter((c) => !c.startsWith('get_'))).toEqual([])
  })

  it('the webview’s own F-keys are held there too, so F5 cannot reload the window mid-contact; Alt, Ctrl and Cmd are the system’s', async () => {
    const host = await contestScreen()
    const [call] = await typeContact(host)
    for (let n = 1; n <= 12; n++) {
      expect(fireEvent.keyDown(call, { key: `F${n}`, code: `F${n}` }), `F${n} was left to the webview`).toBe(false)
    }
    expect(fireEvent.keyDown(call, { key: 'F4', code: 'F4', altKey: true }), 'Alt+F4 still closes the window').toBe(true)
    expect(fireEvent.keyDown(call, { key: 'F5', code: 'F5', ctrlKey: true }), 'a modified F-key is the system’s').toBe(true)
    expect(boxesIn(host).map((b) => b.value)).toEqual(['K9ABC', '59', 'COOK'])
  })

  it('with no contest running the screen’s Esc and F-keys are as they were: no halt, nothing held', async () => {
    fieldDay = null
    await mountOn('fieldDay')
    await waitFor(() => expect(document.querySelector('.contest-screen section.panel.fieldday')).toBeTruthy())
    ;(document.activeElement as HTMLElement | null)?.blur()
    const from = mark()
    expect(fireEvent.keyDown(document.body, { key: 'F5', code: 'F5' }), 'F5 is the webview’s, as before').toBe(true)
    fireEvent.keyDown(document.body, { key: 'Escape', code: 'Escape' })
    await settle()
    expect(keyingSince(from)).toEqual([])
  })

  it('every button of the entry and the last contacts reaches no command that transmits', async () => {
    const host = await contestScreen()
    await typeContact(host)
    const from = mark()
    const seen = new Set<Element>()
    let clicked = 0
    for (let round = 0; round < 4; round++) {
      const fresh = [...host.querySelectorAll<HTMLElement>('.contest-logger-entry button, .contest-logger-recent button')].filter(
        (b) => !seen.has(b),
      )
      if (fresh.length === 0) break
      for (const b of fresh) {
        seen.add(b)
        if (!b.isConnected) continue
        fireEvent.click(b)
        clicked += 1
        await settle()
      }
    }
    expect(clicked, 'control: the sweep clicked the entry').toBeGreaterThan(3)
    expect(keyingSince(from)).toEqual([])
  })

  it('control: the sweep sees a transmit command — the top bar’s Stop TX, and the screen’s Running', async () => {
    const host = await contestScreen()
    const from = mark()
    fireEvent.click(within(document.querySelector('header.topbar') as HTMLElement).getByRole('button', { name: /^stop tx$/i }))
    fireEvent.click(within(host.querySelector('main.layout.single') as HTMLElement).getByRole('button', { name: t('fieldDay.role.running') }))
    await waitFor(() => expect(keyingSince(from)).toEqual(['halt_tx', 'set_mode']))
  })

  it('Enter logs the contact and sends no message, with Enter Sends Message on in every cockpit', async () => {
    settingsAnswer = { ...settingsAnswer, contestEsmPhone: true, contestEsmCw: true, contestEsmRtty: true }
    const host = await contestScreen()
    const boxes = await typeContact(host)
    const from = mark()
    fireEvent.keyDown(boxes[2], { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(bridgeCalls.slice(from).map((c) => c.cmd)).toContain('contest_log_manual'))
    await settle()
    expect(keyingSince(from)).toEqual([])
  })
})

// ── the Logger window keeps one entry ───────────────────────────────────────────────────────

describe('the Logger window is unchanged: one entry, its own', () => {
  afterEach(() => window.history.replaceState(null, '', '/'))

  it('shows the entry once, first, over its copy of the screen, and its Esc stops and still clears', async () => {
    window.history.replaceState(null, '', '/?panel=contestlog')
    render(<DetachedPanel panel="contestlog" />)
    const app = await waitFor(
      () => {
        const a = document.querySelector('.app.detached.contest-logger')
        expect(a?.querySelector('.le-fd-input-call'), 'control: the logger window drew its entry').toBeTruthy()
        return a!
      },
      { timeout: 15_000 },
    )
    const parts = [...app.children].filter((c) => !c.classList.contains('ui-toast-viewport') && !c.querySelector('.ui-toast-viewport'))
    expect(parts.map((c) => `${c.tagName.toLowerCase()}.${[...c.classList].join('.')}`)).toEqual([
      'section.contest-logger-entry',
      'section.contest-logger-recent',
      // The divider that sizes the last contacts against the screen under them.
      'div.pane-splitter.horizontal.contest-seam',
      'main.layout.single',
    ])
    expect(document.querySelectorAll('.contest-logger-entry')).toHaveLength(1)
    expect(document.querySelectorAll('.le-fd-input-call')).toHaveLength(1)
    const boxes = await typeContact(app)
    const from = mark()
    fireEvent.keyDown(boxes[0], { key: 'Escape', code: 'Escape' })
    await settle()
    expect(keyingSince(from), 'the logger window’s Esc is the station’s stop').toEqual(['halt_tx'])
    expect(boxesIn(app)[0].value, 'its Esc clears the entry, as it always did').toBe('')
  })
})

// ── Esc stops CW from every place focus can be ──────────────────────────────────────────────
//
// The Illinois QSO Party (operator, 2026-10-18, on 1.20.0-test4): working CW from the Contest
// screen, Esc did not stop the CW message, unlike every other mode. The main window’s Contest screen
// halted on Esc already; its pop-outs — the entry alone in a window (the Logger window), the screen
// below the entry in a window of its own, the band maps — are documents of their own, so no listener
// in the main window hears a key typed there, and none of them bound one. Esc in each is now the
// station’s halt (`halt_tx`, which cuts the CW keyer, a voice message and every other over), from
// every place focus can be: the Call, RST and county boxes, the county pick list, the screen below,
// the band map, and the window itself.

/** Esc pressed (down and up) on `el`: the transmit-path commands it sent. */
async function escFrom(el: Element): Promise<string[]> {
  const from = mark()
  fireEvent.keyDown(el, { key: 'Escape', code: 'Escape' })
  fireEvent.keyUp(el, { key: 'Escape', code: 'Escape' })
  await settle()
  return keyingSince(from)
}

/** The county box's pick list, opened by typing the start of a county. */
async function countyPicks(root: Element): Promise<HTMLElement> {
  const county = boxesIn(root)[2]
  fireEvent.focus(county)
  fireEvent.change(county, { target: { value: 'CO' } })
  await settle()
  return waitFor(() => {
    const pick = root.querySelector<HTMLElement>('.le-fd-suggest button')
    expect(pick, 'control: the county box opened its pick list').toBeTruthy()
    return pick!
  })
}

/** A pop-out window, past its first snapshot. */
async function popOut(panel: string, drawn: string): Promise<HTMLElement> {
  window.history.replaceState(null, '', `/?panel=${panel}`)
  render(<DetachedPanel panel={panel} />)
  return waitFor(
    () => {
      const a = document.querySelector<HTMLElement>('.app.detached')
      expect(a?.querySelector(drawn), `control: the ${panel} window drew ${drawn}`).toBeTruthy()
      return a!
    },
    { timeout: 15_000 },
  )
}

describe('Esc stops CW sending from everywhere on the Contest screen and its windows', () => {
  beforeEach(() => {
    operatingMode = 'cw'
    transmitting = true
  })
  afterEach(() => window.history.replaceState(null, '', '/'))

  it('the main window: the Call, RST and county boxes, the county pick list, the screen below and the window', async () => {
    const host = await contestScreen()
    const boxes = await typeContact(host)
    for (const [where, el] of [['Call', boxes[0]], ['RST', boxes[1]], ['county', boxes[2]]] as const) {
      transmitting = true
      expect(await escFrom(el), `Esc in the ${where} box`).toEqual(['halt_tx'])
    }
    transmitting = true
    expect(await escFrom(await countyPicks(host)), 'Esc on the county pick list').toEqual(['halt_tx'])
    transmitting = true
    expect(await escFrom(host.querySelector('section.panel.fieldday')!), 'Esc on the screen below').toEqual(['halt_tx'])
    transmitting = true
    expect(await escFrom(document.body), 'Esc on the window').toEqual(['halt_tx'])
  })

  for (const layout of ['full', 'entry'] as const) {
    it(`the entry’s own window (${layout} layout): every box, the county pick list and the window`, async () => {
      localStorage.setItem('nexus.contestLogger.layout', layout)
      const app = await popOut('contestlog', '.le-fd-input-call')
      expect(app.querySelector('main.layout.single') != null, 'control: the layout is the one asked for').toBe(layout === 'full')
      const boxes = await typeContact(app)
      for (const [where, el] of [['Call', boxes[0]], ['RST', boxes[1]], ['county', boxes[2]]] as const) {
        expect(await escFrom(el), `Esc in the ${where} box`).toEqual(['halt_tx'])
      }
      expect(await escFrom(await countyPicks(app)), 'Esc on the county pick list').toEqual(['halt_tx'])
      if (layout === 'full') {
        expect(await escFrom(app.querySelector('section.panel.fieldday')!), 'Esc on the screen below').toEqual(['halt_tx'])
      }
      expect(await escFrom(document.body), 'Esc on the window').toEqual(['halt_tx'])
    })
  }

  it('the screen below in its own window: its header, the screen and the window', async () => {
    const app = await popOut('contestscreen', 'section.panel.fieldday')
    expect(await escFrom(app.querySelector('.fd-header')!), 'Esc on the header').toEqual(['halt_tx'])
    expect(await escFrom(app.querySelector('section.panel.fieldday')!), 'Esc on the screen').toEqual(['halt_tx'])
    expect(await escFrom(document.body), 'Esc on the window').toEqual(['halt_tx'])
  })

  for (const panel of ['bandmapCw', 'bandmapPhone'] as const) {
    it(`the band map in its own window (${panel}): the map and the window`, async () => {
      const app = await popOut(panel, '.bandmap')
      expect(await escFrom(app.querySelector('.bandmap')!), 'Esc on the band map').toEqual(['halt_tx'])
      expect(await escFrom(document.body), 'Esc on the window').toEqual(['halt_tx'])
    })
  }

  for (const panel of ['fieldday', 'fdclub'] as const) {
    it(`the contest’s other window (${panel})`, async () => {
      await popOut(panel, '*')
      expect(await escFrom(document.body), 'Esc on the window').toEqual(['halt_tx'])
    })
  }
})

// ── a satellite held: the logger window's line, not this screen's ───────────────────────────
//
// While the engine holds a bird, the logger window says to log the pass's contacts on the Satellites
// screen (ContestLogger.satellite.test.tsx). The pick was made for the logger window (2026-10-13), so
// this screen, topped with the same log line, stays as it was.

describe('a satellite held: the line is the logger window’s', () => {
  afterEach(() => window.history.replaceState(null, '', '/'))

  it('the Contest screen says nothing about it; the logger window, on the same answers, does', async () => {
    held = { name: 'AO-91', index: 0, description: 'FM transponder', binding: null }
    const host = await contestScreen()
    // Past the next read the logger window would make of the hold (every 2 s).
    await act(async () => new Promise((r) => setTimeout(r, 2_200)))
    const entry = host.querySelector('.contest-logger-entry')!
    expect(entry.querySelector('.le-fd-input-call'), 'control: the entry is on show').not.toBeNull()
    expect(entry.textContent).not.toContain('Satellites screen')
    expect(entry.textContent).not.toContain('AO-91')
    // Control: the same answers in the logger window draw the line.
    cleanup()
    window.history.replaceState(null, '', '/?panel=contestlog')
    render(<DetachedPanel panel="contestlog" />)
    await waitFor(
      () =>
        expect(document.querySelector('.contest-logger-entry')?.textContent).toContain(
          t('contestLogger.satellite.note', { bird: 'AO-91' }),
        ),
      { timeout: 15_000 },
    )
  })
})

// ── one log line ────────────────────────────────────────────────────────────────────────────

describe('a contact logged here is the Phone screen’s contact, byte for byte', () => {
  /** Type K9ABC / 59 / COOK into `boxes` and press Enter in the county box; the bytes that left. */
  async function logFrom(boxes: () => HTMLInputElement[]): Promise<string[]> {
    const b = boxes()
    fireEvent.change(b[0], { target: { value: 'K9ABC' } })
    fireEvent.change(b[2], { target: { value: 'COOK' } })
    await settle()
    const from = mark()
    fireEvent.keyDown(boxes()[2], { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(bridgeCalls.slice(from).map((c) => c.cmd)).toContain('contest_log_manual'))
    return bridgeCalls
      .slice(from)
      .filter((c) => c.cmd === 'contest_log_manual')
      .map((c) => JSON.stringify(c.args))
  }

  it('the same keystrokes send the same contest_log_manual from the Phone LOG pane and from here', async () => {
    await mountOn('phone')
    const phoneStrip = () => {
      const s = document.querySelector('main.phone-cockpit .log-entry-fd')
      expect(s, 'control: the Phone LOG pane is the contest strip').toBeTruthy()
      return boxesIn(s!)
    }
    await waitFor(() => phoneStrip(), { timeout: 15_000 })
    const phone = await logFrom(phoneStrip)
    cleanup()

    await contestScreen()
    const here = await logFrom(() => boxesIn(document.querySelector('.contest-logger-entry')!))

    expect(here).toEqual(phone)
    expect(here).toEqual([JSON.stringify({ call: 'K9ABC', fields: [['RST', '59'], ['QTH', 'COOK']], mode: 'PH', submode: null })])
  })
})

// ── A− / A+ and the text size ───────────────────────────────────────────────────────────────

const read = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css')).filter((r) =>
  r.decls.some((d) => d.prop === 'font-size' || d.prop === 'font' || d.prop.startsWith('--')),
)
const ROOT = rootTokensFrom(RULES, 'dark')
const important = (v: string) => /!\s*important\s*$/.test(v)
const bare = (v: string) => v.replace(/!\s*important\s*$/, '').trim()
const beats = (a: { rule: Rule; value: string }, b: { rule: Rule; value: string }) =>
  important(a.value) !== important(b.value)
    ? important(a.value)
    : cmpSpec(a.rule.spec, b.rule.spec) > 0 || (cmpSpec(a.rule.spec, b.rule.spec) === 0 && a.rule.order > b.rule.order)

/** Evaluate a resolved length to px: calc/min/max/clamp over px, unitless, rem, em and %. */
function evalPx(src: string, em: number): number {
  let i = 0
  const s = src.trim()
  const ws = () => {
    while (i < s.length && /\s/.test(s[i])) i++
  }
  const expr = (): number => {
    let v = term()
    for (ws(); i < s.length && (s[i] === '+' || s[i] === '-'); ws()) {
      const op = s[i++]
      const r = term()
      v = op === '+' ? v + r : v - r
    }
    return v
  }
  const term = (): number => {
    let v = factor()
    for (ws(); i < s.length && (s[i] === '*' || s[i] === '/'); ws()) {
      const op = s[i++]
      const r = factor()
      v = op === '*' ? v * r : v / r
    }
    return v
  }
  const args = (): number[] => {
    const out: number[] = []
    ws()
    if (s[i] !== '(') throw new Error(`expected ( in ${s}`)
    i++
    for (;;) {
      out.push(expr())
      ws()
      if (s[i] === ',') {
        i++
        continue
      }
      if (s[i] === ')') {
        i++
        return out
      }
      throw new Error(`bad argument list in ${s}`)
    }
  }
  const factor = (): number => {
    ws()
    const fn = /^(calc|min|max|clamp)\b/.exec(s.slice(i))
    if (fn) {
      i += fn[1].length
      const a = args()
      if (fn[1] === 'calc') return a[0]
      if (fn[1] === 'min') return Math.min(...a)
      if (fn[1] === 'max') return Math.max(...a)
      return Math.min(Math.max(a[0], a[1]), a[2])
    }
    if (s[i] === '(') {
      i++
      const v = expr()
      ws()
      i++
      return v
    }
    const num = /^(-?\d*\.?\d+)(px|rem|em|%)?/.exec(s.slice(i))
    if (!num) throw new Error(`cannot read "${s.slice(i)}" in ${s}`)
    i += num[0].length
    const n = Number(num[1])
    if (num[2] === 'rem') return n * 16
    if (num[2] === 'em') return n * em
    if (num[2] === '%') return (n * em) / 100
    return n
  }
  const v = expr()
  ws()
  if (i !== s.length) throw new Error(`trailing "${s.slice(i)}" in ${s}`)
  return v
}

/** The font size, in px, the real sheets give `node` at the app's Text size `scale` (Normal 1,
 *  Larger 1.25 — `[data-text-size]` sets `--text-scale` on <html>, whose font size follows it):
 *  CSS's own inheritance walked from <body> down, every custom property an element declares (its
 *  sheet winner, or its inline style) resolved on that element and handed down computed. */
function fontPx(node: Element, scale = 1): number {
  const path: Element[] = []
  for (let n: Element | null = node; n && n !== document.documentElement; n = n.parentElement) path.unshift(n)
  // The root's tokens at this Text size: `--text-scale` first, then every token that reads it.
  const root = new Map(ROOT).set('--text-scale', String(scale))
  let tokens = new Map([...root].map(([k, v]) => [k, expandWith(root, v)]))
  let px = 16 * scale // html { font-size: calc(100% * var(--text-scale)) } over the UA's 16 px
  const chain: El[] = []
  for (const el of path) {
    chain.push(elOf(el))
    const cands = RULES.filter((r) => reachesChain(r.selector, chain, 'dark'))
    const declared = new Map<string, string>()
    const winners = new Map<string, { rule: Rule; value: string }>()
    for (const rule of cands) {
      for (const d of rule.decls) {
        if (!d.prop.startsWith('--')) continue
        const prev = winners.get(d.prop)
        if (!prev || beats({ rule, value: d.value }, prev)) winners.set(d.prop, { rule, value: d.value })
      }
    }
    for (const [k, w] of winners) declared.set(k, bare(w.value))
    const style = (el as HTMLElement).style
    for (let i = 0; i < style.length; i++) {
      if (style[i].startsWith('--')) declared.set(style[i], style.getPropertyValue(style[i]))
    }
    if (declared.size) {
      const own = new Map(tokens)
      for (const [k, v] of declared) own.set(k, v)
      const next = new Map(tokens)
      for (const k of declared.keys()) next.set(k, expandWith(own, own.get(k)!))
      tokens = next
    }
    let win: { rule: Rule; value: string } | null = null
    for (const rule of cands) {
      const d = rule.decls.filter((x) => x.prop === 'font-size' || x.prop === 'font').pop()
      if (d && (!win || beats({ rule, value: d.value }, win))) win = { rule, value: d.value }
    }
    const value = win ? bare(win.value) : null
    if (value && value !== 'inherit' && value !== 'unset') {
      try {
        const n = evalPx(expandWith(tokens, value), px)
        if (Number.isFinite(n) && n > 0) px = n
      } catch {
        // invalid at computed-value time: font-size behaves as `unset`, i.e. inherits
      }
    }
  }
  return px
}

describe('A− / A+ and the app’s Text size', () => {
  const larger = () => document.querySelector<HTMLButtonElement>(`.contest-logger-step[aria-label="${t('contestLogger.size.larger')}"]`)!

  it('is remembered on this computer for the main window, under its own key and never the logger window’s', async () => {
    await contestScreen()
    expect(document.querySelector('.contest-logger-size')!.textContent).toBe(t('contestLogger.size.label', { pct: 100 }))
    fireEvent.click(larger())
    fireEvent.click(larger())
    expect(document.querySelector('.contest-logger-size')!.textContent).toBe(t('contestLogger.size.label', { pct: 120 }))
    expect(localStorage.getItem('nexus.contestLogger.textScale'), 'the main window’s own key').toBe('1.2')
    expect(localStorage.getItem('nexus.contestLogger.textScale.contestlog'), 'the logger window’s is its own').toBeNull()
    cleanup()
    await contestScreen()
    expect(document.querySelector('.contest-logger-size')!.textContent).toBe(t('contestLogger.size.label', { pct: 120 }))
  })

  it('the boxes read large and follow the app’s Text size: 40 px at Normal, 50 px at Larger, and A+ moves both', async () => {
    await contestScreen()
    const call = () => document.querySelector('.contest-logger-entry .le-fd-input-call')!
    const newest = () => document.querySelector('.contest-logger-recent .fd-log-row')!
    expect(fontPx(call())).toBe(40)
    expect(fontPx(call(), 1.25)).toBe(50)
    expect(fontPx(newest(), 1.25)).toBe(25)
    fireEvent.click(larger())
    expect(fontPx(call())).toBeCloseTo(44, 6)
    expect(fontPx(call(), 1.25)).toBeCloseTo(55, 6)
    // The bar's own buttons stay at the app's size, under the finger pressing them.
    expect(fontPx(larger(), 1.25)).toBe(17.5)
  })
})

// ── the no-credit chip's look ───────────────────────────────────────────────────────────────
//
// QUIET, AND READABLE. The band lettered the warning amber on a tint of it; the chip is an outline
// in the de-emphasised state ink with no fill, in the mode chip's face and size, so the row does not
// grow for it. The amber could not have come along: it reads about 4.2:1 on the light panel, under
// the 4.5:1 a word needs, which the FIRES control below shows. Computed from the real sheets on the
// chain this window renders, in every theme and mode cssCascade.ts sweeps.

const LOOK: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css'))
/** FIRES: the chip lettered in the amber the band was, planted here and never shipped. */
const AMBER: Rule[] = [...LOOK, ...parseRules('.le-fd-nocredit { color: var(--state-warn); }', { n: LOOK.length + 1 })]

const memo = new Map<Rule[], Map<string, unknown>>()
function once<T>(rules: Rule[], key: string, make: () => T): T {
  let byKey = memo.get(rules)
  if (!byKey) memo.set(rules, (byKey = new Map()))
  if (!byKey.has(key)) byKey.set(key, make())
  return byKey.get(key) as T
}
const chainKey = (chain: El[]) =>
  JSON.stringify(chain.map((e) => [e.tag, e.classes, Object.entries(e.attrs).filter(([k]) => !/^(id|title|aria-\w+)$/.test(k))]))
/** A colour preset declares custom properties only, so the mode without its presets decides a winner. */
const winnerMode = (mode: Mode): Mode => mode.split(' ').filter((p, i) => i === 0 || p.startsWith('skin=')).join(' ') as Mode
/** The winning declaration of `props` on the chain's last element, or on the nearest ancestor that has one. */
function nearest(rules: Rule[], mode: Mode, chain: El[], ...props: string[]): { value: string; at: El[] } | null {
  for (let i = chain.length; i > 0; i--) {
    const at = chain.slice(0, i)
    const w = once(rules, `w|${winnerMode(mode)}|${chainKey(at)}|${props.join()}`, () => winnerAt(rules, winnerMode(mode), at, ...props))
    if (w && !/^(inherit|transparent|none)$/.test(w.value)) return { value: w.value, at }
  }
  return null
}
const colourOf = (rules: Rule[], mode: Mode, at: El[], value: string, under: Rgb): Rgb => {
  const c = toRgb(expandWith(once(rules, `t|${mode}|${chainKey(at)}`, () => tokensAt(rules, mode, at)), value), under)
  if (!c) throw new Error(`not a colour: "${value}"`)
  return c
}
/** The face the chain's last element sits on: the nearest painted face, laid over the faces beneath
 *  it, down to the page. */
function faceOf(rules: Rule[], mode: Mode, chain: El[]): Rgb {
  const face = nearest(rules, mode, chain, 'background', 'background-color')
  const page = () => colourOf(rules, mode, chain.slice(0, 1), 'var(--bg)', [0, 0, 0])
  if (!face) return page()
  return colourOf(rules, mode, face.at, face.value, face.at.length > 1 ? faceOf(rules, mode, face.at.slice(0, -1)) : page())
}
/** Every theme and mode in which the chain's last element letters under 4.5:1 on its face. */
function unreadable(rules: Rule[], chain: El[]): string[] {
  const out: string[] = []
  for (const mode of MODES) {
    const ink = nearest(rules, mode, chain, 'color')
    if (!ink) {
      out.push(`${mode}: no ink from the sheet`)
      continue
    }
    const bg = faceOf(rules, mode, chain)
    const fg = colourOf(rules, mode, ink.at, ink.value, bg)
    if (contrast(fg, bg) < 4.5) out.push(`${mode}: ${hex(fg)} on ${hex(bg)} = ${contrast(fg, bg).toFixed(2)}:1`)
  }
  return out
}

describe('the no-credit chip’s look, computed from the real sheets', () => {
  /** The chip on FT8, read off the main window's Contest screen. */
  async function chip(): Promise<{ chip: Element; mode: Element }> {
    operatingMode = 'digital'
    const host = await contestScreen()
    const mode = host.querySelector('.contest-logger-entry .le-fd-header > .le-fd-mode')!
    const chip = mode.nextElementSibling!
    expect(chip.textContent, 'control: the chip is on show').toBe('FT8 · no credit')
    return { chip, mode }
  }

  it('an outline in the de-emphasised state ink, no fill, at the mode chip’s size at every Text size', async () => {
    const { chip: c, mode } = await chip()
    const chain = chainOf(c)
    for (const m of BASE_MODES) {
      expect(winnerAt(LOOK, m, chain, 'color')?.value, m).toBe('var(--state-pending)')
      expect(winnerAt(LOOK, m, chain, 'border', 'border-color')?.value, m).toBe('1px solid currentColor')
      expect(winnerAt(LOOK, m, chain, 'background', 'background-color'), m).toBeNull()
    }
    for (const scale of [1, 1.25]) expect(fontPx(c, scale), `Text size ${scale}`).toBe(fontPx(mode, scale))
  })

  it('reads 4.5:1 or better on its face in every theme and mode, where the band’s amber would not', async () => {
    const chain = chainOf((await chip()).chip)
    expect(unreadable(LOOK, chain)).toEqual([])
    const fires = unreadable(AMBER, chain)
    expect(fires.find((f) => f.startsWith('light: ')), 'FIRES: the amber on the light panel').toBeDefined()
  })
})

// The same chip where the amber line stood, in the screen's banner and in the FT screen's header: no
// rule of either host restyles it, and it reads 4.5:1 or better on each host's own face (the banner's
// accent tint, the header's panel), where the amber would not.
describe('the chip in the screen’s banner and the FT screen’s header: its look, computed from the real sheets', () => {
  const hosts: [string, () => Promise<Element>][] = [
    [
      'the Contest screen’s banner',
      async () => {
        operatingMode = 'digital'
        const host = await contestScreen()
        return waitFor(() => {
          const c = host.querySelector('.fd-event-banner > .fd-event-subtitle')!.nextElementSibling!
          expect(c.textContent, 'control: the chip is on show').toBe('FT8 · no credit')
          return c
        })
      },
    ],
    [
      'the FT screen’s header',
      async () => {
        operatingMode = 'digital'
        await mountOn('operate')
        return waitFor(
          () => {
            const c = document.querySelector('.operate-cockpit .cockpit-header .cockpit-specialop')!.nextElementSibling!
            expect(c.textContent, 'control: the chip is on show').toBe('FT8 · no credit')
            return c
          },
          { timeout: 15_000 },
        )
      },
    ],
  ]

  it.each(hosts)('in %s: the log line’s outline, ink and type, with no fill', async (_, find) => {
    const chain = chainOf(await find())
    for (const m of BASE_MODES) {
      expect(winnerAt(LOOK, m, chain, 'color')?.value, m).toBe('var(--state-pending)')
      expect(winnerAt(LOOK, m, chain, 'border', 'border-color')?.value, m).toBe('1px solid currentColor')
      expect(winnerAt(LOOK, m, chain, 'background', 'background-color'), m).toBeNull()
      expect(winnerAt(LOOK, m, chain, 'font-size')?.value, m).toBe('calc(11px * var(--text-scale))')
    }
  })

  it.each(hosts)('in %s: 4.5:1 or better on its face in every theme and mode, where the amber would not', async (_, find) => {
    const chain = chainOf(await find())
    expect(unreadable(LOOK, chain)).toEqual([])
    const fires = unreadable(AMBER, chain)
    expect(fires.find((f) => f.startsWith('light: ')), 'FIRES: the amber on the light face').toBeDefined()
  })
})

describe('the entry pops out on its own', () => {
  it('Pop out entry opens the logger window in its entry-only layout; Logger window opens the whole window', async () => {
    const host = await contestScreen()
    const entry = host.querySelector('.contest-logger-entry') as HTMLElement
    const from = mark()
    fireEvent.click(within(entry).getByRole('button', { name: t('contestScreen.entry.popOut.label') }))
    await settle()
    expect(bridgeCalls.slice(from).filter((c) => c.cmd === 'open_panel_window')).toEqual([{ cmd: 'open_panel_window', args: { panel: 'contestlog' } }])
    expect(localStorage.getItem('nexus.contestLogger.layout')).toBe('entry')
    expect(keyingSince(from)).toEqual([])
    // The screen's own Logger window button asks for the whole window.
    const again = mark()
    fireEvent.click(within(host.querySelector('main.layout.single') as HTMLElement).getByRole('button', { name: t('fieldDay.logger.label') }))
    await settle()
    expect(bridgeCalls.slice(again).filter((c) => c.cmd === 'open_panel_window')).toEqual([{ cmd: 'open_panel_window', args: { panel: 'contestlog' } }])
    expect(localStorage.getItem('nexus.contestLogger.layout')).toBe('full')
  })
})

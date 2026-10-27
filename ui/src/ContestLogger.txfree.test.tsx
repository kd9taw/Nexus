// @vitest-environment jsdom
//
// ⛔ NOTHING IN THE CONTEST LOGGER WINDOW TRANSMITS — proven at the wire, and its keyboard is its own.
//
// The operator's ruling for the logger window: "No PTT, no F-key messages, no Tune. Esc only clears
// the entry, so a logger clearing a typo never cuts your over. Space and the F-keys type or do
// nothing there. All TX and Stop stay on your screen."
//
// EXCEPT ESC, WHICH IS NOW THE STOP HERE TOO (operator, Illinois QSO Party 2026-10-18): working CW with
// this window as the entry, Esc did not stop the message, unlike every other mode. So Esc sends the
// station's halt (`halt_tx`) and still clears the entry; it is the one key that reaches the transmitter,
// and only to stop it. The sweep below presses every OTHER key; Esc has a test of its own.
//
// SO THIS FILE MOCKS AT THE BRIDGE, NOT AT THE MODULE (the stop-control-wiring.test.tsx idiom). The
// REAL `./api` runs under the REAL `?panel=contestlog` window (`DetachedPanel`), and what is replaced
// is `window.__TAURI_INTERNALS__.invoke`, with a recorder. Then EVERY key — the F-keys, Space,
// Enter, Esc, Tab, the arrows, every digit and letter bare and with Ctrl, Alt, Cmd and Shift — is
// pressed at the window and in every box, and EVERY button, switch, checkbox and disclosure in the
// window is clicked, until clicking reveals nothing new. Every command that reached the bridge is
// then held to a reviewed list of commands that cannot key, unkey, arm or hold back a
// transmitter. A command not on the list fails the test until somebody has looked at it.
//
// THE CONTROLS: the same harness pointed at the DOCKED contest screen, whose Running button calls
// CQ in the digital cockpits, reports `set_mode`; and a stand-in for Phone's space-bar PTT, a
// window listener like the real one, reports `set_ptt` to the key sweep. A sweep that cannot see a
// keying command would pass both windows, and these two show it can.
//
// TWO KEYBOARDS. The logger is a separate OS window with a document of its own (`main.tsx` mounts
// `DetachedPanel` for `?panel=`, never `App`), so no main-window handler exists in this document for
// a keystroke here to reach: the sweep's keys find no TX verb because there is no cockpit, no App
// recall hotkey and no stream input here at all. The other way round is `LogEntry.sharedEntry`'s:
// the main window's Esc does not clear the entry, and nothing the logger binds runs there. What
// crosses between the windows is the shared entry's TEXT, through the engine — never a key.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, fireEvent, act, screen } from '@testing-library/react'
import { useEffect } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import { ContestView } from './components/ContestView'
import { setMode, setPtt } from './api'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot } from './types'

// The sweep presses a few thousand keys into a real React tree; measured at about 6 s alone on one
// core, so a loaded full suite gets the house budget.
vi.setConfig({ testTimeout: 120_000 })

// ── the recorder ────────────────────────────────────────────────────────────────────────────
const bridgeCalls: string[] = []

/** The commands the logger window may send, each reviewed: none keys, unkeys, arms, tunes, changes
 *  the operating mode or the dial, or saves the settings (whose save keeps an over already
 *  planned from keying). Reads, the contest log's own writes, the shared entry, files, windows. */
const ALLOWED = new Set([
  // the polls every pop-out runs
  'get_snapshot', 'get_settings', 'get_propagation', 'get_need_alerts', 'get_band_plan', 'get_fd_ruleset',
  // this window's read of the satellite the engine holds, for the line that sends a pass's contacts to
  // the Satellites screen: a read of the hold, which keys, tunes and holds nothing
  'get_sat_transponder',
  // the contest screen: its log, the removed list, the operator box, exports, merge, upload, club
  'contest_removed', 'contest_remove_last', 'contest_restore', 'set_fd_operator', 'export_log',
  // a contact's edit, which writes the contest log and its journal and nothing else
  'contest_edit',
  // Clear test contacts: every contact of the running out of the contest log, each as Remove takes
  // one — the contest log and its journal, nothing else (clearing_never_touches_the_transmitter)
  'contest_clear',
  'save_text_to_downloads', 'fd_merge_to_general', 'fd_set_upload', 'fd_club_export', 'fd_club_give_position',
  // the host's Remove and Allow back on its club board: who is in the club, and nothing else
  'fd_club_remove_position', 'fd_club_allow_back',
  // the journal's contacts the logbook does not hold: the read, and their merge into the logbook
  'contest_unmerged', 'contest_merge_unmerged',
  'open_panel_window',
  // the window's own pin (Keep in front): this window's stacking and its record, nothing at the radio
  'get_window_on_top', 'set_window_on_top',
  // the log line: the contact, its serial binding, its hints, its sent exchange, its log
  'contest_entry_put', 'contest_entry_reset', 'contest_working', 'contest_place', 'contest_i_moved',
  'contest_log_manual', 'contest_log_manual_rows', 'resolve_entity', 'set_cw_peer_info', 'set_log_form_grid',
  'scp_ensure', 'get_scp_calls', 'get_call_history',
  // the county/state box's pre-fill from the whole log: a read of the logbook's rows for one call,
  // which keys, tunes and holds nothing. Asked a moment after the call is typed, so whether it lands
  // inside the sweep's window is timing, and the list must admit it either way.
  'contest_logged_qth',
  // Alt+M in the log line: a band map marker at the dial the station is on, which it reads and never
  // moves (freq_markers.rs); the jump back to one is the band map's, never this window's
  'add_freq_marker',
  // Spot me: one DX cluster spot of the station's call per press, posted over the internet, keying
  // nothing (contest_spot.rs)
  'contest_self_spot',
  // the Stats tab: every call of the log placed by the country file, and its picture saved to
  // Downloads — reads and a file, keying nothing (ContestStats.tsx)
  'contest_stats_places', 'save_png_to_downloads',
])

/** What transmits, or decides whether or what the station transmits: the stop-line census's verbs,
 *  the keyers', the sequencer's and the dial's. None may be on the list above, and none may reach
 *  the bridge from the logger window. */
const KEYING = [
  'set_ptt', 'halt_tx', 'set_tune', 'atu_tune', 'set_tx_enabled', 'stop_cw', 'send_cw', 'send_cw_armed',
  'stop_voice', 'play_voice_message', 'set_mode', 'call_station', 'qso_resend', 'qso_freetext',
  'override_next_tx', 'log_current_qso', 'set_tx_even', 'set_tx_cycle_auto', 'set_tx_offset', 'set_tx_level',
  'set_rf_power', 'rtty_send', 'rtty_stop', 'rtty_set_auto', 'rtty_auto_abort', 'psk_send', 'psk_stop',
  'sstv_stop', 'arm_stream_mic', 'release_stream_mic', 'set_settings', 'set_operating_mode', 'set_frequency',
  'set_contest_esm',
]

const T0 = 1_792_342_800
const fieldDay = {
  running: false,
  state: 'Idle',
  qsoCount: 1,
  sections: 0,
  points: 1,
  multCount: 1,
  event: 'ilqp',
  role: 'in_state',
  log: [
    { call: 'K9AAA', class: '', section: '', band: '20m', mode: 'PH', submode: 'SSB', whenUnix: T0, rcvd: ['59', 'COOK'] },
  ],
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
  upload: { enabled: false, destinations: [], available: ['clublog'], hint: 'hint' },
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
}
const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: true, txAllowed: true },
  link: { tier: 'Ft8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay,
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
} as unknown as AppSnapshot

let rev = 1
function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'contest_entry_put') return ++rev
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (cmd === 'contest_log_manual' || cmd === 'contest_working' || cmd === 'contest_entry_reset') return snapshot
  if (cmd === 'halt_tx') return snapshot // the stop answers with the snapshot, as the engine does
  if (cmd === 'contest_log_manual_rows') return [true]
  if (cmd === 'export_log' || cmd === 'fd_club_export') return 'QSO: …'
  if (cmd === 'save_text_to_downloads') return '/tmp/x'
  if (cmd === 'contest_remove_last' || cmd === 'contest_restore' || cmd === 'contest_edit' || cmd === 'contest_clear') {
    return { outcome: 'refused', refusal: 'changed' }
  }
  if (cmd === 'fd_merge_to_general') return [{ added: 0, already: 1, refused: 0, queued: false }, snapshot]
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  bridgeCalls.length = 0
  rev = 1
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => {
      bridgeCalls.push(cmd)
      return respond(cmd) as T
    },
  }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))

/** Every key a keyboard can send that any cockpit binds, and the rest of the keyboard besides; all
 *  but Esc, the stop, which has a test of its own. */
function everyKey(): KeyboardEventInit[] {
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

/** Press every key at the window and in every box, down and up. */
async function sweepKeys(): Promise<void> {
  const targets: Element[] = [document.body, ...Array.from(document.querySelectorAll('input, textarea'))]
  for (const target of targets) {
    for (const k of everyKey()) {
      fireEvent.keyDown(target, k)
      fireEvent.keyUp(target, k)
    }
    await settle()
  }
}

/** Click everything clickable, again and again while clicking reveals something new. */
async function sweepClicks(): Promise<number> {
  const seen = new Set<string>()
  let clicked = 0
  for (let round = 0; round < 4; round++) {
    const fresh = Array.from(
      document.querySelectorAll<HTMLElement>('button, [role="switch"], input[type="checkbox"], summary'),
    ).filter((el) => {
      const id = `${el.tagName}|${el.className}|${el.textContent}|${el.getAttribute('aria-label') ?? ''}`
      if (seen.has(id)) return false
      seen.add(id)
      return true
    })
    if (fresh.length === 0) break
    // Entry only takes the screen out of the window, and the Stats tab the scoreboard, so they are
    // pressed LAST in their round: everything they hide has been clicked by then, and the rounds after
    // sweep what they show.
    const hides = (el: HTMLElement) =>
      el.textContent === t('contestLogger.layout.label') || el.textContent === t('contestStats.tab.stats')
    fresh.sort((a, b) => Number(hides(a)) - Number(hides(b)))
    for (const el of fresh) {
      if (!el.isConnected) continue
      fireEvent.click(el)
      clicked += 1
      await settle()
    }
  }
  return clicked
}

async function mountLogger(): Promise<void> {
  render(<DetachedPanel panel="contestlog" />)
  // The first snapshot poll lands after 300 ms; the log line is the sign it did.
  await act(async () => new Promise((r) => setTimeout(r, 400)))
  await settle()
  expect(screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), 'control: the log line is on screen').toBeTruthy()
}

function judge(calls: string[]): { refused: string[]; keying: string[] } {
  const sent = [...new Set(calls)].sort()
  return {
    refused: sent.filter((c) => !ALLOWED.has(c)),
    keying: sent.filter((c) => KEYING.includes(c)),
  }
}

describe('the contest logger window transmits nothing', () => {
  it('the reviewed list admits no command that transmits', () => {
    expect(KEYING.filter((c) => ALLOWED.has(c))).toEqual([])
  })

  it('every key, at the window and in every box, reaches no command that transmits', async () => {
    await mountLogger()
    // Type a whole contact first, so Enter has something to log and Esc something to clear.
    const box = screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
    fireEvent.change(box, { target: { value: 'K9ABC' } })
    await settle()
    const from = bridgeCalls.length
    await sweepKeys()
    await settle()
    const { refused, keying } = judge(bridgeCalls.slice(from))
    expect(keying, 'a key in the logger window reached a transmit command').toEqual([])
    expect(refused, 'a key in the logger window reached a command nobody has reviewed').toEqual([])
    // Control: the sweep did reach the log line — typing put the contact, Enter logged it.
    expect(bridgeCalls).toContain('contest_entry_put')
  })

  it('Esc, at the window and in every box, is the stop and only the stop: one halt a press, and the entry clears', async () => {
    await mountLogger()
    const box = screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
    const targets: Element[] = [document.body, ...Array.from(document.querySelectorAll('input, textarea'))]
    expect(targets.length, 'control: the boxes are there to press it in').toBeGreaterThan(2)
    for (const target of targets) {
      fireEvent.change(box, { target: { value: 'K9ABC' } })
      await settle()
      const from = bridgeCalls.length
      fireEvent.keyDown(target, { key: 'Escape', code: 'Escape' })
      fireEvent.keyUp(target, { key: 'Escape', code: 'Escape' })
      await settle()
      const sent = bridgeCalls.slice(from)
      expect(sent.filter((c) => KEYING.includes(c)), 'Esc sent the halt, once, and nothing else that transmits').toEqual(['halt_tx'])
      expect(judge(sent.filter((c) => c !== 'halt_tx')).refused, 'Esc reached a command nobody has reviewed').toEqual([])
    }
    expect(box.value, 'and the entry cleared, as it always did').toBe('')
  })

  it('the F-keys do nothing there, the webview\'s own included', async () => {
    await mountLogger()
    for (let n = 1; n <= 12; n++) {
      expect(fireEvent.keyDown(document.body, { key: `F${n}`, code: `F${n}` }), `F${n} was left to the webview`).toBe(false)
    }
  })

  it('with Alt, Ctrl or Cmd held an F-key is the system\'s, so Alt+F4 closes the window; F5 alone is still held', async () => {
    await mountLogger()
    const call = screen.getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' })
    for (const [where, target] of [['the window', document.body], ['the call box', call]] as const) {
      expect(fireEvent.keyDown(target, { key: 'F5', code: 'F5' }), `${where}: F5 was left to the webview`).toBe(false)
      expect(fireEvent.keyDown(target, { key: 'F4', code: 'F4', altKey: true }), `${where}: Alt+F4 was held`).toBe(true)
      expect(fireEvent.keyDown(target, { key: 'F5', code: 'F5', ctrlKey: true }), `${where}: Ctrl+F5 was held`).toBe(true)
      expect(fireEvent.keyDown(target, { key: 'F5', code: 'F5', metaKey: true }), `${where}: Cmd+F5 was held`).toBe(true)
    }
  })

  it('every button, switch, checkbox and disclosure reaches no command that transmits', async () => {
    await mountLogger()
    const from = bridgeCalls.length
    const clicked = await sweepClicks()
    await settle()
    expect(clicked, 'control: the sweep clicked the screen and the log line').toBeGreaterThan(10)
    const { refused, keying } = judge(bridgeCalls.slice(from))
    expect(keying, 'a click in the logger window reached a transmit command').toEqual([])
    expect(refused, 'a click in the logger window reached a command nobody has reviewed').toEqual([])
    // Control: the clicks reached the wire at all (the screen's own commands went).
    expect(bridgeCalls.slice(from)).toContain('open_panel_window')
  })

  it("control: the docked screen's Running button reaches set_mode, and the sweep sees it", async () => {
    render(
      <main className="layout single">
        <ContestView
          fieldDay={fieldDay as unknown as NonNullable<AppSnapshot['fieldDay']>}
          fdActive
          onSetMode={(m) => void setMode(m)}
        />
      </main>,
    )
    await settle()
    const from = bridgeCalls.length
    await sweepClicks()
    await settle()
    expect(judge(bridgeCalls.slice(from)).keying).toContain('set_mode')
  })

  it("control: a window space-bar PTT like Phone's is reached by the key sweep", async () => {
    function SpacePtt() {
      useEffect(() => {
        const down = (e: KeyboardEvent) => {
          if (e.code === 'Space') void setPtt(true)
        }
        window.addEventListener('keydown', down)
        return () => window.removeEventListener('keydown', down)
      }, [])
      return <span>ptt</span>
    }
    render(<SpacePtt />)
    await settle()
    await sweepKeys()
    expect(judge(bridgeCalls).keying).toContain('set_ptt')
  })

  it('every command on both lists exists in src-tauri, so the lists name real commands', () => {
    const dir = resolve(__dirname, '../../src-tauri/src')
    const src = ['lib.rs', 'contest_lists.rs', 'window_state.rs', 'freq_markers.rs', 'contest_spot.rs'].map((f) => readFileSync(resolve(dir, f), 'utf8')).join('\n')
    const missing = [...ALLOWED, ...KEYING].filter(
      (cmd) => !new RegExp(`\\n(?:pub )?(?:async )?fn ${cmd}\\(`).test(src),
    )
    expect(missing).toEqual([])
  })
})

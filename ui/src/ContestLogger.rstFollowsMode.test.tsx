// @vitest-environment jsdom
//
// THE SIGNAL REPORT FOLLOWS THE MODE IN THE CONTEST LOGGER WINDOW. The window's strip is one strip for
// the whole session: it logs under the station's operating section (`loggerMode`), so a move from
// Phone to CW changes what it logs without mounting it again. A report the operator did not type is
// the mode's own — 59 on phone, 599 on CW and the keyboard modes — and one the operator typed is
// kept for the contact it was typed for.
//
// Found in a rehearsal of the Illinois QSO Party: after contacts on phone, a move to CW left 59 in
// the received report and a CW contact went into the log with it; back on phone it was 599. The
// Sending line said 599 on phone, where a phone contact sends (and logs) 59.
//
// Mocked at the bridge under the real `?panel=contestlog` window, the idiom of
// ContestLogger.txfree.test.tsx: what is asserted is what reached `contest_log_manual`, by value.
// The window polls every 300 ms, so its snapshot is PUBLISHED by the test (`poll`): the strip logs,
// clears and puts first, and the snapshot carrying the contact lands after, at a moment chosen here
// rather than by the interval's phase.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const T0 = 1_792_342_800

/** The station's operating section, as its snapshots say (`radio.operatingMode`). */
let section = 'phone'
/** The Keyboard section's mode on the air, as its snapshots say (`radio.keyboardMode`). */
let keyboardMode: string | undefined
/** The contest log, as the engine holds it. */
let log: Record<string, unknown>[] = []
/** The shared entry while this window is open (`contest_entry.rs`): a put replaces it, a log moves
 *  it on and keeps its boxes. */
let entry = { rev: 1, call: '', fields: {} as Record<string, string>, marks: null as unknown }
/** Every contact the window asked the engine to log: call, received vector, mode class. */
let logged: { call: string; fields: [string, string][]; mode: string }[] = []
/** The sub-mode each of those contacts was logged under (`submode`), in the same order. */
let submodes: (string | null)[] = []
/** What the window's poll reads: the station as it stood at the last `poll()`. */
let published: unknown = null

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it: the session's
 *  report is the 3-digit constant, and the rendered exchange comes in both forms. */
const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 7.04, band: '40m', sideband: 'USB', operatingMode: section, keyboardMode, txEnabled: false, txAllowed: true },
  link: { tier: 'Ft8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    multCount: 0,
    event: 'ilqp',
    role: 'in_state',
    log,
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'MCHE', domain: 'il_counties' },
    ],
    composingText: '599 MCHE',
    composingTextPhone: '59 MCHE',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { ...entry },
})

function respond(cmd: string, args: Record<string, unknown> | undefined): unknown {
  if (cmd === 'get_snapshot') return published
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'contest_entry_put') {
    entry = { rev: entry.rev + 1, call: args!.call as string, fields: args!.fields as Record<string, string>, marks: args!.marks }
    return entry.rev
  }
  if (cmd === 'contest_log_manual') {
    const fields = args!.fields as [string, string][]
    const mode = args!.mode as string
    logged.push({ call: args!.call as string, fields, mode })
    submodes.push((args!.submode as string | null) ?? null)
    log = [
      ...log,
      { call: args!.call, class: '', section: '', band: '40m', mode, submode: mode === 'PH' ? 'SSB' : '', whenUnix: T0 + log.length * 60, rcvd: fields.map(([, v]) => v) },
    ]
    entry = { ...entry, rev: entry.rev + 1, call: '', marks: null }
    return snapshot()
  }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  section = 'phone'
  keyboardMode = undefined
  submodes = []
  log = []
  entry = { rev: 1, call: '', fields: {}, marks: null }
  logged = []
  published = snapshot()
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => respond(cmd, args) as T,
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

/** The station as it stands now reaches the window at its next poll (every 300 ms), and what that
 *  set off. */
const poll = () => {
  published = snapshot()
  return act(async () => new Promise((r) => setTimeout(r, 400)))
}
/** Let the strip finish what a key set off — a log, its clear, its put — before any poll. */
const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const strip = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })
const box = (caption: string) => within(strip()).getByLabelText(caption) as HTMLInputElement
const callBox = () => within(strip()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const sending = () => strip().querySelector('.le-fd-sent-val')?.textContent
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const lastLogged = () => logged[logged.length - 1]

/** Type a contact into the window's strip and press Enter, as the operator does. */
async function logContact(call: string, qth: string, rst?: string) {
  type(callBox(), call)
  if (rst !== undefined) type(box('RST'), rst)
  type(box('County / State'), qth)
  fireEvent.keyDown(callBox(), { key: 'Enter' })
  await settle()
  await settle()
  await poll()
}

describe('the contest logger window: the report follows the mode', () => {
  it('after a contact, a move to CW offers 599 and logs it; back on phone it offers 59', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    // CONTROL: before any contact the box is the section's own, so the probe reads the box.
    expect(box('RST').value, 'phone, nothing logged yet').toBe('59')

    await logContact('K9AAA', 'COOK')
    expect(lastLogged(), 'the phone contact').toEqual({ call: 'K9AAA', fields: [['RST', '59'], ['QTH', 'COOK']], mode: 'PH' })
    expect(box('RST').value, 'the next phone contact').toBe('59')

    section = 'cw'
    await poll()
    expect(box('RST').value, 'moved to CW after a phone contact').toBe('599')
    await logContact('K9EEE', 'MCDN')
    expect(lastLogged(), 'the CW contact').toEqual({ call: 'K9EEE', fields: [['RST', '599'], ['QTH', 'MCDN']], mode: 'CW' })

    section = 'phone'
    await poll()
    expect(box('RST').value, 'back on phone after a CW contact').toBe('59')
    await logContact('K9BBB', 'WILL')
    expect(lastLogged(), 'the phone contact after CW').toEqual({ call: 'K9BBB', fields: [['RST', '59'], ['QTH', 'WILL']], mode: 'PH' })
  })

  it('keeps a report the operator typed for the contact in progress, and the next one follows the mode', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    await logContact('K9AAA', 'COOK')
    // Typed on phone, and the station moves to CW before Enter: the report is what was heard.
    type(callBox(), 'K9CCC')
    type(box('RST'), '57')
    type(box('County / State'), 'LAKE')
    await poll()
    section = 'cw'
    await poll()
    expect(box('RST').value, 'typed, kept across the move').toBe('57')
    fireEvent.keyDown(callBox(), { key: 'Enter' })
    await settle()
    await settle()
    await poll()
    expect(lastLogged(), 'logged as typed, under CW').toEqual({ call: 'K9CCC', fields: [['RST', '57'], ['QTH', 'LAKE']], mode: 'CW' })
    expect(box('RST').value, 'the next contact is untouched: CW’s own').toBe('599')
  })

  it('a section with the same report leaves it as it is: CW, RTTY and keyboard all offer 599', async () => {
    section = 'cw'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    await logContact('K9AAA', 'COOK')
    for (const s of ['rtty', 'keyboard', 'cw']) {
      section = s
      await poll()
      expect(box('RST').value, s).toBe('599')
    }
  })

  it('the Sending line says the report that goes out in this section: 59 on phone, 599 on CW and RTTY', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(sending(), 'phone').toBe('59 MCHE')
    for (const s of ['cw', 'rtty']) {
      section = s
      await poll()
      expect(sending(), s).toBe('599 MCHE')
    }
    section = 'phone'
    await poll()
    expect(sending(), 'back on phone').toBe('59 MCHE')
  })
})

// ⭐ A JTTY CONTACT IS LOGGED AS JTTY. PSK and JTTY are one operating section ('keyboard'), and the
// window logged under the section alone, so with the JTTY screen as the station's section a contact
// went into the log as PSK31 — a stored mode the operator never worked. The snapshot now names the
// Keyboard section's mode (`radio.keyboardMode`), and the window logs under it.
describe('the contest logger window: the keyboard modes log under the mode on the air', () => {
  it('logs a contact as JTTY on JTTY, and as PSK31 / QPSK31 on PSK', async () => {
    section = 'keyboard'
    keyboardMode = 'JTTY'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    await logContact('K9AAA', 'COOK')
    expect([lastLogged().mode, submodes[submodes.length - 1]], 'the JTTY contact').toEqual(['DIG', 'JTTY'])

    keyboardMode = 'QPSK31'
    await poll()
    await logContact('K9BBB', 'WILL')
    expect([lastLogged().mode, submodes[submodes.length - 1]], 'control: QPSK31 on PSK').toEqual(['DIG', 'QPSK31'])

    keyboardMode = 'PSK31'
    await poll()
    await logContact('K9CCC', 'LAKE')
    expect([lastLogged().mode, submodes[submodes.length - 1]], 'control: PSK31 on PSK').toEqual(['DIG', 'PSK31'])
  })
})

// @vitest-environment jsdom
//
// THE OPTIONAL "RST SENT" BOX (N1MM's Snt), in the contest logger window and its entry-only layout.
// Off by default (Settings ▸ Contesting): then there is no box and `contest_log_manual` gets exactly the
// arguments it always did. On, the engine says so by sending the report this contest sends
// (`fieldDay.rstSentBox`), and the box shows what the contact logs as sent: 599 on CW, 59 on phone.
// Space and Tab skip it; Shift-Tab from the received report reaches it. A report typed there goes to
// the engine for that contact only (`sentRst`), and the next contact goes back to the constant.
//
// Mocked at the bridge under the real `?panel=contestlog` window, the idiom of
// ContestLogger.rstFollowsMode.test.tsx: what is asserted is what reached `contest_log_manual`, by value.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { LOGGER_LAYOUT_KEY } from './features/contestLoggerLayout'

const T0 = 1_792_342_800

let section = 'cw'
/** The "RST sent" box as the engine reports it: the session's report, or absent while it is off. */
let rstSentBox: string | undefined = undefined
let log: Record<string, unknown>[] = []
let entry = { rev: 1, call: '', fields: {} as Record<string, string>, marks: null as unknown }
/** Every `contest_log_manual` the window sent, with its whole argument object. */
let logged: Record<string, unknown>[] = []
/** Every `contest_entry_put`'s boxes. */
let puts: Record<string, string>[] = []
let published: unknown = null

const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 7.04, band: '40m', sideband: 'USB', operatingMode: section, txEnabled: false, txAllowed: true },
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
    ...(rstSentBox !== undefined ? { rstSentBox } : {}),
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { ...entry },
})

function respond(cmd: string, args: Record<string, unknown> | undefined): unknown {
  if (cmd === 'get_snapshot') return published
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp', contestRstSentBox: rstSentBox !== undefined }
  if (cmd === 'contest_entry_put') {
    puts.push(args!.fields as Record<string, string>)
    entry = { rev: entry.rev + 1, call: args!.call as string, fields: args!.fields as Record<string, string>, marks: args!.marks }
    return entry.rev
  }
  if (cmd === 'contest_log_manual') {
    logged.push(args!)
    const fields = args!.fields as [string, string][]
    log = [
      ...log,
      { call: args!.call, class: '', section: '', band: '40m', mode: args!.mode, submode: '', whenUnix: T0 + log.length * 60, rcvd: fields.map(([, v]) => v) },
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
  section = 'cw'
  rstSentBox = undefined
  log = []
  entry = { rev: 1, call: '', fields: {}, marks: null }
  logged = []
  puts = []
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

const poll = () => {
  published = snapshot()
  return act(async () => new Promise((r) => setTimeout(r, 400)))
}
const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const strip = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })
const box = (caption: string) => within(strip()).getByLabelText(caption) as HTMLInputElement
const sentBox = () => within(strip()).queryByLabelText(t('logEntry.fd.rstSent.label')) as HTMLInputElement | null
const callBox = () => within(strip()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }) as HTMLInputElement
const logButton = () => strip().querySelector('.le-fd-log-btn') as HTMLButtonElement
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const last = () => logged[logged.length - 1]

async function logContact(call: string, qth: string, sent?: string) {
  type(callBox(), call)
  if (sent !== undefined) type(sentBox()!, sent)
  type(box('County / State'), qth)
  fireEvent.keyDown(callBox(), { key: 'Enter' })
  await settle()
  await settle()
  await poll()
}

describe('the contest entry: the optional RST sent box', () => {
  it('is off by default: no box, and the log command gets no sent report', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    // CONTROL: the strip is drawn, and its received report is there.
    expect(box('RST').value).toBe('599')
    expect(sentBox()).toBeNull()
    await logContact('K9AAA', 'COOK')
    expect(last().call).toBe('K9AAA')
    expect(last().fields).toEqual([['RST', '599'], ['QTH', 'COOK']])
    expect('sentRst' in last(), JSON.stringify(last())).toBe(false)
  })

  it('on, it is pre-filled with the report the contact sends: 599 on CW, 59 on phone', async () => {
    rstSentBox = '599'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(sentBox()!.value).toBe('599')
    section = 'phone'
    await poll()
    expect(sentBox()!.value).toBe('59')
    // Untouched, it sends nothing of its own: the engine stamps the constant.
    await logContact('K9AAA', 'COOK')
    expect('sentRst' in last()).toBe(false)
  })

  it('Space from Call skips it to the received report, Tab passes over it, and Shift-Tab reaches it', async () => {
    rstSentBox = '599'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    callBox().focus()
    fireEvent.keyDown(callBox(), { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(box('RST'))
    expect(sentBox()!.tabIndex).toBe(-1)
    fireEvent.keyDown(box('RST'), { key: 'Tab', shiftKey: true })
    expect(document.activeElement).toBe(sentBox())
    // Space from the sent box walks on to the received report, as every box walks to the next.
    fireEvent.keyDown(sentBox()!, { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(box('RST'))
  })

  it('an edited 579 is logged as sent, shared with the entry, and the next contact goes back to 599', async () => {
    rstSentBox = '599'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    type(callBox(), 'K9NR')
    type(sentBox()!, '579')
    await settle()
    expect(puts.some((f) => f.sentRst === '579'), JSON.stringify(puts)).toBe(true)
    type(box('County / State'), 'KANK')
    fireEvent.keyDown(callBox(), { key: 'Enter' })
    await settle()
    await settle()
    await poll()
    expect(last()).toMatchObject({ call: 'K9NR', fields: [['RST', '599'], ['QTH', 'KANK']], mode: 'CW', sentRst: '579' })
    expect(sentBox()!.value, 'the next contact').toBe('599')
    await logContact('K9ABC', 'COOK')
    expect(last().call).toBe('K9ABC')
    expect('sentRst' in last(), JSON.stringify(last())).toBe(false)
  })

  it('a report without its mode\'s digit count holds Log back and logs nothing', async () => {
    rstSentBox = '599'
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    type(callBox(), 'K9NR')
    type(box('County / State'), 'KANK')
    // CONTROL: the contact as typed can log.
    expect(logButton().disabled).toBe(false)
    type(sentBox()!, '59')
    expect(logButton().disabled).toBe(true)
    expect(sentBox()!.getAttribute('aria-invalid')).toBe('true')
    fireEvent.keyDown(callBox(), { key: 'Enter' })
    await settle()
    expect(logged).toEqual([])
  })

  it('the entry-only window has the box too', async () => {
    rstSentBox = '599'
    localStorage.setItem(LOGGER_LAYOUT_KEY, 'entry')
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    // CONTROL: this is the entry-only layout — the last contacts are not drawn.
    expect(document.querySelector('.contest-logger-recent')).toBeNull()
    expect(sentBox()!.value).toBe('599')
    await logContact('K9NR', 'KANK', '569')
    expect(last()).toMatchObject({ call: 'K9NR', sentRst: '569' })
  })
})

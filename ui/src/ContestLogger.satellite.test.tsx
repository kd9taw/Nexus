// @vitest-environment jsdom
//
// THE LOGGER WINDOW SENDS A PASS'S CONTACTS TO THE SATELLITES SCREEN (the operator's pick, 2026-10-13:
// "Say to use the Satellites screen", while a bird is held). The window's log line logs every contact the
// ordinary way, `contest_log_manual`: on the band the radio is on, with no bird. Only the Satellites
// screen's strip logs a contact on the pass (`contest_log_satellite`), so a pass contact typed here would
// count at ARRL Field Day as an ordinary 2 m or 70 cm contact, and Winter Field Day would credit it. So
// while the engine holds a bird's transponder (`get_sat_transponder`, the read-back the Satellites screen
// and the cockpits' rotor strip poll), one line in the log line, under its bar, names the bird and says to
// log its pass contacts there. Logging here is not blocked.
//
// In the real `?panel=contestlog` window, answered at the bridge as ContestLogger.noCredit.test.tsx is,
// and by value:
//   · AO-91 held: the line names AO-91; the hold moves to SO-50: it names SO-50; released: it is gone;
//   · nothing held (the control): no line, and the log line is on screen;
//   · AO-91 held: a contact typed here still logs, through the ordinary command, as before;
//   · its place, computed from the real sheets: the bar's 1600 px measure and inset, in the app's ink.
// The main window's Contest screen tops itself with the same log line and draws no such line: that is
// App.contestScreenEntry.test.tsx's.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { DetachedPanel } from './DetachedPanel'
import { BASE_MODES, chainOf, parseRules, winnerAt, type Rule } from './cssCascade'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// The hold is read every 2 s, and two of these cases wait out two reads in real time.
vi.setConfig({ testTimeout: 60_000 })

const T0 = 1_792_342_800

/** What `get_sat_transponder` answers: the bird the engine holds a transponder of, or null. */
const AO91 = { name: 'AO-91', index: 0, description: 'FM transponder', binding: null }
const SO50 = { name: 'SO-50', index: 1, description: 'FM repeater', binding: null }
let held: unknown = null
/** Every contact the window asked the engine to log, by the command it went through. */
let logged: { cmd: string; call: string; fields: [string, string][]; mode: string }[] = []
let satAsks = 0

/** An ARRL Field Day session on 2 m phone, as the engine serialises it. */
const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 145.96, band: '2m', sideband: 'FM', operatingMode: 'phone', txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: logged.length,
    sections: 0,
    points: 0,
    event: 'arrlfd',
    log: [],
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
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

function respond(cmd: string, args: Record<string, unknown> | undefined): unknown {
  if (cmd === 'get_snapshot') return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'arrlfd' }
  if (cmd === 'get_sat_transponder') {
    satAsks++
    return held
  }
  if (cmd === 'contest_entry_put') return 2
  if (cmd === 'contest_log_manual' || cmd === 'contest_log_satellite') {
    logged.push({ cmd, call: args!.call as string, fields: args!.fields as [string, string][], mode: args!.mode as string })
    return snapshot()
  }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  held = null
  logged = []
  satAsks = 0
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

const wait = (ms: number) => act(async () => new Promise((r) => setTimeout(r, ms)))
const settle = () => wait(0)
/** The window, past its first snapshot (300 ms) and the first answer about the hold. */
async function mountLogger(): Promise<void> {
  render(<DetachedPanel panel="contestlog" />)
  await wait(400)
  await settle()
}
/** The next read of the hold has landed (every 2 s). */
const nextRead = () => wait(2_200)
const entry = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })
/** What the log line says about a satellite, or null when it says nothing. */
const satLine = (): string | null =>
  within(entry())
    .queryAllByRole('status')
    .map((s) => s.textContent ?? '')
    .find((s) => s.includes('Satellites screen')) ?? null

describe('the logger window while the engine holds a bird', () => {
  it('names the bird and says to log its pass contacts on the Satellites screen; follows the hold; goes with it', async () => {
    held = AO91
    await mountLogger()
    expect(satLine(), 'AO-91 held').toBe(
      'AO-91 is picked on the Satellites screen. Log its pass contacts there: this window logs them as ordinary contacts.',
    )
    expect(satLine()).toBe(t('contestLogger.satellite.note', { bird: 'AO-91' }))
    held = SO50
    await nextRead()
    expect(satLine(), 'the hold moved to SO-50').toBe(t('contestLogger.satellite.note', { bird: 'SO-50' }))
    expect(entry().textContent).not.toContain('AO-91')
    held = null
    await nextRead()
    expect(satLine(), 'released').toBeNull()
    expect(entry().textContent).not.toContain('SO-50')
  })

  it('control: with nothing held there is no line, and the log line is on screen', async () => {
    await mountLogger()
    await nextRead()
    expect(satAsks, 'control: the window asked about the hold').toBeGreaterThan(0)
    expect(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' })).toBeTruthy()
    expect(satLine()).toBeNull()
    expect(document.body.textContent).not.toContain('Satellites screen')
  })

  it('does not stop a contact: typed here with AO-91 held, it logs the ordinary way, as before', async () => {
    held = AO91
    await mountLogger()
    expect(satLine(), 'control: the line is on show').not.toBeNull()
    fireEvent.change(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { target: { value: 'K9ABC' } })
    fireEvent.change(within(entry()).getByLabelText('Class'), { target: { value: '2A' } })
    fireEvent.change(within(entry()).getByLabelText('Section'), { target: { value: 'WI' } })
    fireEvent.keyDown(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { key: 'Enter' })
    await settle()
    await settle()
    expect(logged).toEqual([{ cmd: 'contest_log_manual', call: 'K9ABC', fields: [['CLASS', '2A'], ['SECTION', 'WI']], mode: 'PH' }])
  })
})

// ── its place, computed from the real sheets ─────────────────────────────────────────────────

const read = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const LOOK: Rule[] = parseRules(read('styles.css') + '\n' + read('cockpit-panes.css'))

describe('the line’s place, computed from the real sheets', () => {
  it('on the bar’s 1600 px measure and inset, under it, with no margin of its own, in the app’s ink', async () => {
    held = AO91
    await mountLogger()
    const line = within(entry())
      .queryAllByRole('status')
      .find((s) => (s.textContent ?? '').includes('Satellites screen'))
    expect(line, 'control: the line is on show').toBeDefined()
    const bar = entry().querySelector('.contest-logger-bar')!
    expect(line!.previousElementSibling, 'right under the bar').toBe(bar)
    const at = chainOf(line!)
    const barAt = chainOf(bar)
    for (const m of BASE_MODES) {
      for (const prop of ['max-width', 'margin-inline']) {
        expect(winnerAt(LOOK, m, at, prop)?.value, `${m} ${prop}`).toBe(winnerAt(LOOK, m, barAt, prop)?.value)
      }
      expect(winnerAt(LOOK, m, at, 'max-width')?.value, m).toBe('1600px')
      expect(winnerAt(LOOK, m, at, 'margin-block')?.value, m).toBe('0')
      expect(winnerAt(LOOK, m, at, 'padding')?.value, m).toBe('var(--space-1) var(--space-4) 0')
      expect(winnerAt(LOOK, m, at, 'color')?.value, m).toBe('var(--text)')
    }
  })
})

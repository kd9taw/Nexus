// @vitest-environment jsdom
//
// WHAT THE CONTEST LOGGER WINDOW LOGS A CONTACT UNDER. The window follows the station's operating
// section, which is one of the engine's five: digital, phone, cw, rtty and keyboard
// (`settings::OperatingMode`). The other sections of the main window are none of them: JS8 is the
// digital section on the JS8 tier, picking an SSTV channel claims phone, and APRS and Satellites
// claim none, so the station stays on the section it was in.
//
// On the digital section the line names the mode the station runs, as the JS8 cockpit's own strip
// names JS8. Named nothing, a digital contact is stored with no mode and exports as FT8, which the
// Illinois and New York QSO parties and Winter Field Day give no credit.
//
// In the real `?panel=contestlog` window, mocked at the bridge as ContestLogger.noCredit.test.tsx
// is, and asserted on what reached `contest_log_manual`, by value. The top of the main window's
// Contest screen is the same line, and App.contestScreenEntry.test.tsx pins it there.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

vi.setConfig({ testTimeout: 60_000 })

const T0 = 1_792_342_800

/** The station's operating section and the tier it runs, as its snapshots say. */
let section = 'phone'
let tier = 'FT8'
/** Every contact the window asked the engine to log: the class and the mode named behind it. */
let logged: { call: string; mode: string; submode: string | null }[] = []
let published: unknown = null

/** An Illinois QSO Party session sending from McHenry, as the engine serialises it. */
const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 7.078, band: '40m', sideband: 'USB', operatingMode: section, txEnabled: false, txAllowed: true },
  link: { tier },
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
    multCount: 0,
    event: 'ilqp',
    role: 'in_state',
    log: [],
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
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

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

function respond(cmd: string, args: Record<string, unknown> | undefined): unknown {
  if (cmd === 'get_snapshot') return published
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'get_fd_ruleset') return ILQP_RULES
  if (cmd === 'get_psk_state') return { mode: 'psk31' }
  if (cmd === 'contest_entry_put') return 2
  if (cmd === 'contest_log_manual') {
    logged.push({ call: args!.call as string, mode: args!.mode as string, submode: (args!.submode as string | null) ?? null })
    return snapshot()
  }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  section = 'phone'
  tier = 'FT8'
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

/** The station as it stands now reaches the window at its next poll (every 300 ms). */
const poll = () => {
  published = snapshot()
  return act(async () => new Promise((r) => setTimeout(r, 400)))
}
const settle = () => act(async () => new Promise((r) => setTimeout(r, 0)))
const entry = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })

/** Log K9ABC in Cook County from the window's line, and say what reached the engine. */
async function logOne() {
  fireEvent.change(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { target: { value: 'K9ABC' } })
  fireEvent.change(within(entry()).getByLabelText('County / State'), { target: { value: 'COOK' } })
  fireEvent.keyDown(within(entry()).getByLabelText(/^Call/, { selector: 'input.le-fd-input-call' }), { key: 'Enter' })
  await settle()
  await settle()
  return logged
}

describe('the logger window logs a contact under the mode the station is on', () => {
  it.each([
    ['JS8, the digital section on the JS8 tier', 'JS8', 'JS8'],
    ['FT4', 'FT4', 'FT4'],
    ['FT8', 'FT8', 'FT8'],
    ['Q65', 'Q65', 'Q65'],
    // The engine's own name for Tempo's fast tier, as it logs it: verbatim, not uppercased.
    ['Tempo', 'TempoFast', 'TempoFast'],
  ])('%s: the digital class, and the mode named', async (_name, onTier, named) => {
    section = 'digital'
    tier = onTier
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(entry().querySelector('.le-fd-mode')!.textContent).toBe('DIG')
    expect(await logOne()).toEqual([{ call: 'K9ABC', mode: 'DIG', submode: named }])
  })

  // Controls, as before: off the digital section the tier says nothing about the contact.
  it.each([
    ['phone, with the decoder on JS8', 'phone', 'JS8', { mode: 'PH', submode: null }],
    ['CW, with the decoder on FT8', 'cw', 'FT8', { mode: 'CW', submode: null }],
    ['RTTY, with the decoder on FT4', 'rtty', 'FT4', { mode: 'DIG', submode: 'RTTY' }],
  ])('%s: as the cockpit there logs it', async (_name, onSection, onTier, want) => {
    section = onSection
    tier = onTier
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(await logOne()).toEqual([{ call: 'K9ABC', ...want }])
  })

  it('follows the station from phone to JS8 and back without the window mounting anew', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    section = 'digital'
    tier = 'JS8'
    await poll()
    expect(await logOne()).toEqual([{ call: 'K9ABC', mode: 'DIG', submode: 'JS8' }])
    section = 'phone'
    await poll()
    logged = []
    expect(await logOne()).toEqual([{ call: 'K9ABC', mode: 'PH', submode: null }])
  })
})

// @vitest-environment jsdom
//
// THE LOGGER WINDOW'S PIN: a button on the entry's bar keeps the window in front of Nexus and of other
// programs (the operator, from a contest tester: "resize it to the point where I can drop it in an area
// and keep it to the front"). The shell holds the pin in the window's record, so it is read back at every
// open (`get_window_on_top`) and set through `set_window_on_top`, which answers with what the window now
// is. Mocked at the bridge under the real `?panel=contestlog` window, the idiom of
// ContestLogger.satellite.test.tsx.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const T0 = 1_792_342_800

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    event: 'arrlfd',
    log: [],
    receives: [
      { key: 'CLASS', kind: 'fdClass', required: true },
      { key: 'SECTION', kind: 'oneOf', required: true, domains: ['fd_sections'] },
    ],
    composing: [],
    composingText: '',
    sentExchange: '3A IL',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
}

/** What the shell's record says about this window: whether it offers the pin, and whether it is on. */
let pin = { supported: true, on: false }
/** Every `set_window_on_top` the window sent, by its `on`. */
let sets: boolean[] = []
/** A shell that refuses the change (the record could not be written). */
let refuse = false

function respond(cmd: string, args: Record<string, unknown> | undefined): unknown {
  if (cmd === 'get_snapshot') return snapshot
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'arrlfd' }
  if (cmd === 'get_window_on_top') return pin
  if (cmd === 'set_window_on_top') {
    sets.push(args!.on as boolean)
    if (refuse) throw new Error('could not write the record')
    pin = { supported: true, on: args!.on as boolean }
    return pin
  }
  if (cmd === 'contest_entry_put') return 2
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history|get_sat_transponder)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  pin = { supported: true, on: false }
  sets = []
  refuse = false
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
async function mountLogger(): Promise<void> {
  render(<DetachedPanel panel="contestlog" />)
  await wait(400)
  await wait(0)
}
const entry = () => screen.getByRole('region', { name: t('contestLogger.entry.aria') })
const pinButton = () => within(entry()).queryByRole('button', { name: t('contestLogger.pin.label') })

describe("the logger window's pin", () => {
  it('is on the entry bar, unpressed, and a press pins the window in front and shows it', async () => {
    await mountLogger()
    const button = pinButton()
    expect(button, 'the pin is on the bar').not.toBeNull()
    expect(button!.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(button!)
    await wait(0)
    expect(sets).toEqual([true])
    expect(pinButton()!.getAttribute('aria-pressed')).toBe('true')
    // …and a second press unpins it, back to ordinary stacking.
    fireEvent.click(pinButton()!)
    await wait(0)
    expect(sets).toEqual([true, false])
    expect(pinButton()!.getAttribute('aria-pressed')).toBe('false')
  })

  it('reopens pinned: the window shows the pin its record holds', async () => {
    pin = { supported: true, on: true }
    await mountLogger()
    expect(pinButton()!.getAttribute('aria-pressed')).toBe('true')
  })

  it('a refused change leaves the button as the window really is', async () => {
    refuse = true
    await mountLogger()
    fireEvent.click(pinButton()!)
    await wait(0)
    expect(sets).toEqual([true])
    expect(pinButton()!.getAttribute('aria-pressed')).toBe('false')
  })

  it('is not drawn where the shell offers no pin', async () => {
    pin = { supported: false, on: false }
    await mountLogger()
    expect(pinButton()).toBeNull()
  })
})

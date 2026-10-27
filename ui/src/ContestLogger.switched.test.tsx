// @vitest-environment jsdom
//
// THE CONTEST LOGGER WINDOW SAYS A SAVED PICK SWITCHED THE CONTEST, as the main window's Contest
// screen does. The pick is saved in the main window; this window sees it only in its own
// snapshots, which go from one contest straight to the other (`contestSwitch.ts`). Mocked at the
// bridge under the real `?panel=contestlog` window, the idiom of ContestLogger.txfree.test.tsx.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const T0 = Date.UTC(2026, 9, 18, 18) / 1000

/** The contest the station's session runs, as its snapshots say. */
let event = 'arrlfd'

const snapshot = () => ({
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: true, txAllowed: true },
  link: { tier: 'Ft8' },
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
    multCount: 0,
    event,
    role: '',
    log: [],
    receives: [{ key: 'RST', kind: 'rst', required: true }],
    composing: [{ key: 'RST', raw: '59' }],
    composingText: '59',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: event }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  event = 'arrlfd'
  window.__TAURI_INTERNALS__ = { invoke: async <T,>(cmd: string): Promise<T> => respond(cmd) as T }
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

/** One snapshot poll (the window polls every 300 ms) and what it set off. */
const poll = () => act(async () => new Promise((r) => setTimeout(r, 400)))
const lines = () => [...document.querySelectorAll('.fd-mode-status .fd-mode-line')].map((l) => l.textContent)

const SWITCHED =
  "Now running Illinois QSO Party. Saving the new pick ended ARRL Field Day, and its contacts stay apart from this contest's."

describe('the contest logger window', () => {
  it('names both contests when the session goes from one straight to the other', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await poll()
    expect(lines(), 'opened on ARRL Field Day: nothing switched').not.toContain(SWITCHED)
    expect(lines().length, 'control: the lines are on screen').toBeGreaterThan(0)
    event = 'ilqp'
    await poll()
    expect(lines()).toContain(SWITCHED)
  })
})

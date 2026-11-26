// @vitest-environment jsdom
//
// THE LOGGER WINDOW'S OWN PASS READ (`passHoldsDial`), in the real `?panel=contestlog` window,
// answered at the bridge as ContestLogger.satellite.test.tsx is: the window polls the satellite
// track every 2 s, and while a pass is up its QSY wipe stands down, as the main window's does.
//   · no track (the control): a held QSY sets the line aside;
//   · a pass up: the same move leaves the call in the line.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import { vi } from 'vitest'

vi.setConfig({ testTimeout: 60_000 })

let dial = 14.025
let track: unknown = null

const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: dial, band: '20m', sideband: 'CW', operatingMode: 'cw', txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: true,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    event: 'ilqp',
    role: 'in_state',
    log: [],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties' },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
  },
  contestEntry: { rev: 1, contact: 0, call: '', fields: {}, marks: null },
})

function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'sat_track_status') return track
  if (cmd === 'get_sat_transponder') return null
  if (cmd === 'contest_entry_put') return 2
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.logLine.qsyWipe', 'on')
  dial = 14.025
  track = null
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => respond(cmd) as T,
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

/** Real time in short act() steps, so every snapshot and pass read lands as it comes. */
const wait = async (ms: number) => {
  for (let left = ms; left > 0; left -= 200) await act(async () => new Promise((r) => setTimeout(r, Math.min(200, left))))
}
async function moveWithACall(): Promise<HTMLInputElement> {
  render(<DetachedPanel panel="contestlog" />)
  await wait(2600) // the first snapshot, and the first pass read
  const call = document.querySelector<HTMLInputElement>('input.le-fd-input-call')!
  fireEvent.change(call, { target: { value: 'K9AAA' } })
  await wait(800)
  dial = 14.03
  await wait(4000)
  return call
}

describe('the logger window holds its QSY wipe while a pass is up', () => {
  it('no pass (the control): a held QSY sets the line aside', async () => {
    const call = await moveWithACall()
    expect(call.value).toBe('')
  })

  it('a pass up: the line keeps its call', async () => {
    track = { name: 'SO-50', state: 'tracking', mode: 'rotor-only', dopplerDownlink: false, dopplerUplink: false }
    const call = await moveWithACall()
    expect(call.value).toBe('K9AAA')
  })
})

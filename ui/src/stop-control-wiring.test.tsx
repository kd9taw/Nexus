// @vitest-environment jsdom
//
// THE STOP LINE, AT THE WIRE — does pressing the control actually SEND anything?
//
//   THE OPERATOR MUST NEVER BE UNABLE TO STOP A TRANSMISSION.
//
// `features/panelState.test.ts` checks the NAMES in every pane vocabulary.
// `components/stop-line.test.tsx` checks that every stop control is still ON SCREEN and no
// more disabled than it was, with every ⊞ id hidden.
//
// NEITHER CAN SEE A STOP CONTROL THAT IS PRESENT, ENABLED AND INERT, and that is written into
// both of their headers rather than guarded. It was measured three times before this file
// existed:
//   · `onClick={() => {}}` on CockpitHeader's Stop TX  — the stop of six cockpits — 0 failures
//     across 5253 tests.
//   · the same on CockpitHeader's Tune                 — which KEYS A CARRIER — 0 failures.
//   · `handleHaltTx` in App.tsx emptied                — Operate's Stop TX — 92/92 Operate and
//     stop-line tests still green.
// The cause is structural, not an oversight in any one suite: all 393 cockpit tests do
// `vi.mock('../api')`, which auto-stubs every export, so no test in the tree has ever watched a
// command LEAVE the UI. A census of click→api assertions across ui/src found stopCw 0, setTune 0,
// pskStop 0, stopVoice 0.
//
// SO THIS FILE MOCKS AT THE BRIDGE, NOT AT THE MODULE. `./api` is the REAL module; what is
// replaced is `window.__TAURI_INTERNALS__.invoke` (api.ts's `bridge()`), with a recorder. Every
// assertion below is about the command NAME and the ARGUMENT OBJECT that reached the IPC boundary
// — the same bytes the Rust command would have been handed. A handler that no longer calls its
// api wrapper, an api wrapper pointed at a renamed command, or an argument key that drifted, all
// go red here and only here.
//
// WHY THE REAL App AND NOT THE COCKPIT COMPONENTS. Three of the census's controls are not wired
// inside their cockpit at all — Operate's Stop TX, its Tune and its Esc run through props App
// passes (`handleHaltTx`, `handleSetTune`), which is exactly where the third mutation lived. A
// cockpit-level mount would have proven the prop was called and missed it.
//
// WHAT IS ASSERTED, per control: the EXACT sequence of transmit-path commands the click produced,
// in order, with their arguments. Not "halt_tx was called at some point" — the whole sequence,
// so a stop that fires an extra key-down, or fires its verbs in the wrong order, is also caught.
// Polling traffic (get_snapshot at 300 ms, the per-mode state polls at 2 Hz) is filtered out by
// TX_VERBS; nothing else is.
//
// THE LAST TEST closes the other half of the wire: every command name this file observed must
// exist as a `#[tauri::command] fn <name>` in src-tauri/src/lib.rs, with a parameter for every
// argument key. A UI that serialises `set_tune` at a backend that renamed it is a dead stop
// control that no UI test can see.
//
// WHAT THIS FILE DOES NOT COVER, and why — the census in CLAUDE.md names three kinds that the
// other sweeps call "census-only by construction". Two of them ARE reachable from here and are
// covered below; the third is not, and stays uncovered:
//   · KEYBOARD-ONLY STOPS — COVERED. Phone's Space, CW's Esc, Operate's Esc, RTTY's Esc and
//     PSK's Esc are all `window` listeners, and a real App mount has a real `window`. They were
//     census-only for stop-line.test.tsx because that file finds BUTTONS BY ACCESSIBLE NAME;
//     nothing about them resists a bridge-level check.
//   · CONDITIONALLY RENDERED — COVERED. RTTY's sequencer Abort renders only inside
//     `{auto && seqState !== 'idle'}`; both flags come from the `get_rtty_state` poll, so the
//     bridge fixture puts the cockpit in that state and the button is on screen.
//   · SSTV has NO keyboard stop to cover — the only Escape in SstvView is a React `onKeyDown`
//     on the preview that deselects an overlay item. Stated, not silently dropped.
// Genuinely out of reach: nothing on the census. Two deliberate omissions, both OFF it: APRS
// renders no stop control at all (the rule holds there by construction), and the header's ATU —
// which keys the RIG's own tuning carrier, bounded by the rig — is not a stop control and renders
// only when `radio.atu != null`, which this fixture does not set.
// What this file still does NOT prove is what the BACKEND does with the command — `halt_tx`
// reaching the bridge is not `halt_tx` unkeying a rig. That is `reference-tx-safety-invariants`
// territory and belongs to the Rust suites.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AppSnapshot } from './types'
import App from './App'

// ── the recorder ────────────────────────────────────────────────────────────────────────────
type BridgeCall = { cmd: string; args?: Record<string, unknown> }
const bridgeCalls: BridgeCall[] = []

/** The transmit-path commands. Everything else on the wire is polling, and is filtered out —
 *  this list is what the assertions below are ABOUT, so a new stop verb belongs here the day it
 *  is added or its test will silently assert an empty sequence. */
const TX_VERBS = new Set([
  'halt_tx',
  'stop_cw',
  'set_ptt',
  'set_tune',
  'set_tx_enabled',
  'rtty_stop',
  'rtty_set_auto',
  'rtty_auto_abort',
  'psk_stop',
  'sstv_stop',
  'stop_voice',
  'atu_tune',
])

/** Every transmit-path command since `mark`, rendered as `cmd` or `cmd {"key":value}` — the
 *  argument object is part of the assertion, because the key names ARE the wire contract. */
function firedSince(mark: number): string[] {
  return bridgeCalls
    .slice(mark)
    .filter((c) => TX_VERBS.has(c.cmd))
    .map((c) => (c.args && Object.keys(c.args).length > 0 ? `${c.cmd} ${JSON.stringify(c.args)}` : c.cmd))
}
const mark = (): number => bridgeCalls.length

/** Click and return the transmit-path commands that click produced. */
async function fire(action: () => void): Promise<string[]> {
  const m = mark()
  fireEvent.click(document.body) // flush any pending focus work; never a TX verb
  action()
  await waitFor(() => expect(bridgeCalls.length).toBeGreaterThan(m))
  return firedSince(m)
}

// ── the snapshot the app boots on ───────────────────────────────────────────────────────────
// txAllowed + txEnabled are both TRUE on purpose: Tune is `disabled={!control || !txAllowed}`,
// and Phone's `key(true)` diverts to set_tx_enabled and never reaches set_ptt when TX is off.
// `transmitting` stays FALSE because that is the slot-TX flag alone — CockpitHeader draws the
// TX-enable latch as a *span* while it is true, and in RTTY/SSTV that latch IS a stop control.
// THE TUNE CARRIER IS STATEFUL IN THE FAKE BACKEND, and that is the point of the second half of
// every Tune test: a dead Tune never keys, but a Tune whose OFF-click is dead leaves the rig
// keyed into a dummy load until the tune watchdog expires. The two clicks are different code
// paths — `onTune(!radio.tuning)` — so only a backend that remembers can exercise both.
let tuning = false

const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'Normal',
  radio: {
    dialMhz: 14.078,
    band: '20m',
    catOk: true,
    sideband: 'USB',
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
  link: {
    tier: 'Ft8',
    periodSecs: 15,
    snrDb: -8,
    dtSec: 0.1,
    freqHz: 1500,
    rv: 0,
    state: 'idle',
    quality: 1,
  },
  stations: [],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
} as unknown as AppSnapshot

// RTTY: `sending` enables the dock's Esc/Stop macro (`disabled={!(sending || latched)}`), and
// auto + a non-idle seqState are what put the SEQUENCER ABORT on screen at all.
const rttyState = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  sending: true,
  latched: false,
  backend: 'afsk',
  auto: true,
  seqState: 'cq',
  netHz: 2125,
}
const pskState = { text: '', sending: true, latched: false, netHz: 1000, mode: 'BPSK31', afcHz: 0, armed: true }
// CwCockpit reads `decoded.text.slice(...)` on every render, so the CW decode poll must answer
// the WHOLE DTO — a partial one takes the cockpit down through its ErrorBoundary and the stop
// controls are never mounted at all.
const cwDecodeResult = {
  text: '',
  wpm: 20,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  rst: null as string | null,
  name: null as string | null,
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
}
const sstvState = { sending: true, rxLine: 0, txLine: 0 }
const js8State = {
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
}

/** The fake backend. Only the commands the shell needs a SHAPE from are listed; everything else
 *  answers `{}`, which every remaining caller tolerates. The transmit-path commands answer a
 *  snapshot or their mode state exactly as the real ones do, so a stop's `.then(setX)` lands. */
function respond(cmd: string, args?: Record<string, unknown>): unknown {
  if (cmd === 'set_tune') tuning = args?.on === true
  if (/^(get_snapshot|set_tune|halt_tx|stop_cw|set_ptt|set_tx_enabled|set_operating_mode|set_area|atu_tune|stop_voice)$/.test(cmd)) {
    return { ...snapshot, radio: { ...snapshot.radio, tuning } }
  }
  if (/^(get_rtty_state|rtty_)/.test(cmd)) return rttyState
  if (/^(get_psk_state|psk_)/.test(cmd)) return pskState
  if (/^(get_sstv_state|sstv_)/.test(cmd)) return sstvState
  if (/^(get_js8_state|js8_)/.test(cmd)) return js8State
  if (/^(cw_decode|get_cw_state)$/.test(cmd)) return cwDecodeResult
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log)$/.test(cmd)) return []
  if (/^(get_propagation|get_settings|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update)$/.test(cmd)) return null
  // The log's questions go unanswered, as the whole-log read (answered `{}`) did: no view here
  // needs the log, and `{}` is no answer to any of them.
  if (cmd === 'ask_log') throw new Error('no log in this test')
  return {}
}

beforeEach(() => {
  localStorage.clear()
  bridgeCalls.length = 0
  tuning = false
  // THE WHOLE POINT OF THIS FILE: the real `./api` module, a fake bridge under it.
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      bridgeCalls.push({ cmd, args })
      return respond(cmd, args) as T
    },
  }
  // Every cockpit section on, and a stored feature state so `firstRun` is false (the setup
  // wizard would otherwise cover the screen on a fresh localStorage).
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({
      profile: 'custom',
      enabled: { operate: true, phone: true, cw: true, rtty: true, psk: true, sstv: true, js8: true },
    }),
  )
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  delete window.__TAURI_INTERNALS__
})

/** Mount the real App on `view` and wait for the boot snapshot — before it lands App renders
 *  only "Connecting to Nexus…", and every query below would find nothing. */
async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull(), { timeout: 10_000 })
}

/** A button by accessible name, restricted to what is actually ON SCREEN — every cockpit stays
 *  mounted in a `hidden` keep-alive host, so a bare query can return a control the operator
 *  cannot see (and whose handlers are bound to a different view). */
function onScreenButton(name: RegExp): HTMLButtonElement {
  const hits = screen
    .getAllByRole('button', { name, hidden: true })
    .filter((el) => el.closest('[hidden]') == null) as HTMLButtonElement[]
  expect(hits, `no on-screen button matching ${name}`).toHaveLength(1)
  return hits[0]
}

/** Tune keys the carrier AND drops it again. Both halves matter, and the second one more: a dead
 *  ON-click is a button that does nothing, a dead OFF-click leaves the rig KEYED until the tune
 *  watchdog expires. They are different code paths (`onTune(!radio.tuning)`), so both are driven. */
async function expectTuneKeysAndUnkeys(find: () => HTMLButtonElement): Promise<void> {
  expect(find().disabled, 'Tune is disabled with txAllowed set').toBe(false)
  expect(await fire(() => fireEvent.click(find()))).toEqual(['set_tune {"on":true}'])
  await waitFor(() => expect(find().getAttribute('aria-pressed')).toBe('true'))
  expect(await fire(() => fireEvent.click(find()))).toEqual(['set_tune {"on":false}'])
}

const STOP_TX = /^stop tx$/i
const TUNE = /^tune$|^tuning…$/i
const TX_LATCH = /^▼ tx on$|^■ tx off$/i

// ────────────────────────────────────────────────────────────────────────────────────────────
describe('Phone', () => {
  beforeEach(() => mountOn('phone'))

  it('Stop TX sends halt_tx', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(STOP_TX)))).toEqual(['halt_tx'])
  })

  it('Tune keys the carrier and drops it again', async () => {
    await expectTuneKeysAndUnkeys(() => onScreenButton(TUNE))
  })

  it('PTT keys on press and UNKEYS on release', async () => {
    const ptt = document.querySelector('.ph-ptt') as HTMLButtonElement
    expect(ptt, 'the PTT button is not on screen').not.toBeNull()
    expect(ptt.disabled, 'PTT is disabled with txAllowed set').toBe(false)
    expect(await fire(() => fireEvent.pointerDown(ptt))).toEqual(['set_ptt {"on":true}'])
    expect(await fire(() => fireEvent.pointerUp(ptt))).toEqual(['set_ptt {"on":false}'])
  })

  it('the Space bar keys and unkeys (the keyboard-only stop)', async () => {
    // `e.code`, not `e.key` — PhoneCockpit matches on the physical key.
    expect(await fire(() => fireEvent.keyDown(window, { code: 'Space', key: ' ' }))).toEqual([
      'set_ptt {"on":true}',
    ])
    expect(await fire(() => fireEvent.keyUp(window, { code: 'Space', key: ' ' }))).toEqual([
      'set_ptt {"on":false}',
    ])
  })

  it("the voice keyer's ■ Stop sends stop_voice (a pane-resident convenience, not the line)", async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(/^■ stop$/i)))).toEqual(['stop_voice'])
  })
})

describe('CW', () => {
  beforeEach(() => mountOn('cw'))

  it('Stop TX sends stop_cw THEN halt_tx', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(STOP_TX)))).toEqual(['stop_cw', 'halt_tx'])
  })

  it('Tune keys the carrier and drops it again', async () => {
    await expectTuneKeysAndUnkeys(() => onScreenButton(TUNE))
  })

  it('Esc sends the same stop_cw + halt_tx (the keyboard-only stop)', async () => {
    expect(await fire(() => fireEvent.keyDown(window, { key: 'Escape' }))).toEqual([
      'stop_cw',
      'halt_tx',
    ])
  })
})

describe('Operate', () => {
  beforeEach(() => mountOn('operate'))

  it('Stop TX sends halt_tx — through App, which is where the third mutation lived', async () => {
    const stop = document.querySelector('.cockpit-qso .op-btn.stop') as HTMLButtonElement
    expect(stop, 'Operate Stop TX is not on screen').not.toBeNull()
    expect(stop.disabled).toBe(false)
    expect(await fire(() => fireEvent.click(stop))).toEqual(['halt_tx'])
  })

  it('Tune keys the carrier and drops it again', async () => {
    const tune = document.querySelector('.cockpit-qso .op-btn.tune') as HTMLButtonElement
    expect(tune, 'Operate Tune is not on screen').not.toBeNull()
    await expectTuneKeysAndUnkeys(() => document.querySelector('.cockpit-qso .op-btn.tune') as HTMLButtonElement)
  })

  it('Esc sends halt_tx (the keyboard-only stop)', async () => {
    expect(await fire(() => fireEvent.keyDown(window, { key: 'Escape' }))).toEqual(['halt_tx'])
  })
})

describe('RTTY', () => {
  beforeEach(() => mountOn('rtty'))

  it('Stop TX sends rtty_stop THEN halt_tx', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(STOP_TX)))).toEqual([
      'rtty_stop',
      'halt_tx',
    ])
  })

  it("the dock's Esc/Stop macro sends the same pair", async () => {
    const macro = onScreenButton(/^esc\s*stop$/i)
    expect(macro.disabled, 'the Esc/Stop macro is disabled while sending').toBe(false)
    expect(await fire(() => fireEvent.click(macro))).toEqual(['rtty_stop', 'halt_tx'])
  })

  it('Tune keys the carrier and drops it again', async () => {
    await expectTuneKeysAndUnkeys(() => onScreenButton(TUNE))
  })

  it('Esc sends rtty_stop + halt_tx (the keyboard-only stop)', async () => {
    expect(await fire(() => fireEvent.keyDown(window, { key: 'Escape' }))).toEqual([
      'rtty_stop',
      'halt_tx',
    ])
  })

  it('the TX-enable latch DISARMS with set_tx_enabled {enabled:false} — here the latch IS a stop', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(TX_LATCH)))).toEqual([
      'set_tx_enabled {"enabled":false}',
    ])
  })

  it("the sequencer's Abort sends rtty_auto_abort (the conditionally-rendered stop)", async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(/^esc\s*abort$/i)))).toEqual([
      'rtty_auto_abort',
    ])
  })

  it('the stream pane\'s "Auto on" toggle sends rtty_set_auto {on:false} (pane-resident)', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(/^auto on$/i)))).toEqual([
      'rtty_set_auto {"on":false}',
    ])
  })
})

describe('PSK', () => {
  beforeEach(() => mountOn('psk'))

  it('Stop TX sends psk_stop THEN halt_tx', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(STOP_TX)))).toEqual([
      'psk_stop',
      'halt_tx',
    ])
  })

  it("the dock's Esc/Stop macro sends the same pair", async () => {
    const macro = onScreenButton(/^esc\s*stop$/i)
    expect(macro.disabled).toBe(false)
    expect(await fire(() => fireEvent.click(macro))).toEqual(['psk_stop', 'halt_tx'])
  })

  it('Tune keys the carrier and drops it again', async () => {
    await expectTuneKeysAndUnkeys(() => onScreenButton(TUNE))
  })

  it('Esc sends psk_stop + halt_tx (the keyboard-only stop)', async () => {
    expect(await fire(() => fireEvent.keyDown(window, { key: 'Escape' }))).toEqual([
      'psk_stop',
      'halt_tx',
    ])
  })

  it('the TX-enable latch sends set_tx_enabled {enabled:false}', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(TX_LATCH)))).toEqual([
      'set_tx_enabled {"enabled":false}',
    ])
  })
})

describe('SSTV', () => {
  beforeEach(() => mountOn('sstv'))

  it('the TX bar\'s Stop sends sstv_stop', async () => {
    const stop = document.querySelector('.sstv-tx-bar .sstv-tx-stop') as HTMLButtonElement
    expect(stop, 'the SSTV Stop button is not on screen').not.toBeNull()
    expect(stop.disabled, 'SSTV Stop is disabled while sending').toBe(false)
    expect(await fire(() => fireEvent.click(stop))).toEqual(['sstv_stop'])
  })

  it('the TX-enable latch DISARMS with set_tx_enabled {enabled:false} — here the latch IS a stop', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(TX_LATCH)))).toEqual([
      'set_tx_enabled {"enabled":false}',
    ])
  })

  it('the header Stop TX sends halt_tx and Tune sends set_tune', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(STOP_TX)))).toEqual(['halt_tx'])
    await expectTuneKeysAndUnkeys(() => onScreenButton(TUNE))
  })
})

describe('JS8', () => {
  beforeEach(() => mountOn('js8'))

  it('Stop TX sends halt_tx', async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(STOP_TX)))).toEqual(['halt_tx'])
  })

  it('Tune keys the carrier and drops it again', async () => {
    await expectTuneKeysAndUnkeys(() => onScreenButton(TUNE))
  })

  it('Esc sends halt_tx (the keyboard-only stop)', async () => {
    expect(await fire(() => fireEvent.keyDown(window, { key: 'Escape' }))).toEqual(['halt_tx'])
  })
})

// ── the other half of the wire ──────────────────────────────────────────────────────────────
//
// Everything above proves the UI serialises a command. It cannot prove the BACKEND still answers
// to that name: `set_tune` renamed in lib.rs leaves every assertion above green and every Tune
// button in the app dead. So the command names and argument keys this file observed are checked
// against the Rust command surface, which is where they are consumed.
//
// Source-level on purpose — the point is to catch a rename at the commit that makes it, without
// a running backend.
describe('every command this file sends exists in the Rust command surface', () => {
  const lib = readFileSync(resolve(__dirname, '../../src-tauri/src/lib.rs'), 'utf8')

  // The full set, spelled out rather than harvested from bridgeCalls: a harvested list shrinks
  // silently when a test above stops firing, and an empty list passes.
  const WIRE: Record<string, string[]> = {
    halt_tx: [],
    stop_cw: [],
    set_ptt: ['on'],
    set_tune: ['on'],
    set_tx_enabled: ['enabled'],
    rtty_stop: [],
    rtty_set_auto: ['on'],
    rtty_auto_abort: [],
    psk_stop: [],
    sstv_stop: [],
    stop_voice: [],
  }

  for (const [cmd, args] of Object.entries(WIRE)) {
    it(`${cmd}(${args.join(', ')})`, () => {
      const m = new RegExp(`\\n(?:pub )?(?:async )?fn ${cmd}\\(([^)]*)\\)`, 's').exec(lib)
      expect(m, `no \`fn ${cmd}(\` in src-tauri/src/lib.rs`).not.toBeNull()
      const params = m![1]
      for (const a of args) {
        expect(new RegExp(`\\b${a}\\s*:`).test(params), `${cmd} has no \`${a}:\` parameter`).toBe(true)
      }
    })
  }
})

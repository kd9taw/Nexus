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
//   · KEYBOARD-ONLY STOPS — COVERED. Phone's Space and every screen's Esc are `window` listeners,
//     and a real App mount has a real `window`: Operate's, CW's, RTTY's, PSK's and JS8's are the
//     cockpits' own; Tempo's, Phone's, SSTV's, APRS's and Satellites' are App's (N71). They are
//     census-only for stop-line.test.tsx because that file finds BUTTONS BY ACCESSIBLE NAME;
//     nothing about them resists a bridge-level check, and the Esc census near the end of this
//     file presses Esc on every section in the registry.
//   · CONDITIONALLY RENDERED — COVERED. RTTY's sequencer Abort renders only inside
//     `{auto && seqState !== 'idle'}`; both flags come from the `get_rtty_state` poll, so the
//     bridge fixture puts the cockpit in that state and the button is on screen.
//   · SSTV's keyboard stop is App's Esc (N71); the only Escape in SstvView itself is a React
//     `onKeyDown` on the preview that deselects an overlay item, and it still does.
// Genuinely out of reach: nothing on the census. Two deliberate omissions, both OFF it: APRS
// renders no stop BUTTON (its stops are the TX latch and, from N71, Esc), and the header's ATU —
// which keys the RIG's own tuning carrier, bounded by the rig — is not a stop control and renders
// only when `radio.atu != null`, which this fixture does not set.
// What this file still does NOT prove is what the BACKEND does with the command — `halt_tx`
// reaching the bridge is not `halt_tx` unkeying a rig. That is `reference-tx-safety-invariants`
// territory and belongs to the Rust suites.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, cleanup, waitFor, fireEvent, screen, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { EN } from './i18n'
import type { AppSnapshot } from './types'
import App from './App'
import { allFeatureIds, featureById, sectionFeatures, type View } from './features/registry'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

// A 30 s budget for every test and hook here, for the machine and not for the checks. Each test mounts the real App, and
// in three full-suite runs on a loaded box (2026-09-29 and 30) vitest's default budgets ran out with nothing wrong: "Test
// timed out in 5000ms" on RTTY's latch, RTTY's Auto toggle and JS8's Tune, and "Hook timed out in 10000ms" on the mount
// before RTTY's Tune and Esc and JS8's Esc. No assertion, fixture or wait below is changed: a stop that stops reaching
// the wire still fails at its own assertion, well inside the budget.
vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 })

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
/** What `get_settings` answers. Null (no settings file) everywhere but the one screen that needs a
 *  switch in it: Field Day is drawn only with its master switch (`fdActive`) on. */
let settingsAnswer: unknown = null

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
  // APRS reads its roster, what it has heard, its decoder's health and the internet feed's status on
  // mount, in these shapes: a `{}` roster takes the view down through its ErrorBoundary.
  if (cmd === 'get_aprs_stations') return { stations: [], ttlMin: 60, fadeAfterMin: 30 }
  if (cmd === 'get_aprs_heard') return []
  if (cmd === 'get_aprs_health') {
    return { arm: 'auto', audioPeak: 0, lastAudioUnix: null, drains: 0, framesSeen: 0, framesDecoded: 0, lastDecodeUnix: null,
      lastFrameSeenUnix: null, framePeak: 0, maxFramePeak: 0, frameClippedSamples: 0, radioName: '', bandRadioCount: 1 }
  }
  if (cmd === 'get_aprs_is_status') {
    return { enabled: false, connected: false, verified: false, packets: 0, lastPacketUnix: null, uplinkEnabled: false, uploaded: 0,
      gateRejected: 0, lastReject: null }
  }
  if (cmd === 'aprs_auto_arm') return true
  if (cmd === 'get_aprs_tx_notice') return null
  if (cmd === 'get_awards') return { achievements: [] }
  if (cmd === 'get_journey') return { firsts: [], feats: [], ladders: [] }
  if (cmd === 'app_version') return '0.0.0-test'
  if (cmd === 'radio_launch_info') return { showPicker: false }
  if (/^(get_band_plan|get_licensed_band_plan|log_operators|log_activations|get_all_spots|get_need_alerts|get_dxped_windows|get_sat_schedule|get_voice_messages|get_log)$/.test(cmd)) return []
  if (cmd === 'get_settings') return settingsAnswer
  if (/^(get_propagation|get_fd_ruleset|get_feed_health|get_xray_now|sat_track_status|get_iss_pass|get_tle_status|get_kp_forecast|check_for_update)$/.test(cmd)) return null
  // Connect's own feeds, in the shapes its panes read (ConnectView.panes.test.tsx answers the same
  // ones): a `{}` takes the view down through its ErrorBoundary, and Connect is never drawn at all.
  if (cmd === 'get_band_outlook') return { bands: [], asOf: 0 }
  if (cmd === 'get_space_wx_scales') return [null, []]
  if (/^(get_kc2g_muf|get_ota_map_spots|get_contests)$/.test(cmd)) return []
  if (/^(get_getting_out|get_path_outlook|get_aurora|get_declination|get_pca|get_satellites|get_log_stats)$/.test(cmd)) return null
  // The log's questions go unanswered, as the whole-log read (answered `{}`) did: no view here
  // needs the log, and `{}` is no answer to any of them.
  if (cmd === 'ask_log') throw new Error('no log in this test')
  return {}
}

beforeEach(() => {
  localStorage.clear()
  bridgeCalls.length = 0
  tuning = false
  settingsAnswer = null
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
async function mountOn(view: string, area: 'dx' | 'msg' = 'dx'): Promise<void> {
  localStorage.setItem('nexus.workspace', area)
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
// The TX strip's latch, in FT's words since 2026-10-01 (the header's read "▼ TX On" / "■ TX Off").
const TX_LATCH = /^tx on$|^tx off$/i

// ── Space and the ⊞ menu (Phone) ────────────────────────────────────────────────────────────
const SPACE = { code: 'Space', key: ' ' }

/** Phone's ⊞ Panels, opened; returns the popover on screen. */
async function openPanels(): Promise<HTMLElement> {
  fireEvent.click(onScreenButton(/^⊞ panels/i))
  return waitFor(() => {
    const pop = [...document.querySelectorAll<HTMLElement>('.panels-menu-pop')].find((p) => p.closest('[hidden]') == null)
    expect(pop, 'the ⊞ popover did not open').toBeDefined()
    return pop!
  })
}

/** The first ⊞ Arrange move that can act. A disabled button takes no focus and no key, so pressing
 *  one would prove nothing. */
function liveArrangeButton(pop: HTMLElement): HTMLButtonElement {
  const b = [...pop.querySelectorAll<HTMLButtonElement>('.panels-arrange button')].find((x) => !x.disabled)
  expect(b, 'no live ⊞ Arrange button: these tests would be pressing nothing').toBeDefined()
  return b!
}

/** What an action put on the transmit path, when the right answer may be NOTHING. `fire` waits
 *  for a command and would time out on a silent action. The key handlers call the api
 *  synchronously, so a short settle is enough; the mutation controls in the L3 report show this
 *  sees a send when there is one. */
async function sentBy(action: () => unknown): Promise<string[]> {
  const m = mark()
  await action()
  await new Promise((r) => setTimeout(r, 30))
  return firedSince(m)
}

/** Space pressed and released on a focused control, the way a browser delivers it: keydown, a
 *  moment held, keyup. jsdom performs no default actions, so the click a browser gives a focused
 *  button for an UNCANCELLED Space is dispatched here. Returns whether the control was pressed
 *  (Chrome's own activation is measured in the L3 report). */
async function spaceOn(el: HTMLElement): Promise<boolean> {
  el.focus()
  const down = fireEvent.keyDown(el, SPACE)
  await new Promise((r) => setTimeout(r, 10))
  const up = fireEvent.keyUp(el, SPACE)
  if (down && up) fireEvent.click(el)
  return down && up
}

/** Phone's stored arrangement: what an Arrange move, Undo and Reset each change. */
const phonePlace = (): unknown => JSON.parse(localStorage.getItem('nexus.panels.phone.main') ?? 'null')?.place

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

  // SPACE INSIDE THE ⊞ MENU (layout L3, ruling R7). Space is this cockpit's talk key on every
  // target but a field. Inside the ⊞ Panels popover it presses the focused control instead, as
  // Space does everywhere else in a browser: ⊞ Arrange is a grid of buttons, with Undo and Reset
  // beside it. ONLY THE PRESS is exempt. The release still asks one question, "are we keyed?", so
  // an over keyed from outside the menu unkeys wherever focus has gone by the release
  // (reference-tx-safety-invariants: an unkey a guard can swallow is a stuck transmitter).
  describe('Space inside the ⊞ menu presses the menu, never the transmitter', () => {
    it('an Arrange move, Undo and Reset each act, and nothing reaches the transmit path', async () => {
      const pop = await openPanels()
      let pressed = false
      expect(await sentBy(async () => (pressed = await spaceOn(liveArrangeButton(pop)))), 'Space on an Arrange move').toEqual([])
      expect(pressed, 'Space on an Arrange move was cancelled').toBe(true)
      expect(phonePlace(), 'the Arrange move did not happen').toBeDefined()
      const undo = within(pop).getByRole('button', { name: 'Undo last change' })
      expect(await sentBy(async () => (pressed = await spaceOn(undo))), 'Space on Undo').toEqual([])
      expect(pressed, 'Space on Undo was cancelled').toBe(true)
      expect(phonePlace(), 'Undo did not take the move back').toBeUndefined()
      await spaceOn(liveArrangeButton(pop))
      expect(phonePlace()).toBeDefined()
      const reset = within(pop).getByRole('button', { name: 'Reset layout' })
      expect(await sentBy(async () => (pressed = await spaceOn(reset))), 'Space on Reset').toEqual([])
      expect(pressed, 'Space on Reset was cancelled').toBe(true)
      expect(phonePlace(), 'Reset did not restore the stock arrangement').toBeUndefined()
    })

    it('CONTROL: on a button outside the popover (the ⊞ trigger, Tune) Space keys exactly as before', async () => {
      await openPanels()
      for (const outside of [onScreenButton(/^⊞ panels/i), onScreenButton(TUNE)]) {
        let pressed = true
        const sent = await sentBy(async () => (pressed = await spaceOn(outside)))
        expect(sent, `Space on ${outside.textContent}`).toEqual(['set_ptt {"on":true}', 'set_ptt {"on":false}'])
        expect(pressed, `Space pressed ${outside.textContent} as a button`).toBe(false)
      }
    })

    it('held from outside and released inside the popover, it UNKEYS: onto a button or a ⊞ checkbox', async () => {
      const pop = await openPanels()
      const checkbox = pop.querySelector<HTMLInputElement>('input[type="checkbox"]')
      expect(checkbox, 'no ⊞ entry checkbox in the popover').not.toBeNull()
      for (const inside of [liveArrangeButton(pop), checkbox!]) {
        ;(document.activeElement as HTMLElement | null)?.blur()
        expect(await fire(() => fireEvent.keyDown(document.body, SPACE))).toEqual(['set_ptt {"on":true}'])
        inside.focus()
        expect(await fire(() => fireEvent.keyUp(inside, SPACE)), `released on ${inside.tagName}`).toEqual(['set_ptt {"on":false}'])
      }
    })

    it('auto-repeat inside the popover during an over neither re-keys nor unkeys it', async () => {
      const pop = await openPanels()
      const inside = liveArrangeButton(pop)
      expect(await fire(() => fireEvent.keyDown(document.body, SPACE))).toEqual(['set_ptt {"on":true}'])
      inside.focus()
      const repeats = () => {
        for (let i = 0; i < 5; i++) fireEvent.keyDown(inside, { ...SPACE, repeat: true })
      }
      expect(await sentBy(repeats), 'the held key repeating inside the popover').toEqual([])
      expect(await fire(() => fireEvent.keyUp(inside, SPACE))).toEqual(['set_ptt {"on":false}'])
    })
  })

  it("the voice keyer's ■ Stop sends stop_voice (a pane-resident convenience, not the line)", async () => {
    expect(await fire(() => fireEvent.click(onScreenButton(/^■ stop$/i)))).toEqual(['stop_voice'])
  })
})

// With TX switched off, Phone's talk key turns TX back on instead of keying (`key(true)`'s second
// refusal). Inside the ⊞ menu it must not do that either.
describe('Phone, TX switched off', () => {
  beforeEach(async () => {
    snapshot.radio.txEnabled = false
    await mountOn('phone')
  })
  afterEach(() => {
    snapshot.radio.txEnabled = true
  })

  it('Space inside the ⊞ menu does not turn TX back on; on the ⊞ trigger it does, as before', async () => {
    const pop = await openPanels()
    expect(await sentBy(() => spaceOn(liveArrangeButton(pop))), 'Space on an Arrange move').toEqual([])
    expect(await sentBy(() => spaceOn(onScreenButton(/^⊞ panels/i))), 'Space on the ⊞ trigger').toEqual([
      'set_tx_enabled {"enabled":true}',
    ])
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

// ── Esc stops transmit on every operating screen and on Satellites ─────────────────────────
//
// Measured with real keys in a real browser (N66, 2026-09-28): Esc stopped transmit on FT, CW,
// RTTY, PSK and JS8, each of which binds its own Esc while it is on show. It did NOTHING on Tempo,
// SSTV and APRS; on Phone it stopped only the voice keyer; and on Satellites, where App hides the
// top bar's transmit controls, there was no stop of any kind, by key or by button. On those five
// the stop is halt_tx: Tempo's Stop TX is the top bar's (App's handleHaltTx), Phone's and SSTV's
// header Stop TX call haltTx alone, and APRS and Satellites draw none. So App binds Esc to that halt
// while one of the five is on show, and these tests hold it at the wire:
//   · Esc sends exactly what the screen's Stop TX sends (Phone: then the voice keyer's own stop);
//   · from inside a text field as well: Esc is an abort key, not an editing key, as it is on FT;
//   · a control that stops the key on its way cannot swallow the stop (App listens in the capture
//     phase);
//   · a menu or panel that Esc closes still closes on the same press and the halt is sent too,
//     which is what FT and CW do, measured here alongside;
//   · and every other screen's Esc is exactly what it was: the census is every section in the
//     registry, so the halt cannot leak onto a screen with its own Esc (that would send a second
//     halt) or onto one that has none.

/** Every section on, so every screen can be the one on show. */
function everySectionOn(): void {
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: Object.fromEntries(allFeatureIds().map((id) => [id, true])) }),
  )
}

/** The on-screen element matching `sel` — every keep-alive host stays mounted while hidden. */
function shown(sel: string): HTMLElement | null {
  return [...document.querySelectorAll<HTMLElement>(sel)].find((el) => el.closest('[hidden]') == null) ?? null
}

/** Esc as a keyboard delivers it: to the focused element, or the body when nothing has focus. */
function pressEsc(on: Element = document.activeElement ?? document.body): void {
  fireEvent.keyDown(on, { key: 'Escape', code: 'Escape' })
  fireEvent.keyUp(on, { key: 'Escape', code: 'Escape' })
}

type EscScreen = {
  view: View
  area: 'dx' | 'msg'
  /** The screen's own content, so a test is never pressing Esc on an error panel. */
  drawn: () => HTMLElement | null
  /** What Esc must send, in order. */
  esc: string[]
}
const ESC_SCREENS: EscScreen[] = [
  // Tempo lives in the Tempo area; in the FT area App sends #chat to Operate.
  { view: 'chat', area: 'msg', drawn: () => shown('.cockpit-modes'), esc: ['halt_tx'] },
  // The halt first, then the voice keyer's own stop (its listener, unchanged, ignores Esc in a field).
  { view: 'phone', area: 'dx', drawn: () => shown('.ph-ptt'), esc: ['halt_tx', 'stop_voice'] },
  { view: 'sstv', area: 'dx', drawn: () => shown('.sstv-tx-bar'), esc: ['halt_tx'] },
  { view: 'aprs', area: 'dx', drawn: () => shown('main.aprs-cockpit'), esc: ['halt_tx'] },
  { view: 'sats', area: 'dx', drawn: () => shown('.sats-view'), esc: ['halt_tx'] },
]

async function mountScreen(s: EscScreen): Promise<void> {
  everySectionOn()
  await mountOn(s.view, s.area)
  expect(document.title, `control: ${s.view} is the screen on show`).toBe(`${featureById(s.view)!.label} — Nexus`)
  await waitFor(() => expect(s.drawn(), `control: ${s.view} drew its own content`).not.toBeNull())
  ;(document.activeElement as HTMLElement | null)?.blur()
}

describe('Esc on Tempo, Phone, SSTV, APRS and Satellites sends the same halt as Stop TX', () => {
  it.each(ESC_SCREENS.map((s) => [s.view, s] as const))('%s: Esc sends the halt', async (_view, s) => {
    await mountScreen(s)
    expect(await fire(() => pressEsc())).toEqual(s.esc)
  })

  it.each(ESC_SCREENS.filter((s) => s.view !== 'aprs' && s.view !== 'sats').map((s) => [s.view, s] as const))(
    "%s: Esc sends what the screen's own Stop TX sends",
    async (_view, s) => {
      await mountScreen(s)
      const stopTx = await fire(() => fireEvent.click(onScreenButton(STOP_TX)))
      expect(stopTx, 'control: Stop TX is the halt').toEqual(['halt_tx'])
      const esc = await fire(() => pressEsc())
      expect(esc.slice(0, stopTx.length), 'Esc does not begin with what Stop TX sends').toEqual(stopTx)
    },
  )

  // A text field where the screen has one in this fixture; SSTV's only text field is a picture's
  // caption editor, drawn once a picture is loaded, so there it is the transmit-mode picker.
  it.each(ESC_SCREENS.map((s) => [s.view, s] as const))('%s: Esc from inside a field halts', async (_view, s) => {
    await mountScreen(s)
    const live = (el: HTMLElement): boolean =>
      el.closest('[hidden]') == null && !(el as HTMLInputElement).readOnly && !(el as HTMLInputElement).disabled
    const first = (sel: string): HTMLElement | undefined => [...document.querySelectorAll<HTMLElement>(sel)].find(live)
    const field =
      first('main input:not([type]), main input[type="text"], main input[type="search"], main textarea') ??
      (s.view === 'sstv' ? first('main select') : undefined)
    expect(field, `${s.view}: no field on screen, so this test would press Esc in nothing`).toBeDefined()
    field!.focus()
    expect(document.activeElement).toBe(field)
    const sent = await fire(() => pressEsc(field!))
    expect(sent[0], `${s.view}: Esc pressed in a field did not halt`).toBe('halt_tx')
  })

  it.each(ESC_SCREENS.map((s) => [s.view, s] as const))(
    '%s: a control that stops the key on its way cannot swallow the stop',
    async (_view, s) => {
      await mountScreen(s)
      const button = (s.drawn()!.closest('main') ?? shown('main'))?.querySelector<HTMLButtonElement>('button:not([disabled])') ?? null
      expect(button, `${s.view}: no enabled button inside the screen`).not.toBeNull()
      const swallow = (e: Event): void => e.stopPropagation()
      button!.addEventListener('keydown', swallow)
      try {
        expect((await fire(() => pressEsc(button!)))[0]).toBe('halt_tx')
      } finally {
        button!.removeEventListener('keydown', swallow)
      }
    },
  )
})

describe('a menu or panel that Esc closes still closes, and the halt is sent on the same press', () => {
  const popover = (): HTMLElement | null => shown('.panels-menu-pop')
  // FT and CW are the reference: their own Esc, measured with the ⊞ menu open. Phone and SSTV must
  // do the same.
  it.each([
    ['operate (the reference)', 'operate', ['halt_tx']],
    ['cw (the reference)', 'cw', ['stop_cw', 'halt_tx']],
    ['phone', 'phone', ['halt_tx', 'stop_voice']],
    ['sstv', 'sstv', ['halt_tx']],
  ] as const)('%s: the ⊞ Panels menu closes and the stop is sent', async (_name, view, esc) => {
    everySectionOn()
    await mountOn(view)
    fireEvent.click(onScreenButton(/^⊞ panels/i))
    await waitFor(() => expect(popover(), 'control: the ⊞ menu opened').not.toBeNull())
    const inside = popover()!.querySelector<HTMLButtonElement>('button:not([disabled])')
    expect(inside, 'control: a button to focus inside the menu').not.toBeNull()
    inside!.focus()
    const sent = await fire(() => pressEsc(inside!))
    await waitFor(() => expect(popover(), 'Esc did not close the ⊞ menu').toBeNull())
    expect(sent).toEqual([...esc])
  })

  // A Radix menu and a Radix dialog dismiss only an Esc that nothing has cancelled, so these are what
  // would catch a stop listener that cancelled the key: the top bar's Help menu, and the Getting
  // started dialog it opens. On Tempo, and on Operate as the reference.
  it.each([
    ['operate (the reference)', 'operate', 'dx'],
    ['chat', 'chat', 'msg'],
  ] as const)('%s: the Help menu, then the dialog it opens, each close on Esc and each Esc sends the halt', async (_name, view, area) => {
    everySectionOn()
    await mountOn(view, area)
    const help = (): HTMLElement => {
      const hits = screen.getAllByRole('button', { name: EN['topbar.help.label'], hidden: true }).filter((el) => el.closest('[hidden]') == null)
      expect(hits, 'control: one Help button on screen').toHaveLength(1)
      return hits[0]
    }
    const openHelp = async (): Promise<HTMLElement> => {
      fireEvent.pointerDown(help(), { button: 0, ctrlKey: false, pointerType: 'mouse' })
      return screen.findByRole('menuitem', { name: EN['gettingStarted.title'] })
    }
    const item = await openHelp()
    expect(await fire(() => pressEsc(item))).toEqual(['halt_tx'])
    await waitFor(() => expect(screen.queryByRole('menuitem'), 'Esc did not close the Help menu').toBeNull())
    fireEvent.click(await openHelp())
    const dialog = await screen.findByRole('dialog')
    expect(await fire(() => pressEsc(dialog))).toEqual(['halt_tx'])
    await waitFor(() => expect(screen.queryByRole('dialog'), 'Esc did not close the dialog').toBeNull())
  })

  it("aprs: the internet panel closes and the halt is sent", async () => {
    await mountScreen(ESC_SCREENS.find((s) => s.view === 'aprs')!)
    fireEvent.click(shown('button.aprs-inet')!)
    await waitFor(() => expect(shown('.aprs-inet-panel'), 'control: the internet panel opened').not.toBeNull())
    expect(await fire(() => pressEsc(shown('.aprs-inet-panel')!))).toEqual(['halt_tx'])
    await waitFor(() => expect(shown('.aprs-inet-panel'), 'Esc did not close the internet panel').toBeNull())
  })
})

describe('Esc on every screen in the registry: the five above gained the halt, and nothing else changed', () => {
  // Each cockpit that binds its own Esc, with what it sends in this fixture (RTTY and PSK are on the
  // air in it, so their own stop goes first). Any other section sends nothing: it has no Esc stop.
  const OWN_ESC: Partial<Record<View, string[]>> = {
    operate: ['halt_tx'],
    cw: ['stop_cw', 'halt_tx'],
    rtty: ['rtty_stop', 'halt_tx'],
    psk: ['psk_stop', 'halt_tx'],
    js8: ['halt_tx'],
  }
  const SECTIONS = sectionFeatures().map((f) => f.id as View)

  it('control: the census covers the app’s screens, and the five', () => {
    expect(SECTIONS.length).toBeGreaterThan(15)
    for (const s of ESC_SCREENS) expect(SECTIONS).toContain(s.view)
  })

  it.each(SECTIONS)('%s', async (view) => {
    everySectionOn()
    // Field Day is drawn only with its master switch on.
    if (view === 'fieldDay') settingsAnswer = { ...defaultSettings, fdActive: true }
    await mountOn(view, view === 'chat' ? 'msg' : 'dx')
    expect(document.title, `control: ${view} is the screen on show`).toBe(`${featureById(view)!.label} — Nexus`)
    ;(document.activeElement as HTMLElement | null)?.blur()
    const expected = ESC_SCREENS.find((s) => s.view === view)?.esc ?? OWN_ESC[view] ?? []
    expect(await sentBy(() => pressEsc()), `${view}: what Esc sent`).toEqual(expected)
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

// ── the top bar's Stop TX, where it is the only one ─────────────────────────────────────────
//
// Twelve screens have no cockpit and so no TX strip: Connect, Needed, Spots, DXped, Logbook,
// Awards, Stats, Field Day, POTA/SOTA, Memories, Program and Settings. Their Stop TX is the top
// bar's cluster. At the small size they drop the FT-only items from the bar (operator,
// 2026-10-01), keyed on the bar's `topbar--ft` class, and keep the cluster; App's
// `hideDigitalChrome` would have taken both, which is why Field Day is not on that list. Here,
// with the real App: each of the twelve draws the bar's Stop TX, enabled, and the press sends
// halt_tx, and its bar is not marked an FT screen; Operate and Tempo, the two that are, carry
// the mark. The CSS half (no rule in either sheet hides the cluster at any size) is
// TopBar.small.test.tsx; the geometry is the real-browser sweep's.
describe("the top bar's Stop TX on every screen without a cockpit", () => {
  const COCKPITS: View[] = ['operate', 'chat', 'phone', 'cw', 'rtty', 'psk', 'sstv', 'aprs', 'js8', 'sats']
  const BAR_STOP = sectionFeatures()
    .map((f) => f.id as View)
    .filter((v) => !COCKPITS.includes(v))

  it('control: the census is the twelve screens without a cockpit, Field Day among them', () => {
    expect([...BAR_STOP].sort()).toEqual([
      'awards', 'connect', 'dxped', 'fieldDay', 'logbook', 'memories', 'needed', 'pota', 'program', 'settings', 'spots', 'stats',
    ])
  })

  it.each(BAR_STOP)('%s: on screen, enabled, and it sends halt_tx', async (view) => {
    everySectionOn()
    // Field Day is drawn only with its master switch on.
    if (view === 'fieldDay') settingsAnswer = { ...defaultSettings, fdActive: true }
    await mountOn(view)
    expect(document.title, `control: ${view} is the screen on show`).toBe(`${featureById(view)!.label} — Nexus`)
    const bar = document.querySelector<HTMLElement>('header.topbar')!
    expect(bar.classList.contains('topbar--ft'), `${view} is not an FT screen`).toBe(false)
    const stop = within(bar).getByRole('button', { name: STOP_TX }) as HTMLButtonElement
    expect(stop.disabled, `${view}: the bar's Stop TX is enabled`).toBe(false)
    expect(await fire(() => fireEvent.click(stop))).toEqual(['halt_tx'])
  })

  it.each([
    ['operate', 'dx'],
    ['chat', 'msg'],
  ] as const)('%s is an FT screen: its bar is marked, and its stop is its own strip, not the bar', async (view, area) => {
    everySectionOn()
    await mountOn(view, area)
    const bar = document.querySelector<HTMLElement>('header.topbar')!
    expect(bar.classList.contains('topbar--ft')).toBe(true)
    expect(within(bar).queryByRole('button', { name: STOP_TX })).toBeNull()
  })
})

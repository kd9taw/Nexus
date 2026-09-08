// @vitest-environment jsdom
//
// THE JS8 SECTION MUST NOT RENDER THE TEMPO WORKSPACE UNDERNEATH IT.
//
// Operator report (2026-09, running the build): on the JS8 screen — "why is there tempo fast and
// tempo slow listed? These are not relevant to js8call", "also bringing in tempo chats which is
// wrong", and a "split waterfall" they did not ask for.
//
// All three are ONE defect. `Js8Cockpit` lives in a keep-alive host (`.js8-host`) that renders
// ALONGSIDE `{workspace}` — the switch on `effectiveView` in App.tsx. Every other keep-alive
// cockpit (operate, rtty, psk, sstv, aprs) has a `case` in that switch that sets
// `workspace = null`, because the host below already drew the view. JS8 had NO case, so it fell
// through to `case 'chat': default:` and App drew the whole Tempo three-pane on top of it:
//   • TempoHeader's tier chips  → "TempoFast" / "TempoDeep"  (the "tempo fast / tempo slow" row)
//   • Conversation + stationsPanel                            (the "tempo chats")
//   • the right rail's FT Waterfall beside the cockpit's own  (the "split waterfall")
//
// This is the 0.4–0.21 "two mains fighting for the shell" class that host-hidden.test.ts guards
// from the CSS side; the same collision is reachable from the SWITCH side, and nothing guarded
// that. This file does, by MOUNTING the real App — the reports are about what is on the screen,
// so a source-level grep is not the evidence that settles them.
//
// The controls matter as much as the assertions: 'chat' MUST still show all of it (that is the
// Tempo view), and 'psk' — a keep-alive cockpit that already had its case — must show none of
// it. Without those, an App that failed to mount at all would "pass".
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import type { AppSnapshot } from './types'

// ── the snapshot the app boots on ───────────────────────────────────────────────────────────
// Two stations on Tempo tiers, so the roster the bleed drags in has visible rows.
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
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1500,
    txLevel: 0.5,
    slot: 0,
  },
  aiCw: { enabled: false, status: '', text: '' },
  link: {
    tier: 'TempoFast',
    periodSecs: 15,
    snrDb: -8,
    dtSec: 0.1,
    freqHz: 1500,
    rv: 0,
    state: 'idle',
    quality: 1,
  },
  stations: [
    { call: 'W0IND', grid: 'EN52', snr: -8, tier: 'TempoFast', lastSeen: 0, freqHz: 1210 },
    { call: 'N0CALL', grid: 'EM48', snr: -12, tier: 'TempoDeep', lastSeen: 0, freqHz: 1400 },
  ],
  conversations: [],
  activePeer: null,
  qso: null,
  fieldDay: null,
  recentDecodes: [],
  harqRescues: 0,
} as unknown as AppSnapshot

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

// Every export auto-stubbed from the real module (the Js8Cockpit.structure.test.tsx pattern), so
// an api call added to App later cannot make this suite throw on mount; the handful App actually
// needs a SHAPE from are given one.
vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getSnapshot: vi.fn(async () => snapshot),
    subscribeSnapshot: vi.fn(() => () => {}), // App calls it un-awaited: the unsubscribe is sync
    getAwards: vi.fn(async () => ({ achievements: [] })),
    getJourney: vi.fn(async () => ({ firsts: [], feats: [], ladders: [] })),
    getSettings: vi.fn(async () => null),
    getBandPlan: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getFdRuleset: vi.fn(async () => null),
    logOperators: vi.fn(async () => []),
    radioLaunchInfo: vi.fn(async () => ({ showPicker: false })),
    uiStateLoad: vi.fn(async () => ({})),
    uiStateSave: vi.fn(async () => ({})),
    getAllSpots: vi.fn(async () => []),
    getNeedAlerts: vi.fn(async () => []),
    getPropagation: vi.fn(async () => null),
    getFeedHealth: vi.fn(async () => null),
    getXrayNow: vi.fn(async () => null),
    getDxpedWindows: vi.fn(async () => []),
    getSatSchedule: vi.fn(async () => []),
    getSatTrackStatus: vi.fn(async () => null),
    getIssPass: vi.fn(async () => null),
    getTleStatus: vi.fn(async () => null),
    // The two commands App fires on mount whose answer it feeds straight back into setSnap
    // (the rig-mode assert and the persisted-area sync) — they must answer with a SNAPSHOT.
    setOperatingMode: vi.fn(async () => snapshot),
    setArea: vi.fn(async () => snapshot),
    appVersion: vi.fn(async () => '0.0.0-test'), // TopBar renders it directly
    getJs8State: vi.fn(async () => js8State),
    js8Enter: vi.fn(async () => js8State),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The waterfalls are the third symptom, so they must be COUNTABLE — one stub, one testid, used
// by both the cockpit's scope and the Tempo right rail (same module for both importers).
vi.mock('./components/Waterfall', () => ({
  Waterfall: () => <div data-testid="waterfall" />,
}))

import App from './App'

function enableJs8(): void {
  localStorage.setItem(
    'nexus.features.v1',
    JSON.stringify({ profile: 'custom', enabled: { js8: true } }),
  )
}

beforeEach(() => {
  localStorage.clear()
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
  enableJs8()
})
afterEach(cleanup)

/** Mount App on `view` (the boot hash is the deeplink path resolveBootView honours) and wait
 *  for the snapshot to land — before it does, App renders only "Connecting to Nexus…".
 *  `area` is the persisted workspace: booting on Tempo needs 'msg', because App reconciles a
 *  'chat' view against a 'dx' workspace by sending it to Operate. */
async function mountOn(view: string, area: 'dx' | 'msg' = 'dx'): Promise<void> {
  localStorage.setItem('nexus.workspace', area)
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
}

/** The three Tempo surfaces the operator named, by what is actually on the screen.
 *  The tier row is TempoHeader's own group (`tempo.header.tier.aria`) — not a text match, so
 *  a JS8 speed chip or a roster row cannot be mistaken for it. */
const tempoTierRow = () => document.querySelector('[aria-label="Tempo tier"]')
const tempoTierText = () => tempoTierRow()?.textContent ?? null
const tempoRoster = () => document.querySelector('.grid-stations')
const tempoChat = () => document.querySelector('.grid-center')
const tempoRail = () => document.querySelector('.right-rail')

/** Elements NOT inside a collapsed keep-alive host. Every cockpit stays MOUNTED across
 *  navigation by design — `.rtty-host[hidden]` etc. are `display:none` (host-hidden.test.ts) —
 *  so a raw querySelectorAll counts cockpits the operator cannot see. Only the un-hidden ones
 *  are "on the screen", which is the only thing this file is about. */
function onScreen(sel: string): Element[] {
  return [...document.querySelectorAll(sel)].filter((el) => el.closest('[hidden]') == null)
}
const waterfalls = () => onScreen('[data-testid="waterfall"]')

describe('the JS8 section', () => {
  it('shows no Tempo tier chips, no Tempo chat rail, and exactly ONE waterfall', async () => {
    await mountOn('js8')

    // The cockpit really is on screen — otherwise the absences below prove nothing.
    expect(document.querySelector('.js8-host')).not.toBeNull()
    expect(document.querySelector('.js8-host')?.hasAttribute('hidden')).toBe(false)

    // SOFT, all of them: the operator named three symptoms, and a future regression should
    // report which of them came back rather than stopping at the first.
    // "why is there tempo fast and tempo slow listed?"
    expect.soft(tempoTierText(), 'TempoHeader tier chips are on the JS8 screen').toBeNull()
    // "also bringing in tempo chats which is wrong"
    expect.soft(tempoRoster(), 'the Tempo stations rail is on the JS8 screen').toBeNull()
    expect.soft(tempoChat(), 'the Tempo conversation pane is on the JS8 screen').toBeNull()
    expect.soft(tempoRail(), 'the Tempo right rail is on the JS8 screen').toBeNull()
    // the "split waterfall": the cockpit's own scope PLUS the Tempo rail's FT waterfall
    expect.soft(waterfalls(), 'two waterfalls on one screen').toHaveLength(1)
    // No second <main> on screen either — two mains in the shell is the shape of the defect.
    expect.soft(onScreen('main'), 'two <main> elements in the shell').toHaveLength(1)
  })

  it('control: the PSK section — a keep-alive cockpit with a case — drags none of it in either', async () => {
    await mountOn('psk')
    expect(tempoTierText()).toBeNull()
    expect(tempoRoster()).toBeNull()
    expect(tempoChat()).toBeNull()
    expect(tempoRail()).toBeNull()
  })

  it('control: Tempo itself still draws all three (the assertions above can fail)', async () => {
    await mountOn('chat', 'msg')
    expect(tempoTierText()).toContain('TempoFast')
    expect(tempoTierText()).toContain('TempoDeep')
    expect(tempoRoster()).not.toBeNull()
    expect(tempoChat()).not.toBeNull()
    expect(tempoRail()).not.toBeNull()
    expect(waterfalls().length).toBeGreaterThanOrEqual(1)
  })
})

// ── the SHAPE guard: the next cockpit, not this one ─────────────────────────────────────────
//
// The defect above was not a JS8 mistake, it was a MISSING PAIRING: a view can be given a
// keep-alive host without being given the `case` that empties the workspace slot, and nothing
// says so — the fall-through to `case 'chat': default:` is silent and draws a plausible-looking
// screen. So the pairing is derived from App.tsx rather than listed here: the next cockpit wired
// the same way fails HERE, at its own commit, instead of on an operator's screen.
//
// Read from source, not from a render, because the point is to catch a host that exists before
// anyone has written a test that mounts it.
describe('every keep-alive host has a workspace case (the pairing that was missing)', () => {
  const app = readFileSync(resolve(process.cwd(), 'src', 'App.tsx'), 'utf8')
  const hosts = [
    ...app.matchAll(/className="[a-z0-9-]+-host"\s+hidden=\{effectiveView !== '([A-Za-z0-9]+)'\}/g),
  ].map((m) => m[1])
  const switchBody = app.slice(
    app.indexOf('switch (effectiveView)'),
    app.indexOf('  return (\n    <div className="app">'),
  )
  /** Every `case 'x':` label whose ARM assigns `workspace = null`. Walked line by line rather
   *  than split on `case`, because the arms that matter are exactly the FALL-THROUGH ones —
   *  several labels stacked over one body — and splitting on the label would hand the body to
   *  the last of them only. Labels accumulate until a body line resolves them; `break` clears. */
  const nulledViews: string[] = (() => {
    const out: string[] = []
    let pending: string[] = []
    for (const line of switchBody.split('\n')) {
      const label = /^\s*case '([A-Za-z0-9]+)':\s*$/.exec(line)
      if (label) {
        pending.push(label[1])
        continue
      }
      if (/\bworkspace = null\b/.test(line)) out.push(...pending)
      if (/^\s*break\b/.test(line)) pending = []
    }
    return out
  })()

  it('finds hosts and null-workspace cases to compare (the parse is real)', () => {
    // ⚠️ `[A-Za-z0-9]` and not `[a-z]`: 'js8' has a DIGIT in it, and a letters-only pattern
    // drops it from BOTH sides of the comparison — which reads as a clean bill of health.
    expect(hosts).toContain('operate')
    expect(hosts).toContain('js8')
    expect(hosts.length).toBeGreaterThanOrEqual(6)
    expect(nulledViews).toContain('operate')
  })

  it.each(hosts)("'%s' has a case that leaves the workspace slot empty", (view) => {
    expect(
      nulledViews,
      `'${view}' has a keep-alive host but no 'workspace = null' case — it falls through to ` +
        `default: and draws the Tempo three-pane (header chips, roster, conversation, a second ` +
        `waterfall) underneath its own cockpit`,
    ).toContain(view)
  })
})

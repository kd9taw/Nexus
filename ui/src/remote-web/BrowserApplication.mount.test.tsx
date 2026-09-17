// @vitest-environment jsdom
// The whole BrowserApplication, mounted, and driven through boot.
//
// Why this file exists. BrowserApplication renders a pre-boot shell (one root element) and
// then, once the first snapshot lands, the workspace under an ErrorBoundary (a different root
// element). React unmounts the shell's subtree, and everything in it — the SessionStatus bar
// with the Listen control — is torn down and mounted again inside the workspace. Every other
// test renders App or the controls alone, so the one transition every real session goes
// through was exercised by nothing, and a control that was permanently disabled by that
// unmount shipped with every gate green.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { memo, useEffect, useState } from 'react'
import type { ComponentProps, FunctionComponent, MemoExoticComponent } from 'react'
import { BrowserApplication } from './BrowserApplication'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { AudioLink } from './audio-listen'
import type { AudioEnvironment } from './audio-listen'
import type { HostedConnection } from './client'
import type { ApplicationClient } from './application-client'
import type { OperationState } from './operation-protocol'
import type { MonitorSource } from '../remote-monitor/session'
import { APPLICATION_COMMANDS } from './application-protocol'
import fixtures from '../remote-monitor/fixtures.v2.json'
import settingsFixture from '../components/__fixtures__/defaultSettings.json'
import type { AppSnapshot } from '../types'

// The real App, counted. Wrapping the export rather than a stub: a stub would measure how
// often BrowserApplication asks for a render, and the question here is how often the
// 3,000-line workspace actually runs. Hooks called inside `render` attach to the counting
// component's fiber, so the wrapped body behaves as it would unwrapped; a memoised export
// stays memoised by wrapping the count in the same memo.
let appRenders = 0
vi.mock('../App', async importOriginal => {
  const module = await importOriginal<typeof import('../App')>()
  const exported = module.default as unknown as FunctionComponent | MemoExoticComponent<FunctionComponent>
  const memoised = 'type' in exported
  const body = memoised ? exported.type : exported
  const Counted = (props: ComponentProps<typeof body>) => { appRenders++; return body(props) }
  return { ...module, default: memoised ? memo(Counted) : Counted }
})

const snapshot = {
  mycall: 'N0CALL', mygrid: 'AA00', mode: 'Normal',
  radio: { dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', operatingMode: 'digital',
    transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500,
    txLevel: 0.5, slot: 0, nextSlotMs: 12000 },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [], conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [], harqRescues: 0,
} as unknown as AppSnapshot

const clients: OperationClient[] = []
beforeEach(() => {
  appRenders = 0
  localStorage.clear()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  window.matchMedia = ((media: string) => ({ matches: false, media, addEventListener() {}, removeEventListener() {},
    addListener() {}, removeListener() {} })) as unknown as typeof window.matchMedia
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.disconnected()); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks() })

function environment(): AudioEnvironment {
  return {
    now: () => Date.now(), decoderAvailable: () => true,
    context: () => Promise.resolve({ sampleRate: 48000, push: () => {}, reset: () => {}, close: () => Promise.resolve() }),
    decoder: () => ({ configure: () => {}, decode: () => {}, close: () => {} }),
    document: { visibilityState: 'visible', addEventListener: () => {}, removeEventListener: () => {} },
  }
}

/** A live station's observation lane: a fresh frame on every poll. */
function liveObservation(): MonitorSource {
  let sequence = 0
  return { id: 'test', kind: 'fixture', read: async () => ({ ...structuredClone(fixtures.spe), sequence: ++sequence }) }
}
/** An observation lane that never answers: the monitor keeps its single-flight slot and
 *  publishes nothing, so the workspace host's OWN timers are the only thing that ticks. */
function silentObservation(): MonitorSource {
  return { id: 'test', kind: 'fixture', read: () => new Promise(() => {}) }
}

/** A hosted connection as BrowserApplication sees it: a ready application lane answering the
 *  five boot reads, a real operation client holding station control with audio offered, and a
 *  real audio link whose wire is captured. */
function connection(source: MonitorSource = liveObservation()) {
  vi.useFakeTimers()
  const sent: { request: { type: string; requestId: string } }[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value) }, removeItem: (key: string) => { values.delete(key) } }
  const operations = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined,
    4, pendingControlStorage(() => storage, 'browser-application-test', async (_key, run) => run()))
  clients.push(operations)
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(),
    revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000,
    actions: [], txArmed: false,
    controls: { context: { radioId: 0, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: ['audioListen'] },
  }
  operations.open()
  operations.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value: state })
  const messages: Record<string, unknown>[] = []
  const audio = new AudioLink(message => messages.push(message as Record<string, unknown>), environment())
  const listeners = new Set<() => void>()
  // One object per command, handed back on every read: the real client runs shareStructure
  // over each reply, so an unchanged station sample IS the previous object and a poll that
  // lands it renders nothing. A fresh clone per read would be a change the real wire never makes.
  const replies: Record<string, unknown> = { get_snapshot: structuredClone(snapshot), get_settings: structuredClone(settingsFixture), get_band_plan: [],
    get_spectrum_row: { row: [], loHz: 0, hiHz: 4000, source: 'audio' }, get_meters: { rxLevel: 0, smeterDb: null, cwToneHz: null } }
  const application = {
    kind: 'remote', getPhase: () => 'ready', age: () => 0,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener) } },
    supports: (command: string) => (APPLICATION_COMMANDS as readonly string[]).includes(command),
    invoke: async (command: string) => {
      const value = replies[command]
      if (value === undefined) throw new Error('applicationUnsupported')
      return value
    },
  } as unknown as ApplicationClient
  return { connection: { application, operations, audio, source } as unknown as HostedConnection, messages, state }
}

const booted = (container: HTMLElement) => container.querySelector('.remote-service-app') === null && container.querySelector('.app') !== null

it('the Listen control still sends after the workspace boots', async () => {
  const h = connection()
  const { container } = render(<BrowserApplication connection={h.connection} disconnect={() => {}} />)
  const shellButton = screen.getByRole('button', { name: 'Listen' })
  expect(booted(container)).toBe(false)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(booted(container)).toBe(true)
  // The precondition that makes this test mean anything: the control in the workspace is a
  // REMOUNT of the one in the shell, so its unmount-time cleanup has run.
  const button = screen.getByRole('button', { name: 'Listen' })
  expect(button).not.toBe(shellButton)
  expect(button.hasAttribute('disabled')).toBe(false)
  await act(async () => { fireEvent.click(button) })
  expect(h.messages).toEqual([expect.objectContaining({ type: 'audioListen', listening: true, leaseId: h.state.leaseId })])
  expect(screen.getByText('Connecting audio').getAttribute('role')).toBe('status')
})

// Every 500 ms the workspace host re-renders itself to re-read the sample age. That changes
// nothing App renders, so it may not run App: the budget below is the boot and App's own
// 400 ms unread-badge ticker, and the host's tick contributes nothing. The observation lane
// is silent here on purpose. Its publishes DO run App, and by today's wiring rightly so -
// App subscribes to the rig observation through useReceiverSettings (App.tsx) for the Tempo
// waterfall's rx-offset fallback - so a live lane would put that separate cost inside this
// bound and hide what the bound is about.
const WINDOW_MS = 4000
const OWN_TICKS = Math.floor(WINDOW_MS / 400)
const BUDGET = 3 + OWN_TICKS

/** Advances the clock one timer at a time, each in its own act(). A single act() around the
 *  whole window would coalesce every update raised inside it into ONE render, and report one
 *  render for anything; a browser runs each timer callback as its own task and renders after
 *  each, which is what this reproduces. */
async function elapse(ms: number): Promise<void> {
  const until = Date.now() + ms
  for (let guard = 0; Date.now() < until; guard++) {
    if (guard > 10_000) throw new Error('the clock is not advancing')
    await act(async () => { await vi.advanceTimersToNextTimerAsync() })
  }
}

it("the host's own 500 ms tick never runs the workspace", async () => {
  const h = connection(silentObservation())
  const { container } = render(<BrowserApplication connection={h.connection} disconnect={() => {}} />)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  expect(booted(container)).toBe(true)
  const settled = appRenders
  await elapse(WINDOW_MS)
  expect(appRenders - settled).toBeLessThanOrEqual(BUDGET)
})

it('an unstable host prop does trip that bound, so it can fail', async () => {
  // The positive control: a parent that hands BrowserApplication a fresh callback every 500 ms
  // invalidates its status memo, which changes the `remote` identity, which is a real change
  // App has to render. That is the exact leak the bound is there to catch.
  const h = connection(silentObservation())
  function Host() {
    const [, setTick] = useState(0)
    useEffect(() => { const id = setInterval(() => setTick(value => value + 1), 500); return () => clearInterval(id) }, [])
    return <BrowserApplication connection={h.connection} disconnect={() => {}} />
  }
  render(<Host />)
  await act(async () => { await vi.advanceTimersByTimeAsync(0) })
  const settled = appRenders
  await elapse(WINDOW_MS)
  expect(appRenders - settled).toBeGreaterThan(BUDGET)
})

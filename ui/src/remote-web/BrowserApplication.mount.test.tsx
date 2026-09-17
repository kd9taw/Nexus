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

/** A hosted connection as BrowserApplication sees it: a ready application lane answering the
 *  five boot reads, a real operation client holding station control with audio offered, a
 *  real audio link whose wire is captured, and an observation source publishing a fresh
 *  frame on every poll — which is what a live station does. */
function connection() {
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
  const application = {
    kind: 'remote', getPhase: () => 'ready', age: () => 0,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener) } },
    supports: (command: string) => (APPLICATION_COMMANDS as readonly string[]).includes(command),
    invoke: async (command: string) => {
      if (command === 'get_snapshot') return structuredClone(snapshot)
      if (command === 'get_settings') return structuredClone(settingsFixture)
      if (command === 'get_band_plan') return []
      if (command === 'get_spectrum_row') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' }
      if (command === 'get_meters') return { rxLevel: 0, smeterDb: null, cwToneHz: null }
      throw new Error('applicationUnsupported')
    },
  } as unknown as ApplicationClient
  let sequence = 0
  const source: MonitorSource = { id: 'test', kind: 'fixture', read: async () => ({ ...structuredClone(fixtures.spe), sequence: ++sequence }) }
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

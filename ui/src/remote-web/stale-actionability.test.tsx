// @vitest-environment jsdom
// Stale station readings used to be retired with `visibility: hidden`, which also put every control
// in the workspace out of reach of the keyboard. They are now retired in place (faded, no pointer
// events), which is what stopped the black flash, so a Tab key can reach them. This sweep proves the
// JavaScript gates alone keep that safe: with the readings stale but station authority FRESH (every
// capability, a live lease, a transmit epoch), activating every enabled control in the real
// workspace sends the station nothing. The same sweep with current readings is the positive
// control: it must send, or the spy and the sweep prove nothing.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import App from '../App'
import settingsFixture from '../components/__fixtures__/defaultSettings.json'
import { installApplicationTransport } from '../applicationTransport'
import { StationControlContext, StationDataContext, RemoteOperationsContext } from '../stationAccess'
import type { AppSnapshot, Settings } from '../types'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { controlTransport } from './control-transport'
import { CONTROL_CAPABILITIES } from './station-operation'
import type { OperationState } from './operation-protocol'
import type { ApplicationClient } from './application-client'

const snapshot = {
  mycall: 'N0CALL', mygrid: 'AA00', mode: 'Normal',
  radio: { dialMhz: 7.074, band: '40m', catOk: true, sideband: 'USB', operatingMode: 'digital',
    transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500,
    txLevel: 0.5, slot: 0, nextSlotMs: 12000 },
  aiCw: { enabled: false, status: '', text: '' },
  link: { tier: 'FT8', periodSecs: 15, snrDb: -8, dtSec: 0.1, freqHz: 1500, rv: 0, state: 'idle', quality: 1 },
  stations: [{ call: 'W1AW', grid: 'FN31', snr: -10, freq: 1200, message: 'CQ W1AW FN31', lastHeard: 0 }],
  conversations: [], activePeer: null, qso: null, fieldDay: null, recentDecodes: [], harqRescues: 0,
} as unknown as AppSnapshot
let dispose: (() => void) | undefined
const clients: OperationClient[] = []
beforeEach(() => {
  localStorage.clear()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  window.matchMedia = ((media: string) => ({ matches: false, media, addEventListener() {}, removeEventListener() {},
    addListener() {}, removeListener() {} })) as unknown as typeof window.matchMedia
})
afterEach(() => { cleanup(); dispose?.(); clients.splice(0).forEach(c => c.disconnected()); vi.unstubAllGlobals(); vi.restoreAllMocks() })

function station(stale: boolean) {
  const sent: { request: { type: string; requestId: string } }[] = []
  const localOnly: string[] = []
  const values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  // A frozen clock keeps the authority fresh for the whole sweep and stops automatic re-reads.
  const client = new OperationClient(wire => sent.push(JSON.parse(wire)), true, () => 1000, undefined, 4,
    pendingControlStorage(() => storage, 'stale-sweep', async (_key, run) => run()))
  clients.push(client)
  const state: OperationState = {
    stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
    transmitEpoch: '0000000000000001',
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: 1, ampReadSequence: 1 }, capabilities: [...CONTROL_CAPABILITIES] }
  }
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value: state } as never)
  expect(client.getSnapshot().fresh).toBe(true)
  const reads = { kind: 'remote' as const, invoke: async <T,>(command: string): Promise<T> => {
    if (command === 'get_snapshot') return structuredClone(snapshot) as T
    if (command === 'get_settings') return structuredClone(settingsFixture) as T
    if (command === 'get_band_plan') return [] as T
    if (command === 'get_spectrum_row') return { row: [], loHz: 0, hiHz: 4000, source: 'audio' } as T
    if (command === 'get_meters') return { rxLevel: 0, smeterDb: null, cwToneHz: null } as T
    // Other reads fail the way an unsupported read does, and components catch that. A local
    // desktop action (pop-out windows and the like) is called fire-and-forget; the hosted read
    // allowlist refuses it before any station, so here it is a recorded no-op instead of an
    // unhandled rejection. Station commands never come through this path: they go to `sent`.
    if (command.startsWith('get_')) throw new Error('applicationUnsupported')
    localOnly.push(command)
    return undefined as T
  } }
  dispose = installApplicationTransport(controlTransport(reads, { age: () => 0 } as unknown as ApplicationClient, client))
  const ui = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={!stale}>
    <RemoteOperationsContext.Provider value={client}>
      <App remote={{ snapshot, settings: settingsFixture as unknown as Settings, bandPlan: [], stale, status: <div className="remote-application-status">Session</div> }} />
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  const commands = () => sent.filter(m => ['stationControl', 'logManual', 'acquire'].includes(m.request.type))
  return { ...ui, client, commands }
}

/** Activate every enabled control in the workspace (never the session banner), by click and by
 * keyboard, re-querying after each so controls a click reveals are reached too. */
async function sweep(root: Element) {
  const activated = new Set<Element>()
  const selector = ':scope > .app > :not(.remote-application-status) :is(button, [role="button"], [role="menuitem"], [role="tab"], input[type="checkbox"])'
  for (let round = 0; round < 300; round++) {
    const next = [...root.querySelectorAll<HTMLElement>(selector)]
      .find(e => !activated.has(e) && !(e as HTMLButtonElement).disabled && e.getAttribute('aria-disabled') !== 'true' && !e.hasAttribute('data-remote-stop'))
    if (!next) break
    activated.add(next)
    await act(async () => {
      fireEvent.keyDown(next, { key: 'Enter' })
      fireEvent.click(next)
      await new Promise(resolve => setTimeout(resolve, 0))
    })
  }
  for (const readout of root.querySelectorAll<HTMLElement>('.readout')) {
    await act(async () => {
      fireEvent.keyDown(readout, { key: 'ArrowUp' })
      fireEvent.wheel(readout, { deltaY: -100, deltaMode: 0 })
      await new Promise(resolve => setTimeout(resolve, 150))
    })
  }
  return activated.size
}

it('with stale readings and fresh authority, no enabled workspace control sends a station command', async () => {
  const h = station(true)
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 50)) })
  expect(h.container.querySelector('.app')?.getAttribute('data-remote-stale')).toBe('true')
  const reached = await sweep(h.container)
  expect(reached).toBeGreaterThan(5)
  expect(h.commands()).toEqual([])
}, 120_000)

it('positive control: the same sweep with current readings does send a station command', async () => {
  const h = station(false)
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 50)) })
  await sweep(h.container)
  expect(h.commands().length).toBeGreaterThan(0)
}, 120_000)

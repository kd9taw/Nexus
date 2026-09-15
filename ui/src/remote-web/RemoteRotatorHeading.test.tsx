// @vitest-environment jsdom
// The rotator HEADING from a browser (application v17). The station measures it; the page only
// shows what came back. Three things are pinned here: a bearing reaches the strip and the Needed
// board, UNKNOWN stays "—" rather than becoming a number, and nothing is read while nobody is
// looking at a heading.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import { RotorStrip } from '../components/RotorStrip'
import { NeededPanel } from '../components/NeededPanel'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { ApplicationClient } from './application-client'
import type { OperationState } from './operation-protocol'
import type { QueryPage } from './application-query-protocol'
import type { NeedAlert } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ rotatorModel: 1, rotatorHost: '' })),
  readRotator: vi.fn(async () => 123),
  pointRotator: vi.fn(async () => {}),
  stopRotator: vi.fn(async () => {}),
  stopSatTrack: vi.fn(async () => {}),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  setSatTransponder: vi.fn(async () => {}),
  getDeclination: vi.fn(async () => null),
  openQrzPage: vi.fn(async () => {}),
}))

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

const page = (source: unknown): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(),
  collection: 'rotator' as QueryPage['collection'], snapshotId: crypto.randomUUID(), offset: 0, total: 0, retained: 0,
  nextCursor: null, ageMs: 0, rows: [], meta: { capturedAgeMs: 0, source } as QueryPage['meta'] })

function operations() {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient(s => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 3, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: ['rotator'] } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  return client
}

function station(source: unknown, supported = true) {
  const invoke = vi.fn(async (): Promise<unknown> => page(source))
  const collections = new RemoteCollections({ invoke, supports: (c: string) => supported && c === 'get_remote_rotator', getPhase: () => 'ready' } as unknown as ApplicationClient)
  const ops = operations()
  const tree = (children: ReactNode) => (
    <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
      <RemoteOperationsContext.Provider value={ops}>
        <RemoteCollectionsContext.Provider value={collections}>{children}</RemoteCollectionsContext.Provider>
      </RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>)
  return { invoke, collections, tree, wrap: (children: ReactNode) => render(tree(children)) }
}

const settle = () => act(async () => { for (let i = 0; i < 12; i++) await Promise.resolve() })

const need = { call: 'JA1ABC', entity: 'Japan', band: '20m', zone: 25, tags: [], priority: 1, headline: 'New one', mode: 'CW', freqMhz: 14.02 } as unknown as NeedAlert

it('shows the bearing the station measured, in the cockpit strip and on the Needed board', async () => {
  const strip = station({ configured: true, azimuthDeg: 212.4 })
  strip.wrap(<RotorStrip targetCall="JA1ABC" onPointAt={vi.fn()} />)
  await settle()
  expect(screen.getByTitle(t('rotor.strip.az.title', { deg: 212 })).textContent).toContain('212')
  expect(screen.queryByTitle(t('remote.b1.rotatorNoHeading'))).toBeNull()
  expect(strip.invoke.mock.calls[0][0]).toBe('get_remote_rotator')
  strip.collections.dispose()
  cleanup()

  const board = station({ configured: true, azimuthDeg: 45 })
  const view = board.wrap(<NeededPanel alerts={[need]} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} onWork={() => {}} onPoint={vi.fn()} />)
  await settle()
  expect(view.container.querySelector('.np-rotator-az')!.textContent).toBe('45°')
  board.collections.dispose()
})

it('reads UNKNOWN as unknown: a silent rotctld never becomes a bearing', async () => {
  const silent = station({ configured: true, azimuthDeg: null })
  const view = silent.wrap(<RotorStrip targetCall="JA1ABC" onPointAt={vi.fn()} />)
  await settle()
  expect(screen.getByTitle(t('remote.b1.rotatorNoHeading'))).toBeTruthy()
  expect(view.container.querySelector('svg')).toBeNull()
  expect(silent.invoke).toHaveBeenCalled()
  silent.collections.dispose()
  cleanup()

  // A station that predates the read offers no heading command at all. Positive control above:
  // the same component with a station that does offer it painted 212°.
  const older = station({ configured: true, azimuthDeg: 212.4 }, false)
  older.wrap(<RotorStrip targetCall="JA1ABC" onPointAt={vi.fn()} />)
  await settle()
  expect(screen.getByTitle(t('remote.b1.rotatorNoHeading'))).toBeTruthy()
  expect(older.invoke).not.toHaveBeenCalled()
  older.collections.dispose()
})

it('reads nothing while no heading is on screen', async () => {
  const idle = station({ configured: true, azimuthDeg: 212.4 })
  const view = idle.wrap(<RotorStrip active={false} targetCall="JA1ABC" onPointAt={vi.fn()} />)
  await settle()
  expect(idle.invoke).not.toHaveBeenCalled()
  // Positive control: the same station and the same strip DO read once it is on screen.
  view.rerender(idle.tree(<RotorStrip active targetCall="JA1ABC" onPointAt={vi.fn()} />))
  await settle()
  expect(idle.invoke).toHaveBeenCalled()
  idle.collections.dispose()
})

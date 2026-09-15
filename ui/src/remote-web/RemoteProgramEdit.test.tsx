// @vitest-environment jsdom
// Curating the station's working channel list from a browser: rename, reorder, drop a row, clear.
// Each gesture is one transaction against the `programming` document revision the page is showing,
// naming the row by its channel ID.
//
// ⚠️ jsdom cannot lay anything out, so nothing here is a claim about where these controls sit; it
// counts what is on screen, whether it is enabled, and what left for the station.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { RadioProgView } from '../components/RadioProgView'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { installApplicationTransport, type ApplicationTransport } from '../applicationTransport'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ApplicationClient } from './application-client'
import { navigationPages } from './__fixtures__/navigation-page'
import configurationProgramming from './__fixtures__/configuration-programming.json'
import { t } from '../i18n'

/** The station's own programming document, trimmed to four rows. The shipped fixture carries 1200
 *  channels, and rendering 1200 rows in jsdom several times over turns every assertion below into a
 *  wall-clock race on a busy machine. Four rows exercise the same code; the revision string is the
 *  document's, unchanged, because that is what a change has to echo back. */
const programming = {
  ...(configurationProgramming as any),
  projects: [{ ...(configurationProgramming as any).projects[0],
    channels: (configurationProgramming as any).projects[0].channels.slice(0, 4) }]
}

const toasts = vi.hoisted(() => [] as [string, string][])
vi.mock('../toast', async (actual) => ({ ...(await actual<typeof import('../toast')>()), pushToast: (text: string, kind: string) => { toasts.push([text, kind]) } }))
vi.mock('../confirm', async (actual) => ({ ...(await actual<typeof import('../confirm')>()), confirmDialog: async () => true }))

let dispose: (() => void) | undefined
afterEach(() => { cleanup(); dispose?.(); dispose = undefined; toasts.length = 0; vi.restoreAllMocks(); localStorage.clear() })

/** The station answers every change `applied`, and records what it was asked to do. */
function operations(capabilities: string[], outcome: 'applied' | 'rejected' = 'applied') {
  const values = new Map<string, string>(), sent: Record<string, any>[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const boot = crypto.randomUUID(), lease = crypto.randomUUID()
  const state = (): OperationState => ({ stationBootId: boot, allowed: true, phase: 'controlling', leaseId: lease, revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } as OperationState['controls'] })
  const client: OperationClient = new OperationClient(s => {
    const message = JSON.parse(s)
    sent.push(message)
    const answer = message.request.type === 'heartbeat' ? () => state()
      : message.request.type === 'logChange' ? () => ({ operation: 'logChange', operationId: message.request.requestId,
        ...(outcome === 'applied' ? { outcome, evidence: 'programSaved' } : { outcome, reason: 'contextChanged' }) })
        : null
    if (answer) queueMicrotask(() => client.receive({ type: 'operationResponse', requestId: message.request.requestId, value: answer() }))
    // A REAL clock, not a still one: after an applied change the station clears this browser's
    // command window, so the next gesture needs a heartbeat to bring a new one. A frozen clock
    // never heartbeats, and the second gesture would look blocked when it is only unscheduled.
  }, true, () => Date.now(), pendingLogStorage(() => data, 'station', lock), 4, pendingControlStorage(() => data, 'station', lock))
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0]!.request.requestId, value: state() })
  return { client, edits: () => sent.filter(m => m.request.change?.kind === 'programEdit').map(m => m.request.change) }
}

function program(capabilities: string[], outcome: 'applied' | 'rejected' = 'applied') {
  const invoke = vi.fn(async () => { throw new Error('applicationUnsupported') })
  dispose = installApplicationTransport({ kind: 'remote', invoke: invoke as unknown as ApplicationTransport['invoke'] })
  const source = new RemoteCollections({ supports: () => false, invoke } as unknown as ApplicationClient)
  const pages = navigationPages('programming', programming)
  vi.spyOn(source, 'page').mockImplementation(async args => pages[args.cursor ? Number(args.cursor.split(':')[1]) : 0])
  const ops = operations(capabilities, outcome)
  const view = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={ops.client}>
      <RemoteCollectionsContext.Provider value={source}><RadioProgView myGrid="FN31" catOk={true} /></RemoteCollectionsContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  return { ...view, ...ops }
}

/**
 * The SECOND channel row's controls, re-queried every time. Row 0 is no use: its ▲ is disabled at
 * the top of the list either way.
 *
 * ⚠️ Re-queried, never cached across a gesture: an applied change re-reads the station's document,
 * which re-renders the list, and a button captured before that is no longer the one on screen.
 */
const firstRow = async (container: HTMLElement) => {
  await waitFor(() => expect(container.querySelectorAll('.rp-chan-row').length).toBeGreaterThan(1), { timeout: 8000 })
  const row = container.querySelectorAll('.rp-chan-row')[1]!
  const buttons = [...row.querySelectorAll<HTMLButtonElement>('.rp-chan-btns button')]
  const named = (label: string) => buttons.find(b => b.getAttribute('aria-label') === label)!
  return {
    name: row.querySelector<HTMLInputElement>('.rp-chan-name')!,
    up: named(t('program.chan.moveUp.aria')),
    down: named(t('program.chan.moveDown.aria')),
    remove: named(t('program.chan.remove.aria')),
    clear: container.querySelector<HTMLButtonElement>('.rp-clear')!
  }
}

/** The fixture's rows are CH0000, CH0001, … under ids the station assigned. */
const idOf = (i: number) => programming.projects[0].channels[i].id as string

it.each([
  ['up', { action: 'move', id: idOf(1), by: -1 }],
  ['down', { action: 'move', id: idOf(1), by: 1 }],
  ['remove', { action: 'remove', id: idOf(1) }]
] as const)('sends %s as one keyed gesture against the revision this page is showing', async (pick, expected) => {
  // One gesture per render, deliberately: a write still waiting for its result blocks the next one
  // (RemoteLogCheck), so a second gesture in the same render waits on the heartbeat cadence and
  // turns a payload assertion into a wall-clock race.
  const { container, edits } = program(['programEdit'])
  const row = await firstRow(container)
  expect([row.up.disabled, row.down.disabled, row.remove.disabled, row.name.disabled, row.clear.disabled])
    .toEqual([false, false, false, false, false])
  fireEvent.click(row[pick])
  await waitFor(() => expect(edits().length).toBe(1), { timeout: 5000 })
  expect(edits()[0]!.edit).toEqual(expected)
  // It carries the document revision the page was showing — never a position.
  expect(edits()[0]!.revision).toBe(programming.revision)
  expect('index' in edits()[0]!.edit).toBe(false)
})

it('sends a rename once, on commit, and not per keystroke', async () => {
  const { container, edits } = program(['programEdit'])
  const row = await firstRow(container)
  row.name.focus()
  fireEvent.change(row.name, { target: { value: 'W1A' } })
  fireEvent.change(row.name, { target: { value: 'W1AW' } })
  // Nothing has left yet: a write per keystroke is what this commit-on-blur exists to avoid.
  expect(edits()).toEqual([])
  expect(row.name.value).toBe('W1AW')
  // Enter commits by leaving the field, which is what the blur handler sends on.
  fireEvent.keyDown(row.name, { key: 'Enter' })
  await waitFor(() => expect(edits().length).toBe(1), { timeout: 5000 })
  expect(edits()[0]!.edit).toEqual({ action: 'rename', id: idOf(1), name: 'W1AW' })
})

it('drops an escaped rename without sending anything, and sends nothing for an unchanged name', async () => {
  const { container, edits } = program(['programEdit'])
  const row = await firstRow(container)
  const was = row.name.value
  row.name.focus()
  fireEvent.change(row.name, { target: { value: 'SOMETHING' } })
  fireEvent.keyDown(row.name, { key: 'Escape' })
  await new Promise(resolve => setTimeout(resolve, 50))
  expect(edits()).toEqual([])
  expect(row.name.value).toBe(was)
  // Typing a name and putting it back is not a change either.
  row.name.focus()
  fireEvent.change(row.name, { target: { value: 'SOMETHING' } })
  fireEvent.change(row.name, { target: { value: was } })
  fireEvent.keyDown(row.name, { key: 'Enter' })
  await new Promise(resolve => setTimeout(resolve, 50))
  expect(edits()).toEqual([])
})

it('clears the whole list as one gesture, after the same confirm the desktop asks for', async () => {
  const { container, edits } = program(['programEdit'])
  const row = await firstRow(container)
  fireEvent.click(row.clear)
  await waitFor(() => expect(edits().length).toBe(1), { timeout: 5000 })
  expect(edits()[0]!.edit).toEqual({ action: 'clear' })
})

it('leaves every curation control dead, and sends nothing, without the station hint', async () => {
  const { container, edits } = program(['repeaterTuning', 'programExport'])
  const row = await firstRow(container)
  expect([row.up.disabled, row.down.disabled, row.remove.disabled, row.name.disabled, row.clear.disabled])
    .toEqual([true, true, true, true, true])
  fireEvent.click(row.up)
  fireEvent.click(row.remove)
  fireEvent.click(row.clear)
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(edits()).toEqual([])
})

it('says so when the station refuses the change as stale', async () => {
  const { container } = program(['programEdit'], 'rejected')
  const row = await firstRow(container)
  fireEvent.click(row.remove)
  await waitFor(() => expect(toasts).toContainEqual([t('remote.logChangeStale'), 'error']), { timeout: 5000 })
})

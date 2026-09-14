// @vitest-environment jsdom
// Remote parity batch 1: the Program section's Tune from a browser. The station's own working
// channel list is shown read-only; each FM channel carries a Tune that sends the machine (output,
// shift, offset, tone) and nothing else, live only while the station advertises repeaterTuning.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react'
import { RadioProgView } from '../components/RadioProgView'
import { RemoteCollections, RemoteCollectionsContext } from './collections'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { installApplicationTransport, type ApplicationTransport } from '../applicationTransport'
import { OperationClient, OperationFailure } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { ControlCapability } from './station-operation'
import type { ApplicationClient } from './application-client'
import { navigationPages } from './__fixtures__/navigation-page'
import configurationProgramming from './__fixtures__/configuration-programming.json'
import { t } from '../i18n'

const toasts = vi.hoisted(() => [] as [string, string][])
vi.mock('../toast', async (actual) => ({ ...(await actual<typeof import('../toast')>()), pushToast: (text: string, kind: string) => { toasts.push([text, kind]) } }))

let dispose: (() => void) | undefined
afterEach(() => { cleanup(); dispose?.(); dispose = undefined; toasts.length = 0; vi.restoreAllMocks(); localStorage.clear() })

function operations(capabilities: ControlCapability[]) {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient(s => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 3, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  return client
}

function program(capabilities: ControlCapability[], catOk = true, tune: (args: unknown) => Promise<unknown> = async () => ({})) {
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === 'repeater_tune') return tune(args)
    throw new Error('applicationUnsupported')
  })
  dispose = installApplicationTransport({ kind: 'remote', invoke: invoke as unknown as ApplicationTransport['invoke'] })
  const source = new RemoteCollections({ supports: () => false, invoke } as unknown as ApplicationClient)
  const pages = navigationPages('programming', configurationProgramming)
  vi.spyOn(source, 'page').mockImplementation(async args => pages[args.cursor ? Number(args.cursor.split(':')[1]) : 0])
  const view = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={operations(capabilities)}>
      <RemoteCollectionsContext.Provider value={source}><RadioProgView myGrid="FN31" catOk={catOk} /></RemoteCollectionsContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  return { ...view, invoke }
}

const firstTune = async (container: HTMLElement) => {
  await waitFor(() => expect(container.querySelector('.rp-chan-row')).toBeTruthy(), { timeout: 4000 })
  return container.querySelector<HTMLButtonElement>('.rp-chan-row .rp-tune')
}

it('tunes the station to a working-list repeater with exactly the machine while the station advertises it', async () => {
  const { container, invoke } = program(['repeaterTuning'])
  const tune = await firstTune(container)
  expect(tune).toBeTruthy()
  expect(tune!.disabled).toBe(false)
  fireEvent.click(tune!)
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('repeater_tune', { outputMhz: 146.94, shift: 'minus', offsetHz: 600000, toneHz: 100 }))
  expect(invoke.mock.calls.filter(([command]) => command !== 'repeater_tune')).toEqual([])
  // The channel list itself stays read-only from a browser.
  expect(container.querySelector<HTMLInputElement>('.rp-chan-row .rp-chan-name')!.disabled).toBe(true)
})

it('keeps Tune dead without the station hint and absent without CAT', async () => {
  const older = program(['frequency', 'workSpot'])
  const tune = await firstTune(older.container)
  expect(tune).toBeTruthy()
  expect(tune!.disabled).toBe(true)
  fireEvent.click(tune!)
  expect(older.invoke).not.toHaveBeenCalledWith('repeater_tune', expect.anything())
  cleanup(); dispose?.(); dispose = undefined
  const noCat = program(['repeaterTuning'], false)
  expect(await firstTune(noCat.container)).toBeNull()
})

it('says plainly when the station refuses the repeater input as outside the licence', async () => {
  const { container } = program(['repeaterTuning'], true, async () => { throw new OperationFailure('outsidePrivileges', true) })
  fireEvent.click((await firstTune(container))!)
  await waitFor(() => expect(toasts).toContainEqual([t('remote.b1.outsidePrivileges'), 'error']))
})

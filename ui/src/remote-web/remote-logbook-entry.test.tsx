// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from '../components/Logbook'
import { RemoteOperationsContext, StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from './collections'
import { OperationClient } from './operation-client'
import type { QueryPage } from './application-query-protocol'
import { t } from '../i18n'

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 1000 })
})
afterEach(() => { cleanup(); vi.restoreAllMocks() })

const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), collection: 'log',
  snapshotId: crypto.randomUUID(), offset: 0, total: 1, retained: 1, nextCursor: null, ageMs: 0, meta: {},
  rows: [{ call: 'W1AW', band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1788940800, confirmed: false, awardConfirmed: false,
    grid: 'FN31', country: 'United States', rstSent: '-10', rstRcvd: '-12' }] })

function station(actions: string[]) {
  const sent: Record<string, any>[] = []
  // A still clock: the first poll leaves and the state stays current with no heartbeat after it.
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => 1000,
    { read: () => null, write: () => {} }, 4)
  client.open()
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  reply({ stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions, txArmed: false, transmitEpoch: null,
    controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities: [] } })
  const source = { page: vi.fn(async () => page()) }
  render(<StationControlContext.Provider value={false}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source as unknown as RemoteCollections}>
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
    </RemoteCollectionsContext.Provider></RemoteOperationsContext.Provider></StationControlContext.Provider>)
  const manual = () => sent.filter(m => m.request.type === 'logManual').map(m => m.request)
  return { client, reply, source, manual }
}

it('offers no Logbook form without the station’s manual logging permission', async () => {
  const test = station([])
  await screen.findByText('W1AW')
  expect(screen.queryByRole('button', { name: t('logbook.form.open') })).toBeNull()
  test.client.disconnected()
})

it('logs a new contact from the Logbook form through the station’s manual entry, at station time', async () => {
  const test = station(['log.manual'])
  await screen.findByText('W1AW')
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.open') }))
  expect(screen.queryByText(t('logbook.field.txPower.label'))).toBeNull()
  fireEvent.change(screen.getByLabelText(t('logbook.field.call.label')), { target: { value: 'k1xyz' } })
  fireEvent.change(screen.getByLabelText(t('logbook.field.rstSent.label')), { target: { value: '59' } })
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.log') }))
  await waitFor(() => expect(test.manual()).toHaveLength(1))
  const [request] = test.manual()
  expect(request.record).toMatchObject({ call: 'K1XYZ', band: '20m', freqMhz: 14.074, mode: 'FT8', rstSent: '59',
    whenUnix: null, confirmed: false, awardConfirmed: false })
  await act(async () => test.reply({ outcome: 'applied', evidence: 'fileSynced', uploads: 'stationPipeline', operationId: request.requestId }))
  await waitFor(() => expect(test.source.page).toHaveBeenCalledTimes(2))
  expect(screen.queryByRole('button', { name: t('logbook.form.log') })).toBeNull()
  test.client.disconnected()
})

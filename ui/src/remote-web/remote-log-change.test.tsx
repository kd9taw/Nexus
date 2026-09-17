// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from '../components/Logbook'
import { ConfirmHost } from '../confirm'
import { RemoteOperationsContext, StationControlContext } from '../stationAccess'
import { RemoteCollectionsContext, type RemoteCollections } from './collections'
import { OperationClient } from './operation-client'
import { logTarget } from './operation-protocol'
import type { QueryPage } from './application-query-protocol'
import type { Json } from './application-protocol'
import { t } from '../i18n'

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 1000 })
})
afterEach(() => { cleanup(); vi.restoreAllMocks() })

const qso = (call: string) => ({ call, band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: 1788940800,
  confirmed: false, awardConfirmed: false, grid: 'FN31', country: 'United States', rstSent: '-10', rstRcvd: '-12' })
type Row = ReturnType<typeof qso> & Extract<Json, object>
const page = (rows: (string | Row)[]): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), collection: 'log',
  snapshotId: crypto.randomUUID(), offset: 0, total: rows.length, retained: rows.length, nextCursor: null, ageMs: 0,
  rows: rows.map(r => typeof r === 'string' ? qso(r) : r), meta: {} })

function station(capabilities: string[], rows: (string | Row)[] = ['W1AW', 'K1ABC']) {
  const sent: Record<string, any>[] = []
  // A still clock: the first poll leaves, the state stays current, and no heartbeat interleaves
  // with the replies below.
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => 1000,
    { read: () => null, write: () => {} }, 4)
  client.open()
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1].request.requestId, value })
  reply({ stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
    transmitEpoch: null, controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } })
  const source = { page: vi.fn(async () => page(rows)) }
  const view = render(<StationControlContext.Provider value={false}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source as unknown as RemoteCollections}>
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
    </RemoteCollectionsContext.Provider><ConfirmHost /></RemoteOperationsContext.Provider></StationControlContext.Provider>)
  const changes = () => sent.filter(m => m.request.type === 'logChange').map(m => m.request)
  return { client, reply, source, view, changes }
}

it('offers edit and delete only when the station does, and never the shack-only row actions', async () => {
  const without = station(['qsoLogging'])
  await screen.findByText('W1AW')
  expect(screen.queryByRole('button', { name: t('logbook.row.delete', { call: 'W1AW' }) })).toBeNull()
  without.client.disconnected(); cleanup()
  const test = station(['qsoLogging', 'logEdit'])
  await screen.findByText('W1AW')
  expect(screen.getByRole('button', { name: t('logbook.row.delete', { call: 'W1AW' }) })).toBeTruthy()
  expect(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'W1AW' }) })).toBeTruthy()
  expect(test.view.container.querySelectorAll('.log-rowactions select, .log-actions').length).toBe(0)
  expect(screen.queryByRole('button', { name: t('logbook.row.pushQrz.aria', { call: 'W1AW' }) })).toBeNull()
  test.client.disconnected()
})

it('deletes a remote row only after an explicit confirm, by the key of that row', async () => {
  const test = station(['logEdit'])
  await screen.findByText('W1AW')
  const remove = screen.getByRole('button', { name: t('logbook.row.delete', { call: 'W1AW' }) })
  fireEvent.click(remove)
  fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: t('logbook.delete.confirm') })).toBeNull())
  expect(test.changes()).toHaveLength(0)
  fireEvent.click(remove)
  fireEvent.click(await screen.findByRole('button', { name: t('logbook.delete.confirm') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change).toEqual({ kind: 'delete', target: await logTarget(qso('W1AW')) })
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'fileSynced' }))
  await waitFor(() => expect(test.source.page).toHaveBeenCalledTimes(2))
  test.client.disconnected()
})

it('marks a paper card from the row menu, by row key, only when the station offers QSL marks', async () => {
  const test = station(['logEdit', 'qslMarks'])
  await screen.findByText('W1AW')
  fireEvent.change(screen.getByRole('combobox', { name: t('logbook.row.qslSent.aria', { call: 'W1AW' }) }), { target: { value: 'R' } })
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change).toEqual({ kind: 'qslCard', target: await logTarget(qso('W1AW')), received: true })
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'fileSynced' }))
  await waitFor(() => expect(test.source.page).toHaveBeenCalledTimes(2))
  test.client.disconnected()
})

it('refreshes past a shared log page captured before its own change', async () => {
  const test = station(['logEdit'])
  await screen.findByText('W1AW')
  // The station shares a recent page-zero capture between readers for a few seconds. One taken
  // before this delete still lists the contact; the view must not settle on it.
  test.source.page.mockResolvedValueOnce({ ...page(['W1AW', 'K1ABC']), ageMs: 60_000 }).mockResolvedValueOnce(page(['K1ABC']))
  fireEvent.click(screen.getByRole('button', { name: t('logbook.row.delete', { call: 'W1AW' }) }))
  fireEvent.click(await screen.findByRole('button', { name: t('logbook.delete.confirm') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'fileSynced' }))
  // Both at once: while a page loads the table is empty, so "W1AW is gone" alone proves nothing.
  await waitFor(() => {
    expect(screen.getByText('K1ABC')).toBeTruthy()
    expect(screen.queryByText('W1AW')).toBeNull()
  }, { timeout: 4000 })
  expect(test.source.page).toHaveBeenCalledTimes(3)
  test.client.disconnected()
})

it('edits a remote row through the log form without the fields the station keeps', async () => {
  const test = station(['logEdit'])
  await screen.findByText('W1AW')
  fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'W1AW' }) }))
  expect(screen.queryByText(t('logbook.field.txPower.label'))).toBeNull()
  expect(screen.queryByText(t('logbook.field.parkMine.label'))).toBeNull()
  // Nor the four the record cannot carry: the form used to show them, take the operator's tick,
  // and drop it under an "updated" toast. (The shack's form shows all four: Logbook.fields.test.)
  expect(screen.queryByText(t('logbook.field.myGrid.label'))).toBeNull()
  expect(screen.queryByText(t('logbook.field.myRig.label'))).toBeNull()
  expect(screen.queryByText(t('logbook.field.qslSent.label'))).toBeNull()
  expect(screen.queryByText(t('logbook.field.qslCard.label'))).toBeNull()
  fireEvent.change(screen.getByDisplayValue('FN31'), { target: { value: 'FN42' } })
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change.target).toEqual(await logTarget(qso('W1AW')))
  expect(request.change.record).toMatchObject({ call: 'W1AW', grid: 'FN42', band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', whenUnix: 1788940800, confirmed: false, awardConfirmed: false })
  // A station that refuses the stale row changes nothing here, and says why.
  await act(async () => test.reply({ operation: 'logChange', operationId: request.requestId, outcome: 'rejected', reason: 'contextChanged' }))
  expect(test.source.page).toHaveBeenCalledTimes(1)
  expect(screen.getByRole('button', { name: t('logbook.form.save') })).toBeTruthy()
  test.client.disconnected()
})

it('keeps a WWFF park as WWFF through an edit of an unrelated field', async () => {
  // WWFF is a real stored program (an ADIF SIG kept verbatim); the form used to narrow it to
  // SOTA-or-POTA, so a grid fix from the browser rewrote the park to POTA.
  const wwff = { ...qso('DL1ABC'), ota: { theirProgram: 'WWFF', theirRef: 'DLFF-0001', myProgram: null, myRef: null, iota: null } }
  const test = station(['logEdit'], [wwff])
  await screen.findByText('DL1ABC')
  fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'DL1ABC' }) }))
  fireEvent.change(screen.getByDisplayValue('FN31'), { target: { value: 'JO31' } })
  fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
  await waitFor(() => expect(test.changes()).toHaveLength(1))
  const [request] = test.changes()
  expect(request.change.record.ota).toEqual({ theirProgram: 'WWFF', theirRef: 'DLFF-0001' })
  expect(request.change.record.grid).toBe('JO31')
  test.client.disconnected()
})

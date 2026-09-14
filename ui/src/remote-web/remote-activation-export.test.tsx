// @vitest-environment jsdom
import { afterEach, beforeAll, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createHash } from 'node:crypto'
import { Logbook } from '../components/Logbook'
import { ConfirmHost } from '../confirm'
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

/** 2026-09-09 00:00:00 UTC. */
const DAY = 1788912000
const activation = { program: 'POTA', reference: 'US-1234', dayStartUnix: DAY, date: '2026-09-09', callsign: 'W9XYZ/P', qsos: 12 }
const ADIF = '<CALL:5>K1AAA<MY_SIG_INFO:7>US-1234<EOR>\n'
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), collection: 'log',
  snapshotId: crypto.randomUUID(), offset: 0, total: 1, retained: 1, nextCursor: null, ageMs: 0,
  rows: [{ call: 'K1AAA', band: '20m', mode: 'FT8', freqMhz: 14.074, whenUnix: DAY + 3600, confirmed: false, awardConfirmed: false,
    grid: 'FN31', country: 'United States', rstSent: '-10', rstRcvd: '-12' }], meta: {} })

function station(capabilities: string[]) {
  const sent: Record<string, any>[] = []
  const bytes = Buffer.from(ADIF)
  const client: OperationClient = new OperationClient(s => {
    const message = JSON.parse(s)
    sent.push(message)
    if (message.request.type !== 'activationExport') return
    const value = message.request.selection === null ? { operation: 'activationExport', activations: [activation] }
      : { operation: 'activationExport', index: 0, base64: bytes.toString('base64'),
        file: { byteLength: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), chunks: 1 } }
    queueMicrotask(() => client.receive({ type: 'operationResponse', requestId: message.request.requestId, value }))
  }, true, () => 1000, { read: () => null, write: () => {} }, 4)
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0]!.request.requestId, value: { stationBootId: crypto.randomUUID(),
    allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1, commandWindowId: crypto.randomUUID(), nextSequence: 1,
    leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false, transmitEpoch: null,
    controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } } })
  const source = { page: vi.fn(async () => page()) }
  render(<StationControlContext.Provider value={false}><RemoteOperationsContext.Provider value={client}>
    <RemoteCollectionsContext.Provider value={source as unknown as RemoteCollections}>
      <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />
    </RemoteCollectionsContext.Provider><ConfirmHost /></RemoteOperationsContext.Provider></StationControlContext.Provider>)
  return { client, exports: () => sent.filter(m => m.request.type === 'activationExport').map(m => m.request) }
}

it('downloads one activation from the remote Logbook under the name the desktop export gives it', async () => {
  const blobs: Blob[] = [], names: string[] = []
  URL.createObjectURL = vi.fn((blob: Blob) => { blobs.push(blob); return 'blob:activation' }) as typeof URL.createObjectURL
  URL.revokeObjectURL = vi.fn()
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) { names.push(this.download) })
  const test = station(['qsoLogging', 'activationExport'])
  const picker = await screen.findByRole('combobox', { name: t('logbook.export.activation.label') }) as HTMLSelectElement
  const download = screen.getByRole('button', { name: t('logbook.export.activation.button') }) as HTMLButtonElement
  expect(download.disabled).toBe(true)
  fireEvent.change(picker, { target: { value: picker.options[1]!.value } })
  fireEvent.click(download)
  // The portable call cannot be a filename character, exactly as at the desktop; the ADIF keeps it.
  await waitFor(() => expect(names).toEqual(['W9XYZ-P@US-1234-20260909.adi']))
  expect(blobs[0]!.size).toBe(ADIF.length)
  // The list may be read again at any time the log grows, as at the desktop, so reads are matched by
  // what they ask for, not by order: exactly one file read, naming that activation; the rest are lists.
  const reads = test.exports(), files = reads.filter(r => r.selection !== null), lists = reads.filter(r => r.selection === null)
  expect(files.map(r => r.selection)).toEqual([{ reference: 'US-1234', dayStartUnix: DAY, callsign: 'W9XYZ/P' }])
  expect(lists.length).toBeGreaterThan(0)
  expect(lists.every(r => r.index === 0)).toBe(true)
  test.client.disconnected()
})

it('offers no activation download when the station does not offer one, and never asks it', async () => {
  const test = station(['qsoLogging', 'logEdit'])
  await screen.findByText('K1AAA')
  expect(screen.queryByRole('button', { name: t('logbook.export.activation.button') })).toBeNull()
  expect(test.exports()).toEqual([])
  test.client.disconnected()
})

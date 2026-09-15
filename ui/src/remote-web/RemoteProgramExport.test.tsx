// @vitest-environment jsdom
// The Program section's CHIRP/CSV export from a browser. The station renders the file with the
// desktop's own two writers and this page saves it to THIS machine's downloads — the channel rows
// were already on screen, the writer never was.
//
// ⚠️ jsdom cannot lay anything out, so nothing here is a claim about where these buttons sit; it
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
import { EXPORT_CHUNK_BYTES, type OperationState } from './operation-protocol'
import type { ApplicationClient } from './application-client'
import { navigationPages } from './__fixtures__/navigation-page'
import configurationProgramming from './__fixtures__/configuration-programming.json'
import { t } from '../i18n'

const toasts = vi.hoisted(() => [] as [string, string][])
vi.mock('../toast', async (actual) => ({ ...(await actual<typeof import('../toast')>()), pushToast: (text: string, kind: string) => { toasts.push([text, kind]) } }))

let dispose: (() => void) | undefined
const saved: { name: string; text: string }[] = []
afterEach(() => { cleanup(); dispose?.(); dispose = undefined; toasts.length = 0; saved.length = 0; vi.restoreAllMocks(); localStorage.clear() })

const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, '0')).join('')

/** jsdom has no object URLs and no real downloads: record what the anchor was handed instead. */
function captureDownloads() {
  const blobs = new Map<string, Blob>()
  vi.spyOn(URL, 'createObjectURL').mockImplementation((blob: Blob | MediaSource) => {
    const url = `blob:${crypto.randomUUID()}`
    blobs.set(url, blob as Blob)
    return url
  })
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
    const blob = blobs.get(this.href)
    if (blob) void blob.text().then(text => { saved.push({ name: this.download, text }) })
  })
}

type Station = (request: Record<string, any>) => unknown
function operations(capabilities: string[], station?: Station) {
  const values = new Map<string, string>(), sent: Record<string, any>[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const state = (): OperationState => ({ stationBootId: boot, allowed: true, phase: 'controlling', leaseId: lease, revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } as OperationState['controls'] })
  const boot = crypto.randomUUID(), lease = crypto.randomUUID()
  const client: OperationClient = new OperationClient(s => {
    const message = JSON.parse(s)
    sent.push(message)
    const answer = message.request.type === 'heartbeat' ? () => state()
      : message.request.type === 'programExport' && station ? () => station(message.request) : null
    if (answer) queueMicrotask(() => {
      const value = answer()
      client.receive({ type: 'operationResponse', requestId: message.request.requestId, ...(typeof value === 'string' ? { error: value } : { value }) })
    })
  }, true, () => 1000, pendingLogStorage(() => data, 'station', lock), 4, pendingControlStorage(() => data, 'station', lock))
  client.open()
  client.receive({ type: 'operationResponse', requestId: sent[0]!.request.requestId, value: state() })
  return { client, requests: () => sent.filter(m => m.request.type === 'programExport').map(m => m.request) }
}

function program(capabilities: string[], station?: Station) {
  captureDownloads()
  const invoke = vi.fn(async () => { throw new Error('applicationUnsupported') })
  dispose = installApplicationTransport({ kind: 'remote', invoke: invoke as unknown as ApplicationTransport['invoke'] })
  const source = new RemoteCollections({ supports: () => false, invoke } as unknown as ApplicationClient)
  const pages = navigationPages('programming', configurationProgramming)
  vi.spyOn(source, 'page').mockImplementation(async args => pages[args.cursor ? Number(args.cursor.split(':')[1]) : 0])
  const ops = operations(capabilities, station)
  const view = render(<StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={ops.client}>
      <RemoteCollectionsContext.Provider value={source}><RadioProgView myGrid="FN31" catOk={true} /></RemoteCollectionsContext.Provider>
    </RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)
  return { ...view, invoke, ...ops }
}

const buttons = async (container: HTMLElement) => {
  await waitFor(() => expect(container.querySelector('.rp-chan-row')).toBeTruthy(), { timeout: 4000 })
  const all = [...container.querySelectorAll<HTMLButtonElement>('.rp-deliver button')]
  return {
    chirp: container.querySelector<HTMLButtonElement>('.rp-export-chirp')!,
    csv: all.find(b => b.title === t('program.deliver.exportCsv.title'))!,
    cap: container.querySelector<HTMLSelectElement>('.rp-cap select')!
  }
}

/** A station that renders `text` and hands it over in whole chunks. */
const renderer = async (text: string): Promise<Station> => {
  const bytes = new TextEncoder().encode(text)
  const sha256 = hex(await crypto.subtle.digest('SHA-256', bytes))
  return r => ({
    operation: 'programExport', index: r.index,
    file: { byteLength: bytes.length, sha256, chunks: Math.ceil(bytes.length / EXPORT_CHUNK_BYTES) },
    base64: Buffer.from(bytes.subarray(r.index * EXPORT_CHUNK_BYTES, (r.index + 1) * EXPORT_CHUNK_BYTES)).toString('base64')
  })
}

it('saves the station-rendered channel file to this browser, carrying the format and rig cap', async () => {
  const text = 'Location,Name,Frequency\n1,CH0000,146.000000\n'
  const { container, requests } = program(['programExport'], await renderer(text))
  const { csv, cap } = await buttons(container)
  expect(csv.disabled).toBe(false)
  // The rig name cap belongs to the export, so a browser that may export may choose it.
  expect(cap.disabled).toBe(false)
  fireEvent.change(cap, { target: { value: '12' } })
  fireEvent.click(csv)
  await waitFor(() => expect(saved.length).toBe(1))
  expect(saved[0]!.text).toBe(text)
  expect(saved[0]!.name).toMatch(/^nexus-channels-\d{4}-\d{2}-\d{2}\.csv$/)
  expect(requests().map(r => [r.format, r.nameCap])).toEqual([['csv', 12]])
  await waitFor(() => expect(toasts.some(([, kind]) => kind === 'success')).toBe(true))
})

it('leaves the export dead, and asks nothing, without the station hint', async () => {
  const { container, requests } = program(['repeaterTuning', 'frequency'], await renderer('Location,Name\n'))
  const { chirp, csv, cap } = await buttons(container)
  expect([chirp.disabled, csv.disabled, cap.disabled]).toEqual([true, true, true])
  fireEvent.click(csv)
  fireEvent.click(chirp)
  await new Promise(resolve => setTimeout(resolve, 20))
  expect(requests()).toEqual([])
  expect(saved).toEqual([])
})

it('says which refusal happened, and saves nothing', async () => {
  for (const [refused, message] of [['notFound', t('program.export.browser.empty')],
    ['tooLarge', t('program.export.browser.tooLarge')]] as const) {
    const { container } = program(['programExport'], () => ({ operation: 'programExport', refused }))
    const { csv } = await buttons(container)
    fireEvent.click(csv)
    await waitFor(() => expect(toasts).toContainEqual([message, 'error']))
    expect(saved).toEqual([])
    cleanup(); dispose?.(); dispose = undefined; toasts.length = 0; vi.restoreAllMocks()
  }
})

it('saves nothing when the file does not arrive intact', async () => {
  const bytes = new TextEncoder().encode('Location,Name\n1,CH0000\n')
  const { container } = program(['programExport'], r => ({
    operation: 'programExport', index: r.index,
    file: { byteLength: bytes.length, sha256: 'f'.repeat(64), chunks: 1 },
    base64: Buffer.from(bytes).toString('base64')
  }))
  const { csv } = await buttons(container)
  fireEvent.click(csv)
  await waitFor(() => expect(toasts).toContainEqual([t('program.export.browser.failed'), 'error']))
  expect(saved).toEqual([])
})

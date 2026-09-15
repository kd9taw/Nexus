// @vitest-environment jsdom
// Spotting ANOTHER station from a browser. The spot posts publicly from the station's own cluster
// login, so it needs STATION CONTROL rather than the logging grant, it is confirmed on every
// press, and no press ever reaches `post_spot` in the browser's own process.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'
import * as api from '../api'
import * as confirm from '../confirm'
import { SpotDialog } from '../components/SpotDialog'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from './operation-client'
import { pendingControlStorage } from './control-storage'
import { pendingLogStorage, type ReceiptLock } from './operation-storage'
import type { OperationState } from './operation-protocol'
import type { LogCapability } from './operation-protocol'
import { t } from '../i18n'

vi.mock('../api', () => ({ postSpot: vi.fn(async () => {}) }))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

function operations(capabilities: LogCapability[]) {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient(s => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 4, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  const changes = () => sent.map(s => JSON.parse(s)).filter(w => w.request?.type === 'logChange')
  return { client, changes }
}

const browser = (ops: ReturnType<typeof operations>, children: ReactNode) => render(
  <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
    <RemoteOperationsContext.Provider value={ops.client}>{children}</RemoteOperationsContext.Provider>
  </StationDataContext.Provider></StationControlContext.Provider>)

const dialog = () => <SpotDialog open onClose={() => {}} initialCall="JA2DEF/P" freqMhz={14.0765} defaultComment="FT8" />
const send = () => screen.getByText(t('spots.post.submit')) as HTMLButtonElement
const settle = () => act(async () => { for (let i = 0; i < 12; i++) await Promise.resolve() })

it('confirms every press, then sends exactly one spot change and never posts from the browser', async () => {
  const ops = operations(['postSpot'])
  browser(ops, dialog())
  await settle()
  expect(send().disabled).toBe(false)
  fireEvent.click(send())
  await settle()
  expect(confirm.confirmDialog).toHaveBeenCalledTimes(1)
  // The question names exactly what goes out.
  expect(JSON.stringify(vi.mocked(confirm.confirmDialog).mock.calls[0][0])).toContain('JA2DEF/P')
  const changes = ops.changes()
  expect(changes).toHaveLength(1)
  // The dialog seeds the dial to the kHz, as it does on the desktop, and what it SHOWS is what
  // is sent — the browser never substitutes a number the operator did not read.
  expect(changes[0].request.change).toEqual({ kind: 'spot', call: 'JA2DEF/P', freqMhz: 14.076, comment: 'FT8' })
  // A browser never reaches the station's own Tauri command.
  expect(api.postSpot).not.toHaveBeenCalled()
})

it('sends nothing when the operator says no to the confirm', async () => {
  vi.mocked(confirm.confirmDialog).mockResolvedValueOnce(false)
  const ops = operations(['postSpot'])
  browser(ops, dialog())
  await settle()
  fireEvent.click(send())
  await settle()
  expect(confirm.confirmDialog).toHaveBeenCalledTimes(1)
  expect(ops.changes()).toHaveLength(0)
  expect(api.postSpot).not.toHaveBeenCalled()
})

it('is dead for a browser without station control, and the logging grant is not a substitute', async () => {
  // Every other log capability a station can offer, and not this one.
  const ops = operations(['logEdit', 'qslMarks', 'otaHunt', 'otaActivation', 'selfSpot', 'activationExport', 'settingsLogging'])
  browser(ops, dialog())
  await settle()
  expect(send().disabled).toBe(true)
  fireEvent.click(send())
  await settle()
  expect(confirm.confirmDialog).not.toHaveBeenCalled()
  expect(ops.changes()).toHaveLength(0)
  expect(api.postSpot).not.toHaveBeenCalled()
  cleanup()
  // Positive control: the same dialog with the hint is live and sends.
  const allowed = operations(['postSpot'])
  browser(allowed, dialog())
  await settle()
  expect(send().disabled).toBe(false)
  fireEvent.click(send())
  await settle()
  expect(allowed.changes()).toHaveLength(1)
})

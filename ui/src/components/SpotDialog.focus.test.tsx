// @vitest-environment jsdom
//
// THE SPOT DIALOG GIVES THE KEYBOARD BACK TO WHERE IT WAS OPENED FROM — cancelled, escaped, posted,
// and posted from a browser, which asks first (confirmDialog): that question's own control is the
// dialog's Post button, gone with the dialog, so the question cannot return the keyboard there and
// the dialog must. It used to be left on the page itself.

import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { SpotDialog } from './SpotDialog'
import { ConfirmHost } from '../confirm'
import { postSpot } from '../api'
import { RemoteOperationsContext, StationControlContext, StationDataContext } from '../stationAccess'
import { OperationClient } from '../remote-web/operation-client'
import { pendingControlStorage } from '../remote-web/control-storage'
import { pendingLogStorage, type ReceiptLock } from '../remote-web/operation-storage'
import type { OperationState } from '../remote-web/operation-protocol'
import { t } from '../i18n'

vi.mock('../api', () => ({ postSpot: vi.fn(async () => {}) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => vi.clearAllMocks())
afterEach(() => cleanup())

function Cockpit() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Spot this station
      </button>
      <SpotDialog open={open} onClose={() => setOpen(false)} initialCall="JA2DEF" freqMhz={14.074} defaultComment="" />
      <ConfirmHost />
    </>
  )
}
async function openFromSpot() {
  const spot = screen.getByRole('button', { name: 'Spot this station' })
  act(() => spot.focus())
  fireEvent.click(spot)
  await screen.findByRole('dialog', { name: t('spots.post.aria') })
  expect(document.activeElement?.tagName, 'the call box has the keyboard').toBe('INPUT')
  return spot
}

it('cancelled: back to Spot', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  fireEvent.click(screen.getByRole('button', { name: t('spots.post.cancel') }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
})

it('escaped: back to Spot', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
})

it('posted: back to Spot', async () => {
  render(<Cockpit />)
  const spot = await openFromSpot()
  fireEvent.click(screen.getByRole('button', { name: t('spots.post.submit') }))
  await waitFor(() => expect(postSpot).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  expect(document.activeElement).toBe(spot)
})

it('posted from a browser, after its question: back to Spot, past the question', async () => {
  const values = new Map<string, string>(), sent: string[] = []
  const data = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const lock: ReceiptLock = async (_k, run) => run()
  const client = new OperationClient((s) => sent.push(s), true, () => 1000, pendingLogStorage(() => data, 'station', lock), 4, pendingControlStorage(() => data, 'station', lock))
  client.open()
  const state: OperationState = { stationBootId: crypto.randomUUID(), allowed: true, phase: 'controlling', leaseId: crypto.randomUUID(), revision: 1,
    commandWindowId: crypto.randomUUID(), nextSequence: 1, leaseRemainingMs: 5000, actions: [], txArmed: false,
    controls: { context: { radioId: 1, radioConnection: 1, ampConnection: null, ampReadSequence: null }, capabilities: ['postSpot'] } }
  client.receive({ type: 'operationResponse', requestId: JSON.parse(sent[sent.length - 1]).request.requestId, value: state })
  render(
    <StationControlContext.Provider value={false}><StationDataContext.Provider value={true}>
      <RemoteOperationsContext.Provider value={client}><Cockpit /></RemoteOperationsContext.Provider>
    </StationDataContext.Provider></StationControlContext.Provider>,
  )
  const spot = await openFromSpot()
  const post = screen.getByRole('button', { name: t('spots.post.submit') })
  await waitFor(() => expect((post as HTMLButtonElement).disabled).toBe(false))
  act(() => post.focus())
  fireEvent.click(post)
  fireEvent.click(await screen.findByRole('button', { name: t('spots.post.confirm.post') }))
  // The station takes the spot.
  await waitFor(() => expect(sent.map((s) => JSON.parse(s)).some((w) => w.request?.type === 'logChange')).toBe(true))
  const changes = sent.map((s) => JSON.parse(s)).filter((w) => w.request?.type === 'logChange')
  const change = changes[changes.length - 1]
  await act(async () => client.receive({ type: 'operationResponse', requestId: change.request.requestId,
    value: { operation: 'logChange', operationId: change.request.requestId, outcome: 'applied', evidence: 'clusterQueued' } }))
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  await waitFor(() => expect(document.activeElement).toBe(spot))
  client.disconnected()
})

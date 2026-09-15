// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot } from '../types'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'

// The desktop's own self-spot: nothing here reaches the backend, pota.app or a cluster. `selfSpot`
// is the one door to both, and it is a mock.
const api = vi.hoisted(() => ({
  selfSpot: vi.fn(),
  getOtaSpots: vi.fn(async () => []),
  getActivation: vi.fn(async () => ({ program: 'POTA', reference: 'US-0001', qsoCount: 3 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
}))
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: toast.pushToast }))

import { PotaSotaView } from './PotaSotaView'

const snap = { hunt: null, radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
beforeEach(() => { api.selfSpot.mockReset(); toast.pushToast.mockReset() })
afterEach(() => { cleanup(); localStorage.clear() })

async function view() {
  render(<><PotaSotaView snap={snap} /><ConfirmHost /></>)
  return screen.findByRole('button', { name: t('ota.selfSpot.button') })
}

it('posts nothing until the operator presses Spot me and confirms, then posts once', async () => {
  api.selfSpot.mockResolvedValue({ pota: 'posted', cluster: 'queued' })
  const button = await view()
  expect(api.selfSpot).not.toHaveBeenCalled()
  fireEvent.click(button)
  expect(await screen.findByText(t('ota.selfSpot.confirm.body', { reference: 'US-0001', freq: '14.2850' }))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
  await waitFor(() => expect(screen.queryByRole('button', { name: t('ota.selfSpot.confirm.post') })).toBeNull())
  expect(api.selfSpot).not.toHaveBeenCalled()
  // A second press asks again; the earlier answer is never remembered.
  fireEvent.click(button)
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(api.selfSpot).toHaveBeenCalledTimes(1))
  expect(api.selfSpot).toHaveBeenCalledWith('US-0001', 14_285_000)
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.both'), 'success'))
})

it('reports each target on its own when they differ, so one failure never hides the other', async () => {
  api.selfSpot.mockResolvedValue({ pota: 'loginRequired', cluster: 'queued' })
  fireEvent.click(await view())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledTimes(2))
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.cluster.queued'), 'success')
  expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.pota.loginRequired'), 'error', 8000)
  expect(toast.pushToast).not.toHaveBeenCalledWith(t('ota.selfSpot.both'), 'success')
})

it('says so when the park or dial moved after the press', async () => {
  api.selfSpot.mockRejectedValue('contextChanged')
  fireEvent.click(await view())
  fireEvent.click(await screen.findByRole('button', { name: t('ota.selfSpot.confirm.post') }))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.selfSpot.moved'), 'error', 6000))
})

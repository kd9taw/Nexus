// @vitest-environment jsdom
//
// Stop pressed while a run or contact is under way cannot take the station back from the park's
// "My station" profile. Its answer names the profile to go back to, and the panel says once, as a
// toast, to pick it in the top bar. A Stop that went back (or a station with no profiles) says only
// that the activation ended.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AppSnapshot } from '../types'
import { ConfirmHost } from '../confirm'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  selfSpot: vi.fn(),
  clearActivation: vi.fn(),
  getOtaSpots: vi.fn(async () => []),
  getActivation: vi.fn(async () => ({ program: 'POTA', reference: 'US-0001', qsoCount: 3 })),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
  logActivations: vi.fn(async () => []),
}))
vi.mock('../api', () => ({
  ...api,
  clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
}))
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: toast.pushToast }))

import { PotaSotaView } from './PotaSotaView'

vi.setConfig({ testTimeout: 15_000 })

const snap = { hunt: null, radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
const ended = { program: null, reference: null, qsoCount: 0 }

beforeEach(() => {
  toast.pushToast.mockReset()
  api.clearActivation.mockReset()
})
afterEach(() => { cleanup(); localStorage.clear() })

async function stopAndEnd() {
  render(<><PotaSotaView snap={snap} /><ConfirmHost /></>)
  fireEvent.click(await screen.findByRole('button', { name: new RegExp(t('ota.activation.stop.label')) }))
  await screen.findByRole('dialog', { name: t('activationOver.title') })
  fireEvent.click(screen.getByRole('button', { name: t('activationOver.end') }))
  await waitFor(() => expect(api.clearActivation).toHaveBeenCalledTimes(1))
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(t('ota.activation.ended'), 'info', 2000))
}

it('a Stop that could not go back says which profile to pick in the top bar, once', async () => {
  api.clearActivation.mockResolvedValue({ ...ended, stationProfileBack: { id: 'home', name: '', call: 'KD9TAW' } })
  await stopAndEnd()
  const said = t('ota.activation.profileStays', { profile: t('settings.stationProfiles.home'), call: 'KD9TAW' })
  await waitFor(() => expect(toast.pushToast).toHaveBeenCalledWith(said, 'info'))
  expect(toast.pushToast.mock.calls.filter((c) => c[0] === said)).toHaveLength(1)
  expect(said, 'the words name the profile and its call').toContain('KD9TAW')
})

it('a Stop that went back, or a station with no profiles, says only that it ended', async () => {
  api.clearActivation.mockResolvedValue(ended)
  await stopAndEnd()
  expect(toast.pushToast.mock.calls).toEqual([[t('ota.activation.ended'), 'info', 2000]])
})

// @vitest-environment jsdom
//
// The state you are activating from, carried by your contacts as ADIF MY_STATE: the park's own
// when it names one, and on a state line the one you pick here, never a guess. Nothing here reaches
// the backend: the api is a mock.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { Activation, AppSnapshot } from '../types'
import { t } from '../i18n'

const api = vi.hoisted(() => ({
  getOtaSpots: vi.fn(async () => []),
  getActivation: vi.fn(),
  setActivationState: vi.fn(),
  parksCount: vi.fn(async () => 0),
  huntedParksCount: vi.fn(async () => 0),
}))
vi.mock('../api', () => ({
  ...api,
  selfSpot: vi.fn(), clearHuntTarget: vi.fn(), openPanelWindow: vi.fn(), setHuntTarget: vi.fn(), setActivation: vi.fn(),
  clearActivation: vi.fn(), downloadParks: vi.fn(), importParksCsv: vi.fn(), importHuntedParksCsv: vi.fn(),
}))
vi.mock('../toast', async (original) => ({ ...(await original<typeof import('../toast')>()), pushToast: vi.fn() }))

import { PotaSotaView } from './PotaSotaView'

const snap = { hunt: null, radio: { dialMhz: 14.285 } } as unknown as AppSnapshot
const activation = (over: Partial<Activation>): Activation =>
  ({ program: 'POTA', reference: 'US-0003', qsoCount: 0, states: ['US-MT', 'US-ND'], myState: null, ...over })

beforeEach(() => {
  api.getActivation.mockReset()
  api.setActivationState.mockReset()
})
afterEach(() => { cleanup(); localStorage.clear() })

it('asks which state on a state line, and shows the one picked', async () => {
  api.getActivation.mockResolvedValue(activation({}))
  api.setActivationState.mockResolvedValue(activation({ myState: 'ND' }))
  render(<PotaSotaView snap={snap} />)
  const ask = await screen.findByRole('group', { name: t('ota.activation.state.ask') })
  expect(screen.queryByText(t('ota.activation.state.mine', { state: 'MT' }))).toBeNull()
  fireEvent.click(within(ask).getByRole('button', { name: 'ND' }))
  await waitFor(() => expect(api.setActivationState).toHaveBeenCalledWith('ND'))
  expect(await screen.findByText(t('ota.activation.state.mine', { state: 'ND' }))).toBeTruthy()
  expect(screen.queryByRole('group', { name: t('ota.activation.state.ask') })).toBeNull()
})

it('shows a park’s own state with what LoTW needs for it, and asks nothing', async () => {
  api.getActivation.mockResolvedValue(activation({ reference: 'US-0001', states: ['US-ND'], myState: 'ND' }))
  render(<PotaSotaView snap={snap} />)
  const mine = await screen.findByText(t('ota.activation.state.mine', { state: 'ND' }))
  expect(mine.getAttribute('title')).toBe(t('ota.activation.state.lotw'))
  expect(screen.queryByRole('group', { name: t('ota.activation.state.ask') })).toBeNull()
})

it('says nothing of a state for a park nothing placed', async () => {
  api.getActivation.mockResolvedValue(activation({ reference: 'DE-0001', states: [], myState: null }))
  render(<PotaSotaView snap={snap} />)
  await screen.findByText((_, el) => el?.classList.contains('pota-act-text') ?? false)
  expect(screen.queryByRole('group', { name: t('ota.activation.state.ask') })).toBeNull()
  expect(document.querySelector('.pota-act-state')).toBeNull()
})

// @vitest-environment jsdom
import { afterEach, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { RemoteStationAction, RemoteStationStatus } from './types'

afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__ })
it('renders local pairing and device approval through only the isolated Remote commands', async () => {
  const accountId = crypto.randomUUID(), pairingId = crypto.randomUUID(), deviceId = crypto.randomUUID()
  let status: RemoteStationStatus = { phase: 'approval', origin: 'https://remote-staging.hamradiotools.io',
    stationId: null, accountId, pairingId, pairingCode: crypto.randomUUID().replace(/-/g,'').slice(0,16),
    expiresAt: Date.now()+600000, devices: [], error: null }
  const actions: RemoteStationAction[] = []
  const invoke = async (command: string, input: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    actions.push(action)
    if (action.type === 'approve') status = { ...status, stationId: pairingId, pairingCode: null, phase: 'disabled',
      devices: [{ id: deviceId, name: 'Test browser', approved: 0, expiresAt: Date.now()+600000 }] }
    if (action.type === 'enable') status = { ...status, phase: 'connected' }
    if (action.type === 'disable') status = { ...status, phase: 'disabled' }
    return status
  }
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<form><RemoteStation /></form>)
  await screen.findByText(accountId)
  fireEvent.click(screen.getByRole('button', { name: 'Approve this account pairing' }))
  await screen.findByText(deviceId.slice(-6))
  fireEvent.click(screen.getByRole('button', { name: 'Approve browser' }))
  await waitFor(() => expect((screen.getByRole('button', { name: 'Enable Remote observation' }) as HTMLButtonElement).disabled).toBe(false))
  fireEvent.click(screen.getByRole('button', { name: 'Enable Remote observation' }))
  await screen.findByRole('button', { name: 'Disable Remote observation' })
  fireEvent.click(screen.getByRole('button', { name: 'Disable Remote observation' }))
  await screen.findByRole('button', { name: 'Enable Remote observation' })
  expect(actions).toContainEqual({ type: 'approve', accountId, enrollmentId: pairingId })
  expect(actions).toContainEqual({ type: 'device', deviceId, approve: true })
  expect(actions).toContainEqual({ type: 'disable' })
  expect([...document.querySelectorAll('button')].every(button => button.type === 'button')).toBe(true)
})

// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'

afterEach(() => { cleanup(); vi.restoreAllMocks() })
function account(entitled = true): AccountSession {
  const accountId = crypto.randomUUID()
  return { accountId, entitlement: { accountId, enabled: entitled, expiresAt: Date.now()+3600000 }, stations: [] }
}
function client(value: AccountSession | null) {
  const post = vi.fn(async (_path: string, _body?: object): Promise<unknown> => value)
  const service = { post, authenticated: async () => !!value, signIn: vi.fn(async () => {}), signOut: vi.fn(async () => {}) }
  vi.spyOn(BrowserClient, 'load').mockResolvedValue(service as unknown as BrowserClient)
  return service
}
it('offers a working retry after configuration failure instead of a sign-in button with no client', async () => {
  const service = client(null)
  vi.mocked(BrowserClient.load).mockRejectedValueOnce(new Error('offline'))
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
  const signIn = await screen.findByRole('button', { name: 'Sign in or create an account' })
  fireEvent.click(signIn)
  await waitFor(() => expect(service.signIn).toHaveBeenCalledTimes(1))
  expect(document.querySelectorAll('main.rm-scroll')).toHaveLength(1)
  expect(document.querySelectorAll('.remote-monitor-app')).toHaveLength(1)
})
it('a signed-in account without a manual trial cannot start station pairing', async () => {
  client(account(false)); render(<RemoteApp />)
  await screen.findByText(/trial/i, { selector: '[role="status"]' })
  expect(screen.queryByRole('textbox')).toBeNull()
  expect(screen.queryByRole('button', { name: 'Observe station' })).toBeNull()
})
it('claims the typed pairing code and requires approval at the shack', async () => {
  const session = account(), service = client(session)
  render(<RemoteApp />)
  const input = await screen.findByLabelText('Pairing code')
  const code = crypto.randomUUID().replace(/-/g,'').slice(0,16)
  fireEvent.change(input, { target: { value: code.toUpperCase() } })
  fireEvent.submit(input.closest('form')!)
  await waitFor(() => expect(service.post).toHaveBeenCalledWith('pair/claim', { code }))
  expect(screen.getByText(/approve.*shack|shack.*approve/i)).toBeTruthy()
  expect(screen.queryByRole('button', { name: 'Observe station' })).toBeNull()
})
it('browser enrollment displays the local comparison code and does not imply approval', async () => {
  const session = account(), stationId = crypto.randomUUID(), deviceId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Synthetic station', device: null })
  const service = client(session)
  service.post.mockImplementation(async path => {
    if (path.endsWith('/device')) session.stations[0].device = { id: deviceId, name: 'Test browser', approved: 0 }
    return { ...session, stations: session.stations.map(station => ({ ...station })) }
  })
  render(<RemoteApp />)
  const input = await screen.findByLabelText('Name this browser')
  fireEvent.change(input, { target: { value: 'Test browser' } })
  fireEvent.submit(input.closest('form')!)
  await screen.findByText(deviceId.slice(-6))
  expect(screen.queryByRole('button', { name: 'Observe station' })).toBeNull()
  expect(service.post).toHaveBeenCalledWith(`stations/${stationId}/device`, { name: 'Test browser' })
})

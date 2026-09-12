// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'

afterEach(() => { cleanup(); vi.restoreAllMocks() })
function account(entitled = true): AccountSession {
  const accountId = crypto.randomUUID()
  const now = Date.now()
  return { accountId, serverNow: now, stations: [],
    entitlement: { accountId, enabled: entitled, expiresAt: now + 3600000,
      state: entitled ? 'active' : 'none', startedAt: entitled ? now : null,
      source: entitled ? 'trial' : null } }
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
it('a signed-in account with no trial yet can start pairing, and is told the clock has not started', async () => {
  client(account(false)); render(<RemoteApp />)
  // Said in the service's own words, not inferred from enabled plus an expiry against our clock.
  await screen.findByText(/pair a station from the shack/i, { selector: '[role="status"]' })
  // The self-serve path: the pairing form has to be reachable by exactly the account that has
  // never had a trial. Gating it on entitlement made it unreachable for those people.
  expect(await screen.findByLabelText('Pairing code')).toBeTruthy()
  // Nothing is operable yet - no station is paired and no clock is running.
  expect(screen.queryByRole('button', { name: 'Observe station' })).toBeNull()
})
it('an ended trial and a disabled account read as different things, not one vague wall', async () => {
  const ended = account(); ended.entitlement.state = 'ended'
  client(ended); const view = render(<RemoteApp />)
  await screen.findByText(/fourteen days are over/i, { selector: '[role="status"]' })
  // The fortnight running out must never be mistaken for the account being switched off.
  expect(screen.queryByText(/switched off/i)).toBeNull()
  // ...and an ended trial cannot start another one.
  expect(screen.queryByLabelText('Pairing code')).toBeNull()
  view.unmount()

  const off = account(); off.entitlement.state = 'disabled'
  client(off); render(<RemoteApp />)
  await screen.findByText(/switched off/i, { selector: '[role="status"]' })
  expect(screen.queryByLabelText('Pairing code')).toBeNull()
})
it('a pilot account with no recorded start says so rather than inventing a date', async () => {
  const pilot = account(); pilot.entitlement.startedAt = null
  client(pilot); render(<RemoteApp />)
  await screen.findByText(/start date is not recorded/i, { selector: '[role="status"]' })
})
it('claims the typed pairing code and requires approval at the shack', async () => {
  const session = account(), service = client(session)
  render(<RemoteApp />)
  const input = await screen.findByLabelText('Pairing code')
  const code = crypto.randomUUID().replace(/-/g,'').slice(0,16)
  fireEvent.change(input, { target: { value: code.toUpperCase() } })
  fireEvent.submit(input.closest('form')!)
  await waitFor(() => expect(service.post).toHaveBeenCalledWith('pair/claim', { code }))
  expect(screen.getByText(/approve.*shack|shack.*approve/i, { selector: '[role="status"]' })).toBeTruthy()
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

// A first-time operator should not have to find a sign-up link on somebody else's login form.
it('offers creating an account as its own route, not a link to hunt for on the login form', async () => {
  const service = client(null)
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: 'Create an account' }))
  await waitFor(() => expect(service.signIn).toHaveBeenCalledWith(true))
  fireEvent.click(screen.getByRole('button', { name: 'Sign in or create an account' }))
  await waitFor(() => expect(service.signIn).toHaveBeenCalledTimes(2))
  expect(service.signIn).toHaveBeenLastCalledWith()
})

// The account id is a step only while something is waiting to be paired. Afterwards it is support
// detail; a raw UUID standing under "match this on both screens" reads as an unfinished action.
it('shows the account id as an instruction only while there is something to pair', async () => {
  const pairing = account(false)
  client(pairing); const view = render(<RemoteApp />)
  await screen.findByText(/match this account id/i)
  expect(screen.queryByText('Support details')).toBeNull()
  view.unmount()

  const paired = account()
  paired.stations = [
    { id: crypto.randomUUID(), name: 'Shack', device: null },
    { id: crypto.randomUUID(), name: 'Portable', device: null },
  ]
  client(paired); render(<RemoteApp />)
  await screen.findByText('Support details')
  // Still present for quoting when asking for help, just no longer presented as a step.
  expect(document.body.textContent).toContain(paired.accountId)
})

// @vitest-environment jsdom
// The Remote page and this browser's device key (A5). jsdom has no IndexedDB, so the key module
// keeps its keys in memory here; everything else is the real module, and the keys are made at run
// time. What the station does with the key is tested at the station.
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'
import { deviceKey } from './device-key'
import { shortFingerprint } from './stream-protocol'

vi.mock('./device-key', async importOriginal => {
  const actual = await importOriginal<typeof import('./device-key')>()
  const pairs = new Map<string, CryptoKeyPair>()
  const store = { get: async (id: string) => pairs.get(id),
    keep: async (id: string, pair: CryptoKeyPair) => { if (!pairs.has(id)) pairs.set(id, pair); return pairs.get(id)! } }
  return { ...actual, deviceKey: (stationId: string) => actual.deviceKey(stationId, store) }
})

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers() })
function account(): AccountSession {
  const accountId = crypto.randomUUID(), now = Date.now()
  return { accountId, serverNow: now, stations: [], pending: null,
    entitlement: { accountId, enabled: true, expiresAt: now + 3600000, state: 'active', startedAt: now, source: 'trial' } }
}
function client(session: AccountSession) {
  const post = vi.fn(async (_path: string, _body?: object): Promise<unknown> => structuredClone(session))
  const service = { post, authenticated: async () => true, signIn: vi.fn(async () => {}), signOut: vi.fn(async () => {}), signInRefusal: null, streamVersion: 1 }
  vi.spyOn(BrowserClient, 'load').mockResolvedValue(service as unknown as BrowserClient)
  return service
}
const devicePosts = (post: ReturnType<typeof client>['post']) => post.mock.calls.filter(([path]) => path.endsWith('/device'))

it('A5: asks for its device with its key, and shows the key while it waits, for the operator to compare', async () => {
  const session = account(), stationId = crypto.randomUUID(), deviceId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', device: null })
  const service = client(session)
  // As the service does: the device is made with the key it was sent, and lists it from then on.
  service.post.mockImplementation(async (path, body) => {
    if (path.endsWith('/device')) session.stations[0].device = { id: deviceId, name: 'Laptop', approved: 0,
      publicKey: (body as { publicKey?: string }).publicKey ?? null }
    return structuredClone(session)
  })
  render(<RemoteApp />)
  // Stream is the ask: the browser names itself, so there is nothing to type first.
  fireEvent.click(await screen.findByRole('button', { name: 'Stream' }))
  const key = (await deviceKey(stationId))!
  // The one request that makes the device carries the key: nothing has to be sent after it.
  await waitFor(() => expect(devicePosts(service.post)).toEqual([[`stations/${stationId}/device`, { name: 'Web browser', publicKey: key.publicKey }]]))
  // While it waits, the card says what the shack asks, with the same key, to compare.
  expect(await screen.findByText(new RegExp(`Approve it if it shows this key: ${shortFingerprint(key.fingerprint)}\\.$`))).toBeTruthy()
  expect(devicePosts(service.post), 'no second request').toHaveLength(1)
})

it('A5: confirms a pairing with the key of the browser the approval will approve', async () => {
  const session = account(), id = crypto.randomUUID()
  session.pending = { id, name: 'Home', expiresAt: session.serverNow + 9 * 60000, confirmed: false }
  const service = client(session)
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: /attach this station/i }))
  const key = (await deviceKey(id))!
  await waitFor(() => expect(service.post).toHaveBeenCalledWith('pair/confirm', { id, publicKey: key.publicKey }))
})

it('A5: a browser approved before keys sends its key once, never asks for anything else, and shows it', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const session = account(), stationId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', device: { id: crypto.randomUUID(), name: 'Laptop', approved: 1 } })
  const service = client(session)
  render(<RemoteApp />)
  const key = (await deviceKey(stationId))!
  await waitFor(() => expect(devicePosts(service.post)).toEqual([[`stations/${stationId}/device`, { name: 'Laptop', publicKey: key.publicKey }]]))
  expect(await screen.findByText(`This browser’s key: ${shortFingerprint(key.fingerprint)}`)).toBeTruthy()
  // The service still lists no key (say the registration was refused): the next session polls do
  // not send it again.
  await act(async () => { vi.advanceTimersByTime(5000) })
  await act(async () => { vi.advanceTimersByTime(5000) })
  await waitFor(() => expect(service.post.mock.calls.filter(([path]) => path === 'session').length).toBeGreaterThanOrEqual(3))
  expect(devicePosts(service.post)).toHaveLength(1)
  // CONTROL: a browser whose key the service already holds sends nothing at all.
  cleanup(); vi.restoreAllMocks()
  const held = account(), heldStation = crypto.randomUUID()
  const heldKey = (await deviceKey(heldStation))!
  held.stations.push({ id: heldStation, name: 'Home', device: { id: crypto.randomUUID(), name: 'Laptop', approved: 1, publicKey: heldKey.publicKey } })
  const quiet = client(held)
  render(<RemoteApp />)
  expect(await screen.findByText(`This browser’s key: ${shortFingerprint(heldKey.fingerprint)}`)).toBeTruthy()
  expect(devicePosts(quiet.post)).toHaveLength(0)
})

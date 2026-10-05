// @vitest-environment jsdom
// The Remote page and the station's own key (security review S3-L1). The page keeps the first key the
// service lists for each station this browser has a device on, shows it beside this browser's key for
// the operator to compare with Nexus at the shack, and while the service lists another one it says so,
// shows both, and refuses the stream until the operator accepts the new one there. jsdom has no
// IndexedDB, so the kept keys and the device keys are in memory here; everything else is the real page.
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient, RemoteError } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'
import type { StationKeyView } from './station-key'

type View = Promise<StationKeyView | null>
const kept = vi.hoisted(() => new Map<string, string>())
// One read of the kept keys can be made late: it reads the kept key at once, and the page has its view
// only when the test lets it go, as a read still hashing when the operator presses Accept would.
const late = vi.hoisted(() => ({ next: null as null | ((view: View) => View) }))
vi.mock('./station-key', async importOriginal => {
  const actual = await importOriginal<typeof import('./station-key')>()
  const pins = {
    keep: async (id: string, key: string) => { if (!kept.has(id)) kept.set(id, key); return kept.get(id)! },
    replace: async (id: string, key: string) => { kept.set(id, key) },
    forget: async (id: string) => { kept.delete(id) },
  }
  return { ...actual,
    stationKeyView: (id: string, listed: string | null | undefined) => {
      const view = actual.stationKeyView(id, listed, pins), hold = late.next
      late.next = null
      return hold ? hold(view) : view
    },
    acceptStationKey: (id: string, key: string) => actual.acceptStationKey(id, key, pins),
    forgetStationKey: (id: string) => actual.forgetStationKey(id, pins) }
})
vi.mock('./device-key', async importOriginal => {
  const actual = await importOriginal<typeof import('./device-key')>()
  const pairs = new Map<string, CryptoKeyPair>()
  const store = { get: async (id: string) => pairs.get(id),
    keep: async (id: string, pair: CryptoKeyPair) => { if (!pairs.has(id)) pairs.set(id, pair); return pairs.get(id)! } }
  return { ...actual, deviceKey: (stationId: string) => actual.deviceKey(stationId, store) }
})

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); kept.clear(); late.next = null })

// Made-up shapes of a station key (the P-256 SPKI prefix, 04, then one byte sixty-four times), not
// keys; and the first 128 bits of the SHA-256 of each, as both ends show them.
const PREFIX = '3059301306072a8648ce3d020106082a8648ce3d03010703420004'
const KEY_A = PREFIX + 'ab'.repeat(64), SHOWN_A = 'A8DD D2FF AD49 30AC 6B77 647B 8DE0 D379'
const KEY_B = PREFIX + 'cd'.repeat(64), SHOWN_B = '290E 40D9 5600 2535 47E2 0666 0607 A183'
const CHANGED = 'This station’s key has changed: the Remote service now lists a different key for it than the one this browser kept. Nothing connects until you accept the new key. Compare it with “This station’s key” in Nexus at the shack, and accept it only if they match.'
/** A key as both ends show it: 128 bits, eight groups of four. */
const EIGHT_GROUPS = /[0-9A-F]{4}( [0-9A-F]{4}){7}$/

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
const approved = () => ({ id: crypto.randomUUID(), name: 'Laptop', approved: 1 })
const streamButton = () => screen.getByRole('button', { name: 'Stream' }) as HTMLButtonElement
/** The page's next read of the kept keys, made late: it has read the kept key once `begun` settles, and
 *  the page has its view only on `deliver`, which answers that view. */
function lateRead() {
  let release!: () => void, read!: View
  const gate = new Promise<void>(resolve => { release = resolve })
  const begun = new Promise<void>(resolve => { late.next = view => { read = view; resolve(); return gate.then(() => view) } })
  return { begun, deliver: () => { release(); return read } }
}

it('S3-L1: the card keeps the first key the service lists for the station, and shows it beside this browser\'s, both in eight groups', async () => {
  const session = account(), stationId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', stationKey: KEY_A, device: approved() })
  client(session)
  render(<RemoteApp />)
  expect(await screen.findByText(`This station’s key: ${SHOWN_A}`)).toBeTruthy()
  expect(kept.get(stationId), 'first sight pins').toBe(KEY_A)
  expect((await screen.findByText(/^This browser’s key: /)).textContent).toMatch(EIGHT_GROUPS)
  expect(streamButton().disabled).toBe(false)
  // Control: a station that lists no key shows none and keeps none.
  cleanup()
  const keyless = account(), other = crypto.randomUUID()
  keyless.stations.push({ id: other, name: 'Field', stationKey: null, device: approved() })
  client(keyless)
  render(<RemoteApp />)
  await screen.findByText(/^This browser’s key: /)
  expect(screen.queryByText(/^This station’s key: /)).toBeNull()
  expect(kept.has(other)).toBe(false)
})

it('S3-L1: a browser waiting for its approval shows its key in eight groups, and keeps the station\'s key already', async () => {
  const session = account(), stationId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', stationKey: KEY_A, device: { id: crypto.randomUUID(), name: 'Laptop', approved: 0 } })
  client(session)
  render(<RemoteApp />)
  const waiting = await screen.findByText(/^Waiting for approval at the shack: .* Approve it if it shows this key: /)
  expect(waiting.textContent).toMatch(/: [0-9A-F]{4}( [0-9A-F]{4}){7}\.$/)
  await waitFor(() => expect(kept.get(stationId)).toBe(KEY_A))
})

it('S3-L1: another key listed: the card says so with both keys and Stream is off; polls never take the new key, only Accept does', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const session = account(), stationId = crypto.randomUUID()
  kept.set(stationId, KEY_A)
  session.stations.push({ id: stationId, name: 'Home', stationKey: KEY_B, device: approved() })
  const service = client(session)
  render(<RemoteApp />)
  expect(await screen.findByText(CHANGED)).toBeTruthy()
  expect(screen.getByText(`Key this browser kept: ${SHOWN_A}`)).toBeTruthy()
  expect(screen.getByText(`New key: ${SHOWN_B}`)).toBeTruthy()
  expect(streamButton().disabled, 'nothing connects').toBe(true)
  // Never automatic: the session is read again, twice, and the kept key and the question stay.
  await act(async () => { vi.advanceTimersByTime(5000) })
  await act(async () => { vi.advanceTimersByTime(5000) })
  await waitFor(() => expect(service.post.mock.calls.filter(([path]) => path === 'session').length).toBeGreaterThanOrEqual(3))
  expect(kept.get(stationId)).toBe(KEY_A)
  expect(screen.getByText(CHANGED)).toBeTruthy()
  // The operator's act: the new key is kept, and the card is as for any station.
  fireEvent.click(screen.getByRole('button', { name: 'Accept the new key' }))
  await waitFor(() => expect(kept.get(stationId)).toBe(KEY_B))
  expect(await screen.findByText(`This station’s key: ${SHOWN_B}`)).toBeTruthy()
  expect(screen.queryByText(CHANGED)).toBeNull()
  expect(streamButton().disabled, 'control: Stream is back').toBe(false)
})

it('S3-L1: a poll that read the kept key before Accept and finishes after it never brings the warning back', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  const session = account(), stationId = crypto.randomUUID()
  kept.set(stationId, KEY_A)
  session.stations.push({ id: stationId, name: 'Home', stationKey: KEY_B, device: approved() })
  client(session)
  render(<RemoteApp />)
  expect(await screen.findByText(CHANGED)).toBeTruthy()
  // The next poll reads the kept key, still A, and is still working when the operator accepts B.
  const read = lateRead()
  await act(async () => { vi.advanceTimersByTime(5000) })
  await read.begun
  fireEvent.click(screen.getByRole('button', { name: 'Accept the new key' }))
  await waitFor(() => expect(kept.get(stationId)).toBe(KEY_B))
  expect(await screen.findByText(`This station’s key: ${SHOWN_B}`)).toBeTruthy()
  // The poll finishes now, after the accept, with what it read before it.
  const view = await act(() => read.deliver())
  expect(view && view.keptPrint !== view.print, 'control: the late read saw the key kept before Accept').toBe(true)
  expect(screen.queryByText(CHANGED)).toBeNull()
  expect(screen.getByText(`This station’s key: ${SHOWN_B}`)).toBeTruthy()
  expect(streamButton().disabled, 'Stream stays on').toBe(false)
})

it('S3-L1: removing this browser\'s approval, or revoking the station, on the page clears the station\'s kept key', async () => {
  const session = account(), stationId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', stationKey: KEY_A, device: approved() })
  const service = client(session)
  // As the service does: the device goes with its approval, and the station with its access.
  service.post.mockImplementation(async path => {
    if (path === `stations/${stationId}/forget-device`) session.stations[0].device = null
    if (path === `stations/${stationId}/revoke`) session.stations = []
    return structuredClone(session)
  })
  render(<RemoteApp />)
  await waitFor(() => expect(kept.get(stationId)).toBe(KEY_A))
  fireEvent.click(screen.getByRole('button', { name: 'Remove this browser’s approval' }))
  await waitFor(() => expect(kept.has(stationId), 'removed with this browser\'s approval').toBe(false))
  // Still listed, with no device of this browser's on it: nothing is kept until this browser asks again.
  await screen.findByText(/isn’t approved for this station yet|isn't approved for this station yet/)
  expect(kept.has(stationId)).toBe(false)
  cleanup()

  const revoked = account(), other = crypto.randomUUID()
  revoked.stations.push({ id: other, name: 'Field', stationKey: KEY_B, device: approved() })
  const second = client(revoked)
  let refuse = true
  second.post.mockImplementation(async path => {
    if (path === `stations/${other}/revoke`) {
      // Control: a revoke the service refuses leaves the kept key as it was.
      if (refuse) throw new RemoteError(503)
      revoked.stations = []
    }
    return structuredClone(revoked)
  })
  render(<RemoteApp />)
  await waitFor(() => expect(kept.get(other)).toBe(KEY_B))
  fireEvent.click(screen.getByRole('button', { name: 'Revoke station access' }))
  await screen.findByRole('alert')
  expect(kept.get(other), 'control: refused, nothing cleared').toBe(KEY_B)
  refuse = false
  fireEvent.click(screen.getByRole('button', { name: 'Revoke station access' }))
  await waitFor(() => expect(kept.has(other), 'revoked with the station').toBe(false))
})

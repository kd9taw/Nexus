// @vitest-environment jsdom
// The Remote site's one way into streaming (the operator, 2026-10-02, "Stream, plus Listen": "Each
// station card has one big "Stream" button and a small "Listen" for audio only. The old watch/control
// workspace is hidden, though its code stays so we can bring it back after streaming is proven."). Every
// state a station card can be in says ONE plain sentence and the one next step; the states only the
// open page can know (the station away, streaming off at the shack, another browser in control, the
// key not approved there) are StreamView.entry.test.tsx.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient, HostedConnection } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'

// jsdom has no IndexedDB: the key module keeps its keys in memory here (as RemoteApp.device-key.test.tsx
// does), so the card can show the key the shack will show.
vi.mock('./device-key', async importOriginal => {
  const actual = await importOriginal<typeof import('./device-key')>()
  const pairs = new Map<string, CryptoKeyPair>()
  const store = { get: async (id: string) => pairs.get(id),
    keep: async (id: string, pair: CryptoKeyPair) => { if (!pairs.has(id)) pairs.set(id, pair); return pairs.get(id)! } }
  return { ...actual, deviceKey: (stationId: string) => actual.deviceKey(stationId, store) }
})

const CHROME_ON_WINDOWS = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
const BETA = 'Remote streaming is a beta feature. Access could be revoked at any time.'
const READY = 'Ready. Stream to see and operate Nexus at the shack, or Listen for its audio only.'
const NOT_APPROVED = 'This browser isn\'t approved for this station yet. Press Stream, and Nexus at the shack asks you to approve it.'
const WAITING = /^Waiting for approval at the shack: Nexus there asks whether to let “Chrome on Windows” stream\. Approve it if it shows this key: [0-9A-F]{4}( [0-9A-F]{4}){7}\.$/

afterEach(() => { cleanup(); vi.restoreAllMocks(); localStorage.clear() })
function account(state: AccountSession['entitlement']['state'] = 'active'): AccountSession {
  const accountId = crypto.randomUUID(), now = Date.now()
  return { accountId, serverNow: now, stations: [], pending: null,
    entitlement: { accountId, enabled: state === 'active', expiresAt: now + 3600000, state, startedAt: state === 'none' ? null : now,
      source: state === 'none' ? null : 'trial' } }
}
function client(session: AccountSession) {
  const post = vi.fn(async (_path: string, _body?: object): Promise<unknown> => structuredClone(session))
  const service = { post, authenticated: async () => true, signIn: vi.fn(async () => {}), signOut: vi.fn(async () => {}), signInRefusal: null,
    streamVersion: 1 }
  vi.spyOn(BrowserClient, 'load').mockResolvedValue(service as unknown as BrowserClient)
  return service
}
const card = () => document.querySelector('.rm-card.remote-section')!
const sentence = () => card().querySelector('.remote-station-state')?.textContent

it('an approved browser\'s card: one big Stream, a small Listen, the beta line, one sentence, and the old workspace hidden', async () => {
  const session = account()
  session.stations.push({ id: crypto.randomUUID(), name: 'Home', device: { id: crypto.randomUUID(), name: 'Chrome on Windows', approved: 1 } })
  client(session)
  render(<RemoteApp />)
  const stream = await screen.findByRole('button', { name: 'Stream' })
  const listen = screen.getByRole('button', { name: 'Listen' })
  // The one thing to do next is the amber one; Listen is the plain small one beside it.
  expect(stream.classList.contains('remote-button--primary')).toBe(true)
  expect(listen.classList.contains('remote-button--primary')).toBe(false)
  expect(stream.compareDocumentPosition(listen) & Node.DOCUMENT_POSITION_FOLLOWING, 'Stream comes first').toBeTruthy()
  expect(sentence()).toBe(READY)
  expect(card().querySelector('.remote-beta')?.textContent).toBe(`Beta ${BETA}`)
  expect(screen.queryByRole('button', { name: 'Open Nexus' })).toBeNull()
  expect(screen.queryByRole('button', { name: 'Observe station' })).toBeNull()
})

it('the old workspace and observer come back, unchanged, behind the documented flag', async () => {
  localStorage.setItem('nexus.remote.workspace', 'on')
  const session = account()
  session.stations.push({ id: crypto.randomUUID(), name: 'Home', device: { id: crypto.randomUUID(), name: 'Laptop', approved: 1 } })
  client(session)
  render(<RemoteApp />)
  expect(await screen.findByRole('button', { name: 'Open Nexus' })).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Observe station' })).toBeTruthy()
  // Stream stays the card's first and primary way in with the flag on.
  expect(screen.getByRole('button', { name: 'Stream' }).classList.contains('remote-button--primary')).toBe(true)
  // Anything but 'on' leaves them hidden.
  cleanup(); localStorage.setItem('nexus.remote.workspace', '1')
  client(session)
  render(<RemoteApp />)
  await screen.findByRole('button', { name: 'Stream' })
  expect(screen.queryByRole('button', { name: 'Open Nexus' })).toBeNull()
})

it('not approved yet: one sentence, and Stream asks the shack under this browser\'s own name and key, with nothing to type', async () => {
  vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(CHROME_ON_WINDOWS)
  const session = account(), stationId = crypto.randomUUID(), deviceId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', device: null })
  const service = client(session)
  service.post.mockImplementation(async (path, body) => {
    if (path.endsWith('/device')) session.stations[0].device = { id: deviceId, name: (body as { name: string }).name, approved: 0,
      publicKey: (body as { publicKey?: string }).publicKey ?? null }
    return structuredClone(session)
  })
  render(<RemoteApp />)
  await screen.findByRole('button', { name: 'Stream' })
  expect(sentence()).toBe(NOT_APPROVED)
  expect(card().querySelector('.remote-beta')?.textContent).toBe(`Beta ${BETA}`)
  expect(card().querySelector('input'), 'no name to type before asking').toBeNull()
  // Control: nothing is asked until the operator presses.
  expect(service.post.mock.calls.filter(([path]) => path.endsWith('/device'))).toHaveLength(0)
  fireEvent.click(screen.getByRole('button', { name: 'Stream' }))
  await waitFor(() => expect(service.post).toHaveBeenCalledWith(`stations/${stationId}/device`,
    { name: 'Chrome on Windows', publicKey: expect.stringMatching(/^[0-9a-f]+$/) }))
  // Waiting: the next step is at the shack, so the card offers nothing to press here.
  await waitFor(() => expect(sentence()).toMatch(WAITING))
  expect(screen.queryByRole('button', { name: 'Stream' })).toBeNull()
  expect(screen.queryByRole('button', { name: 'Listen' })).toBeNull()
  expect(card().textContent).toContain(`Browser code: ${deviceId.slice(-6)}`)
  expect(card().querySelector('.remote-beta')?.textContent).toBe(`Beta ${BETA}`)
})

it('Listen asks the shack too when the browser is not approved, and opens audio only once it is', async () => {
  const session = account(), stationId = crypto.randomUUID()
  session.stations.push({ id: stationId, name: 'Home', device: null })
  const service = client(session)
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: 'Listen' }))
  await waitFor(() => expect(service.post).toHaveBeenCalledWith(`stations/${stationId}/device`, expect.anything()))
  cleanup()
  // Approved: Listen opens the station with no picture; Stream opens the stream. Both start on their own.
  session.stations[0].device = { id: crypto.randomUUID(), name: 'Laptop', approved: 1 }
  client(session)
  vi.spyOn(HostedConnection.prototype, 'start').mockImplementation(() => {})
  vi.spyOn(HostedConnection.prototype, 'stop').mockImplementation(() => {})
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: 'Listen' }))
  await waitFor(() => expect(document.querySelector('.remote-stream-app')).toBeTruthy())
  expect(document.querySelector('.remote-stream-video'), 'Listen: no picture').toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Disconnect and return to stations' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Stream' }))
  await waitFor(() => expect(document.querySelector('.remote-stream-video')).toBeTruthy())
})

it('an account whose access has stopped: the card says why in one sentence, and Stream and Listen cannot be pressed', async () => {
  const session = account('ended')
  session.stations.push({ id: crypto.randomUUID(), name: 'Home', device: { id: crypto.randomUUID(), name: 'Laptop', approved: 1 } })
  client(session)
  render(<RemoteApp />)
  const stream = await screen.findByRole('button', { name: 'Stream' }) as HTMLButtonElement
  expect(stream.disabled).toBe(true)
  expect((screen.getByRole('button', { name: 'Listen' }) as HTMLButtonElement).disabled).toBe(true)
  expect(sentence()).toMatch(/^Your trial ended/)
  expect(sentence()).not.toBe(READY)
})

it('signed out and with no station, the page says the one next step: sign in, then pair at the shack', async () => {
  vi.spyOn(BrowserClient, 'load').mockResolvedValue({ post: vi.fn(), authenticated: async () => false, signIn: vi.fn(), signOut: vi.fn(),
    signInRefusal: null } as unknown as BrowserClient)
  render(<RemoteApp />)
  expect(await screen.findByRole('button', { name: 'Sign in' })).toBeTruthy()
  expect(screen.queryByRole('button', { name: 'Stream' })).toBeNull()
  cleanup()
  client(account('none'))
  render(<RemoteApp />)
  expect(await screen.findByText(/open Settings → Station → Remote access and start pairing/)).toBeTruthy()
  expect(screen.getByRole('button', { name: 'Link to my account' })).toBeTruthy()
})

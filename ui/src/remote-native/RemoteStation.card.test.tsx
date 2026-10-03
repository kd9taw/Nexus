// @vitest-environment jsdom
// Settings ▸ Remote access as one card (the operator, 2026-10-02, "One simple panel, Advanced folded":
// "a "Stream this station from my browser" switch, your approved browsers with their keys, and the
// sign-in status. The old pairing and permission options fold under "Advanced" (kept, not deleted).").
// RemoteStation.test.tsx keeps proving the old options' behaviour, now under Advanced.
import { afterEach, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { RemoteStationAction, RemoteStationStatus } from './types'

afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__ })
const fingerprint = (c: string) => c.repeat(64)
const short = (hex: string) => hex.slice(0, 16).toUpperCase().match(/.{4}/g)!.join(' ')

function mount(status: RemoteStationStatus) {
  const actions: RemoteStationAction[] = []
  window.__TAURI_INTERNALS__ = { invoke: (async (command: string, input?: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    actions.push((input as { action: RemoteStationAction }).action)
    return status
  }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  return actions
}
const main = () => {
  // Everything in the card that is not under Advanced.
  const root = document.querySelector('.remote-native')!.cloneNode(true) as HTMLElement
  root.querySelector('.remote-native-advanced')?.remove()
  return root
}

it('one card: the sign-in status, then each browser with its key and the one thing to do next; the permissions are under Advanced, folded', async () => {
  const day = 86400000, now = Date.now()
  const pinned = crypto.randomUUID(), unpinned = crypto.randomUUID(), waiting = crypto.randomUUID()
  const actions = mount({ phase: 'connected', origin: 'https://remote-staging.hamradiotools.io', stationId: crypto.randomUUID(),
    accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null, error: null,
    devices: [
      { id: pinned, name: 'Chrome on Windows', approved: 1, expiresAt: now + 30 * day, key: fingerprint('a') },
      { id: unpinned, name: 'Laptop', approved: 1, expiresAt: now + 30 * day, key: fingerprint('b') },
      { id: waiting, name: 'Firefox on Android', approved: 0, expiresAt: now + 600000, key: fingerprint('c') },
    ], pinnedDevices: [pinned], loggingPermissions: [pinned], stationPermissions: [pinned], transmitPermissions: [] })
  const list = await screen.findByRole('list')
  expect(screen.getByRole('status').textContent).toBe('Connected to the Remote service')
  const rows = within(list).getAllByRole('listitem')
  expect(rows.map(row => row.querySelector('.remote-native-browser-name')?.textContent)).toEqual(['Chrome on Windows', 'Laptop', 'Firefox on Android'])
  expect(rows.map(row => row.querySelector('.remote-native-browser-key')?.textContent)).toEqual(
    [`Key ${short(fingerprint('a'))}`, `Key ${short(fingerprint('b'))}`, `Key ${short(fingerprint('c'))}`])
  // Approved and pinned: it can stream, and Remove is its one action.
  expect(within(rows[0]).queryByRole('button', { name: 'Approve' })).toBeNull()
  expect(within(rows[0]).getByRole('button', { name: 'Remove' })).toBeTruthy()
  // Approved before its key was pinned: approve it again, or remove it.
  expect(rows[1].textContent).toContain('can’t stream until you approve it again here')
  expect(within(rows[1]).getByRole('button', { name: 'Approve' })).toBeTruthy()
  expect(rows[2].textContent).toContain('Asks to stream this station.')
  fireEvent.click(within(rows[2]).getByRole('button', { name: 'Approve' }))
  await waitFor(() => expect(actions).toContainEqual({ type: 'device', deviceId: waiting, approve: true, transmit: false, key: fingerprint('c') }))
  fireEvent.click(within(rows[0]).getByRole('button', { name: 'Remove' }))
  await waitFor(() => expect(actions).toContainEqual({ type: 'device', deviceId: pinned, approve: false }))
  // The permission and housekeeping controls are all under Advanced, which starts folded.
  const advanced = document.querySelector('details.remote-native-advanced') as HTMLDetailsElement
  expect(advanced.open).toBe(false)
  expect(advanced.querySelector('summary')?.textContent).toBe('Advanced')
  for (const name of ['Revoke logging permission', 'Revoke station controls', 'Allow FT8/FT4 transmission', 'Refresh browser requests',
    'Revoke station access', 'Approve browser', 'Approve again', 'Revoke browser approval']) {
    expect(within(advanced).getAllByRole('button', { name }).length, `${name} is kept under Advanced`).toBeGreaterThan(0)
    expect(within(main()).queryByRole('button', { name }), `${name} is not in the card itself`).toBeNull()
  }
  expect(within(advanced).getAllByRole('checkbox', { name: 'Also allow FT8/FT4 transmit' }).length).toBeGreaterThan(0)
  expect(within(main()).queryByRole('checkbox')).toBeNull()
})

it('the approve in the card honours the transmit tick under Advanced, exactly as the old approve did (off unless ticked)', async () => {
  const waiting = crypto.randomUUID()
  const actions = mount({ phase: 'connected', origin: 'https://remote-staging.hamradiotools.io', stationId: crypto.randomUUID(),
    accountId: crypto.randomUUID(), pairingId: null, pairingCode: null, expiresAt: null, error: null, pinnedDevices: [],
    devices: [{ id: waiting, name: 'Phone', approved: 0, expiresAt: Date.now() + 600000, key: fingerprint('d') }] })
  const row = (await screen.findAllByRole('listitem'))[0]
  fireEvent.click(within(document.querySelector('.remote-native-advanced') as HTMLElement).getByRole('checkbox', { name: 'Also allow FT8/FT4 transmit' }))
  fireEvent.click(within(row).getByRole('button', { name: 'Approve' }))
  await waitFor(() => expect(actions).toEqual([{ type: 'device', deviceId: waiting, approve: true, transmit: true, key: fingerprint('d') }]))
})

it('not linked yet, the card says so and holds the pairing steps; Remote off, the card says so with Turn on Remote beside it', async () => {
  mount({ phase: 'unpaired', origin: 'https://remote-staging.hamradiotools.io', stationId: null, accountId: null, pairingId: null,
    pairingCode: null, expiresAt: null, devices: [], error: null })
  expect((await screen.findByRole('status')).textContent).toBe('No Remote account paired')
  expect(within(main()).getByRole('button', { name: 'Pair a station' })).toBeTruthy()
  expect(within(main()).getByLabelText('Station name')).toBeTruthy()
  cleanup()
  mount({ phase: 'disabled', origin: 'https://remote-staging.hamradiotools.io', stationId: crypto.randomUUID(), accountId: crypto.randomUUID(),
    pairingId: null, pairingCode: null, expiresAt: null, devices: [], error: null })
  expect((await screen.findByRole('status')).textContent).toBe('Remote is off')
  expect(within(main()).getByRole('button', { name: 'Turn on Remote' })).toBeTruthy()
  expect(within(main()).getByText('No current browser requests or approvals')).toBeTruthy()
})

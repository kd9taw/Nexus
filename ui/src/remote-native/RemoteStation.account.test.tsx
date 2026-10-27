// @vitest-environment jsdom
// Settings ▸ Remote access, the account's part: signing in at the shack with no code, the notice of a device
// the station granted on its own proof, what the card says of a proof it could not confirm or a removal the
// service has not heard yet, and People.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import type { AccountStatus, RemoteStationAction, RemoteStationStatus } from './types'
import { EN } from '../i18n/en'

vi.setConfig({ testTimeout: 15_000 })
afterEach(() => { cleanup(); delete window.__TAURI_INTERNALS__ })

const noAccount: AccountStatus = { signingIn: false, added: [], unconfirmed: {}, removals: [], people: [] }
const bound = (account: Partial<AccountStatus>, devices: RemoteStationStatus['devices'] = []): RemoteStationStatus => ({
  phase: 'connected', origin: 'https://remote.hamradiotools.io', stationId: crypto.randomUUID(), accountId: crypto.randomUUID(),
  pairingId: null, pairingCode: null, expiresAt: null, devices, error: null, account: { ...noAccount, ...account } })

/** The card over a station whose status `next` answers each action with, recording the actions. */
function shack(first: RemoteStationStatus, next: (action: RemoteStationAction, status: RemoteStationStatus) => RemoteStationStatus = (_, s) => s) {
  let status = first
  const actions: RemoteStationAction[] = []
  window.__TAURI_INTERNALS__ = { invoke: (async (command: string, input: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    actions.push(action)
    status = next(action, status)
    return status
  }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  return actions
}

it('signs in at the shack with the station name, waits for the browser, and can cancel', async () => {
  const unpaired: RemoteStationStatus = { phase: 'unpaired', origin: 'https://remote.hamradiotools.io', stationId: null, accountId: null,
    pairingId: null, pairingCode: null, expiresAt: null, devices: [], error: null, account: noAccount }
  const actions = shack(unpaired, (action, status) => action.type === 'signIn' ? { ...status, account: { ...noAccount, signingIn: true } }
    : action.type === 'cancel' ? { ...status, account: noAccount } : status)
  const signIn = await screen.findByRole('button', { name: EN['remote.account.signIn'] }) as HTMLButtonElement
  expect(signIn.disabled).toBe(true)
  // Pairing with a code stays beside it.
  expect(screen.getByRole('button', { name: EN['remote.pairStation'] })).toBeTruthy()
  fireEvent.change(screen.getByLabelText(EN['remote.stationName']), { target: { value: 'Shack' } })
  fireEvent.click(signIn)
  await screen.findByText(EN['remote.account.signingIn'])
  expect(screen.queryByRole('button', { name: EN['remote.pairStation'] })).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.cancelSignIn'] }))
  await screen.findByRole('button', { name: EN['remote.account.signIn'] })
  expect(actions).toEqual([{ type: 'signIn', name: 'Shack' }, { type: 'cancel' }])
})

it('says why signing in is not offered by this build', async () => {
  shack({ ...bound({}), phase: 'unpaired', stationId: null, error: 'accountNotReady' })
  await screen.findByText(EN['remote.account.notReady'])
})

it('shows a device the station granted on its own proof until the operator has seen it, with Remove beside it', async () => {
  const deviceId = crypto.randomUUID(), key = 'a1b2c3d4'.repeat(8)
  const added = { deviceId, name: 'DESKTOP-7Q', kind: 'computer' as const, platform: 'windows' as const, key, role: 'operate' as const, guest: false }
  const actions = shack(bound({ added: [added] }), (action, status) => action.type === 'dismissAdded' ? { ...status, account: noAccount } : status)
  const notice = await screen.findByText(/DESKTOP-7Q \(Nexus on Windows\), key A1B2/)
  expect(notice.textContent).toMatch(/^Added by signing in as you/)
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.gotIt'] }))
  await waitFor(() => expect(screen.queryByText(/DESKTOP-7Q/)).toBeNull())
  expect(actions).toEqual([{ type: 'dismissAdded', deviceId }])
})

it('removes a granted device straight from its notice', async () => {
  const deviceId = crypto.randomUUID()
  const added = { deviceId, name: 'K9 laptop', kind: 'browser' as const, key: null, role: 'listen' as const, guest: true }
  const actions = shack(bound({ added: [added] }))
  await screen.findByText(/Added for someone you invited \(Listen\): K9 laptop \(a browser\)/)
  fireEvent.click(screen.getByRole('button', { name: EN['remote.native.remove'] }))
  await waitFor(() => expect(actions).toEqual([{ type: 'device', deviceId, approve: false }]))
})

it('says when the station could not confirm a device the service approved, and when a removal waits', async () => {
  const unconfirmed = crypto.randomUUID(), removed = crypto.randomUUID(), key = 'ab'.repeat(32)
  const devices = [
    { id: unconfirmed, name: 'Unknown browser', approved: 1, expiresAt: Date.now() + 600000, key, addedBy: 'account' as const },
    { id: removed, name: 'Old browser', approved: 1, expiresAt: Date.now() + 600000, key },
  ]
  shack({ ...bound({ unconfirmed: { [unconfirmed]: 'proofNotYours' }, removals: [removed] }, devices), pinnedDevices: [] })
  await screen.findByText(EN['remote.account.unconfirmed'])
  expect(screen.getByText(EN['remote.account.removalWaiting'])).toBeTruthy()
  expect(screen.getByText(EN['remote.account.addedByAccount'])).toBeTruthy()
  // The prompt a device added at the station gets is still there: approving it is the operator's call.
  expect(screen.getAllByRole('button', { name: EN['remote.approve'] }).length).toBe(2)
})

// A device added by signing in is listed approved by the service, and the station grants it nothing until it is approved
// here (`remote_service::account::GRANTS_ON_SIGN_IN`): its row asks, as a device added at the station asks, with Approve, and
// never says the station could not confirm it (it could).
it('shows a device added by signing in as asking, with Approve, until it is approved here', async () => {
  const computer = crypto.randomUUID(), browser = crypto.randomUUID(), key = 'ab'.repeat(32)
  const devices = [
    { id: computer, name: 'DESKTOP-7Q', approved: 1, expiresAt: Date.now() + 600000, key, kind: 'computer' as const,
      platform: 'windows' as const, addedBy: 'account' as const },
    { id: browser, name: 'Chrome on Windows', approved: 1, expiresAt: Date.now() + 600000, key, addedBy: 'account' as const },
  ]
  const actions = shack({ ...bound({ unconfirmed: { [computer]: 'approveAtStation', [browser]: 'approveAtStation' } }, devices),
    pinnedDevices: [] })
  await screen.findByText(EN['remote.native.computerAsks'])
  expect(screen.getByText(EN['remote.native.asks'])).toBeTruthy()
  expect(screen.queryByText(EN['remote.account.unconfirmed']), 'it was confirmed').toBeNull()
  expect(screen.queryByText(EN['remote.native.computerKeyNotPinned']), 'never approved here').toBeNull()
  expect(screen.getAllByText(EN['remote.account.addedByAccount']).length).toBe(2)
  fireEvent.click(within(screen.getByText('DESKTOP-7Q').closest('li')!).getByRole('button', { name: EN['remote.approve'] }))
  await waitFor(() => expect(actions).toEqual([{ type: 'device', deviceId: computer, approve: true, key, kind: 'computer' }]))
})

// Someone invited to operate holds what the owner's own other computer holds. The form says so where Operate is offered,
// so the owner invites to operate only someone they would hand the shack to.
it('says what Operate gives where it is offered, and not for Listen', async () => {
  shack(bound({}))
  const role = await screen.findByLabelText(EN['remote.account.role']) as HTMLSelectElement
  expect(role.value, 'Operate is offered first').toBe('operate')
  expect(screen.getByText(EN['remote.account.operateIsYours'])).toBeTruthy()
  fireEvent.change(role, { target: { value: 'listen' } })
  expect(screen.queryByText(EN['remote.account.operateIsYours']), 'CONTROL: Listen').toBeNull()
})

it('invites someone with a role, lists the people invited, and takes an invitation back', async () => {
  const actions = shack(bound({ people: [{ mailbox: 'k9.guest@example.test', role: 'listen' }] }))
  const person = await screen.findByText('k9.guest@example.test')
  expect(person.parentElement!.textContent).toContain(EN['remote.account.role.listen'])
  const invite = screen.getByRole('button', { name: EN['remote.account.invite'] }) as HTMLButtonElement
  expect(invite.disabled).toBe(true)
  fireEvent.change(screen.getByLabelText(EN['remote.account.email']), { target: { value: ' W1AW@example.test ' } })
  fireEvent.change(screen.getByLabelText(EN['remote.account.role']), { target: { value: 'listen' } })
  fireEvent.click(invite)
  await waitFor(() => expect(actions).toEqual([{ type: 'invite', email: 'W1AW@example.test', role: 'listen' }]))
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.uninvite'] }))
  await waitFor(() => expect(actions[actions.length - 1]).toEqual({ type: 'uninvite', email: 'k9.guest@example.test' }))
})

it('names the other computers an invitation taken back took off this network, and nothing when there were none', async () => {
  shack(bound({ people: [], alsoUninvited: ['Old guest PC', 'Shed laptop'] }))
  const told = await screen.findByText(EN['remote.account.alsoUninvited'].replace('{{names}}', 'Old guest PC, Shed laptop'))
  expect(told.getAttribute('role')).toBe('status')
  cleanup()
  shack(bound({ people: [], alsoUninvited: [] }))
  await screen.findByRole('heading', { name: EN['remote.account.people'] })
  expect(screen.queryByText(/Also taken off this network/), 'CONTROL: none taken').toBeNull()
})

it('points to Reset network identity while the list of computers removed here does not read, and says nothing once it does', async () => {
  shack(bound({ removalsUnreadable: true }))
  const told = await screen.findByText(EN['remote.account.removalsUnreadable'])
  expect(told.getAttribute('role')).toBe('alert')
  cleanup()
  shack(bound({ removalsUnreadable: false }))
  await screen.findByRole('heading', { name: EN['remote.account.people'] })
  expect(screen.queryByText(EN['remote.account.removalsUnreadable']), 'CONTROL: it reads').toBeNull()
})

it('asks a bound station to sign in again once its proof key is retired, and waits for the browser', async () => {
  const actions = shack(bound({ reproof: true }), (action, status) => action.type === 'signInAgain'
    ? { ...status, account: { ...status.account!, signingIn: true } }
    : action.type === 'cancelSignInAgain' ? { ...status, account: { ...status.account!, signingIn: false } } : status)
  await screen.findByText(EN['remote.account.reproof'])
  fireEvent.click(await screen.findByRole('button', { name: EN['remote.account.signInAgain'] }))
  await screen.findByText(EN['remote.account.signingIn'])
  expect(actions[actions.length - 1]).toEqual({ type: 'signInAgain' })
  expect(screen.queryByRole('button', { name: EN['remote.account.signInAgain'] })).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.cancelSignIn'] }))
  await screen.findByRole('button', { name: EN['remote.account.signInAgain'] })
  expect(actions[actions.length - 1]).toEqual({ type: 'cancelSignInAgain' })
})

it('offers no second sign-in while the proof still holds, and names a sign-in by someone else', async () => {
  shack({ ...bound({}), error: 'proofNotYours' })
  await screen.findByText(EN['remote.account.notOwner'])
  expect(screen.queryByRole('button', { name: EN['remote.account.signInAgain'] })).toBeNull()
})

it('shows a held sign-in device waiting to transmit with Allow to transmit, a browser and a computer alike, and then as it was', async () => {
  const browser = crypto.randomUUID(), computer = crypto.randomUUID(), free = crypto.randomUUID()
  const pinned = (id: string, name: string, kind: 'browser' | 'computer', key: string) => ({ id, name, approved: 1, expiresAt: Date.now() + 86_400_000,
    untilRemoved: 1, key, kind, platform: kind === 'computer' ? 'windows' as const : null, role: 'operate' as const, addedBy: 'account' as const })
  const devices = [pinned(browser, 'Owner phone', 'browser', 'aa'.repeat(32)), pinned(computer, 'DESKTOP-7Q', 'computer', 'bb'.repeat(32)),
    pinned(free, 'Free browser', 'browser', 'cc'.repeat(32))]
  const actions = shack({ ...bound({}, devices), pinnedDevices: [browser, computer, free], heldDevices: [browser, computer] },
    (action, status) => action.type === 'allowTransmit'
      ? { ...status, heldDevices: (status.heldDevices ?? []).filter(id => id !== action.deviceId) } : status)
  await screen.findAllByText('Owner phone')
  expect(screen.getAllByText(EN['remote.account.waitingToTransmit'])).toHaveLength(2)
  // One name for the shack's act, the one its button and the pop-up use: never the pairing's Approve.
  for (const line of screen.getAllByText(EN['remote.account.waitingToTransmit'])) expect(line.textContent).not.toMatch(/approv/i)
  const allows = screen.getAllByRole('button', { name: EN['remote.account.allowTransmit'] })
  expect(allows).toHaveLength(2)
  fireEvent.click(allows[0])
  await waitFor(() => expect(screen.getAllByRole('button', { name: EN['remote.account.allowTransmit'] })).toHaveLength(1))
  fireEvent.click(screen.getByRole('button', { name: EN['remote.account.allowTransmit'] }))
  await waitFor(() => expect(screen.queryByText(EN['remote.account.waitingToTransmit'])).toBeNull())
  expect(screen.queryByRole('button', { name: EN['remote.account.allowTransmit'] })).toBeNull()
  // Each then shows as it did: approved, with Remove beside it.
  expect(screen.getAllByRole('button', { name: EN['remote.native.remove'] })).toHaveLength(3)
  expect(actions).toEqual([{ type: 'allowTransmit', deviceId: browser }, { type: 'allowTransmit', deviceId: computer }])
})

it('offers Allow to transmit only on a held row with no Approve: never one asking, waiting for Approve, unpinned, unconfirmed or removed here', async () => {
  const held = crypto.randomUUID(), asking = crypto.randomUUID(), waits = crypto.randomUUID(), unpinned = crypto.randomUUID(),
    unconfirmed = crypto.randomUUID(), removed = crypto.randomUUID()
  const row = (id: string, name: string, key: string) => ({ id, name, approved: 1, expiresAt: Date.now() + 86_400_000, untilRemoved: 1,
    key, kind: 'browser' as const, role: 'operate' as const, addedBy: 'account' as const })
  const devices = [row(held, 'Held browser', 'aa'.repeat(32)), row(waits, 'Waiting browser', 'bb'.repeat(32)),
    row(unpinned, 'Unpinned browser', 'cc'.repeat(32)), row(unconfirmed, 'Unconfirmed browser', 'dd'.repeat(32)),
    row(removed, 'Removed browser', 'ee'.repeat(32)), { ...row(asking, 'Asking browser', 'ff'.repeat(32)), approved: 0 }]
  // As a station that named every one of them held would show them: the card still offers Allow on the held row alone.
  const actions = shack({ ...bound({ unconfirmed: { [waits]: 'approveAtStation', [unconfirmed]: 'tokenAudience' }, removals: [removed] }, devices),
    pinnedDevices: [held, asking, waits, unconfirmed, removed], heldDevices: [held, asking, waits, unpinned, unconfirmed, removed] })
  await screen.findAllByText('Held browser')
  expect(screen.getAllByText(EN['remote.account.waitingToTransmit'])).toHaveLength(1)
  const allows = screen.getAllByRole('button', { name: EN['remote.account.allowTransmit'] })
  expect(allows).toHaveLength(1)
  expect(screen.getAllByRole('button', { name: EN['remote.approve'] }).length, 'the rows that ask keep their Approve').toBeGreaterThan(0)
  fireEvent.click(allows[0])
  await waitFor(() => expect(actions).toEqual([{ type: 'allowTransmit', deviceId: held }]))
})

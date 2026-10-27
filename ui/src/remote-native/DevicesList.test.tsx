// @vitest-environment jsdom
//
// Settings ▸ Remote access ▸ Devices: one list at the shack, by person (the operator's ruling of 2026-10-25).
// It replaces the separate lists of browsers, computers, paired computers and people, and the browser
// approvals under Advanced. PRESENTATION ONLY: every button here is held to the action the old list it
// replaces sent, by the same ids (each case names that list), and every one of them is refused through a
// stream.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteStation } from './RemoteStation'
import { StreamInputDispatcher } from './stream-input'
import type { LanStatus, RemoteStationAction, RemoteStationStatus } from './types'
import { EN } from '../i18n/en'

vi.setConfig({ testTimeout: 15_000 })

const OWN_BROWSER = '10000000-0000-4000-8000-0000000000b1', OWN_LAPTOP = '10000000-0000-4000-8000-0000000000c1'
const LAPTOP_LAN = '8a1f0c2e-77d3-8b41-9c0a-5e2b6d4f1a30', DEN_LAN = '8a1f0c2e-77d3-8b41-9c0a-5e2b6d4f1a31'
const GUEST_PHONE = '10000000-0000-4000-8000-0000000000d1', GUEST_LAN = '8a1f0c2e-77d3-8b41-9c0a-5e2b6d4f1a32'
const ASKING = '10000000-0000-4000-8000-0000000000e1', HELD = '10000000-0000-4000-8000-0000000000f1'
const STRANGER = '10000000-0000-4000-8000-0000000000a1'
const GUEST = 'k9.guest@example.test', IDLE_GUEST = 'w1aw@example.test'
const key = (c: string) => c.repeat(64)
const short = (hex: string) => hex.slice(0, 32).toUpperCase().match(/.{4}/g)!.join(' ')
const until = Date.now() + 30 * 86400000

const lan: LanStatus = { on: true, listening: '192.168.1.20:42075', key: key('9'), devices: [
  { id: LAPTOP_LAN, name: 'Shack laptop', key: key('1') },
  { id: DEN_LAN, name: 'Den PC', key: key('2') },
  { id: GUEST_LAN, name: 'Guest notebook', key: key('3') },
] }
/** A station with a device in every state, on every road, for you, a guest and nobody known. */
const station = (): RemoteStationStatus => ({
  phase: 'connected', origin: 'https://remote.invalid', stationId: crypto.randomUUID(), accountId: crypto.randomUUID(),
  pairingId: null, pairingCode: null, expiresAt: null, error: null, lan,
  devices: [
    { id: OWN_BROWSER, name: 'Chrome on Windows', approved: 1, expiresAt: until, untilRemoved: 1, key: key('a'), kind: 'browser', addedBy: 'shack' },
    { id: OWN_LAPTOP, name: 'Shack laptop', approved: 1, expiresAt: until, untilRemoved: 1, key: key('b'), kind: 'computer', platform: 'windows', role: 'operate', addedBy: 'account' },
    { id: GUEST_PHONE, name: 'Pixel', approved: 1, expiresAt: until, untilRemoved: 1, key: key('c'), kind: 'phone', platform: 'android', role: 'listen', addedBy: 'account' },
    { id: ASKING, name: 'Firefox on Android', approved: 0, expiresAt: until, key: key('d'), kind: 'browser' },
    { id: HELD, name: 'Travel laptop', approved: 1, expiresAt: until, untilRemoved: 1, key: key('e'), kind: 'computer', platform: 'macos', role: 'operate', addedBy: 'account' },
    { id: STRANGER, name: 'Edge on Windows', approved: 1, expiresAt: until, untilRemoved: 1, key: key('f'), kind: 'browser', addedBy: 'account' },
  ],
  pinnedDevices: [OWN_BROWSER, OWN_LAPTOP, GUEST_PHONE, HELD, STRANGER],
  heldDevices: [HELD], stationPermissions: [OWN_BROWSER, HELD], controller: OWN_LAPTOP,
  account: { signingIn: false, added: [], unconfirmed: {}, removals: [],
    people: [{ mailbox: GUEST, role: 'listen' }, { mailbox: IDLE_GUEST, role: 'operate' }] },
  deviceRows: [
    { listed: [OWN_BROWSER], lan: [], guest: null, unknown: false, live: false },
    { listed: [OWN_LAPTOP], lan: [LAPTOP_LAN], guest: null, unknown: false, live: true },
    { listed: [GUEST_PHONE], lan: [], guest: GUEST, unknown: false, live: false },
    { listed: [ASKING], lan: [], guest: null, unknown: false, live: false },
    { listed: [HELD], lan: [], guest: null, unknown: false, live: false, holdLeft: 541 },
    { listed: [STRANGER], lan: [], guest: null, unknown: true, live: false },
    { listed: [], lan: [DEN_LAN], guest: null, unknown: false, live: false },
    { listed: [], lan: [GUEST_LAN], guest: GUEST, unknown: false, live: false },
  ],
})

let under: Element | null = null, stream: StreamInputDispatcher | null = null
beforeEach(() => {
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
  stream = new StreamInputDispatcher(window)
})
afterEach(() => {
  stream?.dispose(); stream = null
  cleanup()
  delete (document as { elementFromPoint?: unknown }).elementFromPoint
  delete window.__TAURI_INTERNALS__
})

/** Where each press sent was stamped as made: at the shack or through a stream. */
let origins: string[] = []
function mount(status: RemoteStationStatus, refuse?: (action: RemoteStationAction) => string | null) {
  const actions: RemoteStationAction[] = []
  origins = []
  window.__TAURI_INTERNALS__ = { invoke: (async (command: string, input?: unknown) => {
    if (command === 'get_remote_station_status') return status
    if (command === 'get_settings') return {}
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const { action, origin } = input as { action: RemoteStationAction; origin: string }
    if (action.type !== 'refresh') { actions.push(action); origins.push(origin) }
    const refused = refuse?.(action)
    if (refused) throw refused
    return status
  }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<RemoteStation />)
  return actions
}
const list = async () => (await screen.findByRole('heading', { name: 'Devices' })).closest('section')!
const person = (name: string) => screen.getByRole('group', { name })
const rowOf = (name: string) => screen.getAllByText(name).find(n => n.classList.contains('remote-native-browser-name'))!.closest('li')!
const names = (group: HTMLElement) => [...group.querySelectorAll('.remote-native-browser-name')].map(n => n.textContent)
const badges = (row: HTMLElement) => [...row.querySelectorAll('.remote-device-road')].map(n => n.textContent)
const stateOf = (row: HTMLElement) => row.querySelector('.remote-native-browser-state')!.textContent

it('groups by person: you first, then each person invited (with no devices too), then the devices not known whose', async () => {
  mount(station())
  const section = await list()
  expect([...section.querySelectorAll('[role=group]')].map(g => g.getAttribute('aria-label')))
    .toEqual(['You', GUEST, IDLE_GUEST, EN['remote.devices.unknown']])
  expect(names(person('You'))).toEqual(['Chrome on Windows', 'Shack laptop', 'Firefox on Android', 'Travel laptop', 'Den PC'])
  expect(names(person(EN['remote.devices.unknown']))).toEqual(['Edge on Windows'])
  expect(within(person(IDLE_GUEST)).getByText('No devices yet.')).toBeTruthy()
  // One row per device: the laptop on both roads is one row, not one in each of two lists.
  expect(screen.getAllByText('Shack laptop').filter(n => n.classList.contains('remote-native-browser-name')).length).toBe(1)
  // The old lists are gone.
  for (const old of ['Your browsers', 'Your computers', 'Paired computers', 'Browser approvals'])
    expect(screen.queryByRole('heading', { name: old }), old).toBeNull()
})

it('a guest’s devices appear under the guest, on either road, with the guest’s role', async () => {
  mount(station())
  await list()
  const guest = person(GUEST)
  expect(names(guest)).toEqual(['Pixel', 'Guest notebook'])
  expect(within(guest).getByText('Listen')).toBeTruthy()
  expect(stateOf(rowOf('Pixel'))).toBe('Listen only.')
  expect(within(person('You')).queryByText('Pixel')).toBeNull()
})

it('badges each row with how it connects', async () => {
  mount(station())
  await list()
  expect(badges(rowOf('Shack laptop'))).toEqual(['Home network', 'Internet'])
  expect(badges(rowOf('Den PC'))).toEqual(['Home network'])
  expect(badges(rowOf('Pixel'))).toEqual(['Internet'])
  expect(badges(rowOf('Chrome on Windows'))).toEqual(['Browser'])
})

it('says each state in plain words: live, held with the time left, allowed to transmit, waiting', async () => {
  mount(station())
  await list()
  expect(stateOf(rowOf('Shack laptop'))).toBe('Live now. Allowed to transmit.')
  expect(stateOf(rowOf('Den PC'))).toBe('Allowed to transmit.')
  expect(stateOf(rowOf('Travel laptop'))).toBe('Listening and tuning only. Transmit waits for you to press Allow to transmit, or about 10 min.')
  expect(stateOf(rowOf('Firefox on Android'))).toBe(EN['remote.native.asks'])
  expect(stateOf(rowOf('Chrome on Windows'))).toBe('Approved.')
  cleanup()
  // CONTROL: held with no time sent (no confirmed notice to the owner): no time is promised.
  const noTime = station()
  noTime.deviceRows = noTime.deviceRows!.map(row => ({ ...row, holdLeft: null }))
  mount(noTime)
  await list()
  expect(stateOf(rowOf('Travel laptop'))).toBe(EN['remote.account.waitingToTransmit'])
})

// Each button against the action the list it came from sent (RemoteStation.tsx and LanStation.tsx before
// the one list): Your browsers and Your computers' Approve, Allow to transmit and Remove; Paired computers'
// Remove; People's Remove; Advanced ▸ Browser approvals' station controls.
it('every button sends exactly what the old list’s button sent', async () => {
  const actions = mount(station())
  await list()
  const press = async (row: HTMLElement, name: string) => {
    fireEvent.click(within(row).getByRole('button', { name }))
    await act(async () => { await Promise.resolve() })
  }
  await press(rowOf('Firefox on Android'), 'Approve')
  await press(rowOf('Travel laptop'), 'Allow to transmit')
  await press(rowOf('Chrome on Windows'), 'Remove')
  await press(rowOf('Shack laptop'), 'Remove')
  await press(rowOf('Den PC'), 'Remove')
  fireEvent.click(within(person(GUEST)).getAllByRole('button', { name: 'Remove' })[0])
  await act(async () => { await Promise.resolve() })
  fireEvent.click(within(rowOf('Chrome on Windows')).getByRole('button', { name: 'Revoke station controls' }))
  await waitFor(() => expect(actions).toEqual([
    { type: 'device', deviceId: ASKING, approve: true, key: key('d'), kind: 'browser' },
    { type: 'allowTransmit', deviceId: HELD },
    { type: 'device', deviceId: OWN_BROWSER, approve: false },
    // On both roads: the Remove Your computers had, which ends its road on this network too.
    { type: 'device', deviceId: OWN_LAPTOP, approve: false },
    { type: 'lanRevoke', deviceId: DEN_LAN },
    { type: 'uninvite', email: GUEST },
    { type: 'stationPermission', deviceId: OWN_BROWSER, allow: false },
  ]))
  // One name per act: Approve only on what waits to be approved, Allow to transmit only on what is held.
  expect(within(rowOf('Den PC')).queryByRole('button', { name: 'Approve' })).toBeNull()
  expect(within(rowOf('Shack laptop')).queryByRole('button', { name: 'Allow to transmit' })).toBeNull()
  expect(within(rowOf('Firefox on Android')).queryByRole('button', { name: 'Remove' }), 'asking: as before, no Remove').toBeNull()
})

it('a computer approved by sign-in with a key not pinned is offered Approve, as Your computers offered it', async () => {
  const status = station()
  status.pinnedDevices = status.pinnedDevices!.filter(id => id !== HELD)
  status.heldDevices = []
  const actions = mount(status)
  await list()
  const row = rowOf('Travel laptop')
  expect(row.textContent).toContain(EN['remote.native.computerKeyNotPinned'])
  expect(row.textContent).toContain(`Key ${short(key('e'))}`)
  fireEvent.click(within(row).getByRole('button', { name: 'Approve' }))
  await waitFor(() => expect(actions).toEqual([{ type: 'device', deviceId: HELD, approve: true, key: key('e'), kind: 'computer' }]))
})

it('a Remove on this network’s list is refused through a stream and said why; at the shack it goes', async () => {
  const actions = mount(station())
  await list()
  under = within(rowOf('Den PC')).getByRole('button', { name: 'Remove' })
  act(() => {
    const pointer = (action: 'down' | 'up') => ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
      modifiers: 0, pointerType: 'mouse', clicks: 1 }) as Parameters<StreamInputDispatcher['handle']>[0]
    stream!.handle(pointer('down')); stream!.handle(pointer('up'))
  })
  await act(async () => { await Promise.resolve() })
  expect(actions).toEqual([])
  expect(screen.getByRole('alert').textContent).toBe('Only at the station itself: this can’t be done through a stream.')
  fireEvent.click(within(rowOf('Den PC')).getByRole('button', { name: 'Remove' }))
  await waitFor(() => expect(actions).toEqual([{ type: 'lanRevoke', deviceId: DEN_LAN }]))
})

/** A click made over the stream's picture, on `button`. */
const streamedClick = (button: Element) => act(() => {
  under = button
  const pointer = (action: 'down' | 'up') => ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
    modifiers: 0, pointerType: 'mouse', clicks: 1 }) as Parameters<StreamInputDispatcher['handle']>[0]
  stream!.handle(pointer('down')); stream!.handle(pointer('up'))
})
/** Enter pressed over the stream's picture, on `button` holding the keyboard. */
const streamedEnter = (button: HTMLElement) => act(() => {
  button.focus()
  for (const action of ['down', 'up'] as const)
    stream!.handle({ type: 'key', action, key: 'Enter', code: 'Enter', modifiers: 0, repeat: false } as Parameters<StreamInputDispatcher['handle']>[0])
})

it('ONLY THE SHACK DECIDES: every Devices button pressed through a stream, by a click or by Enter, sends nothing and says why; at the shack each goes', async () => {
  const actions = mount(station())
  await list()
  const buttons = () => [
    within(rowOf('Firefox on Android')).getByRole('button', { name: 'Approve' }),
    within(rowOf('Travel laptop')).getByRole('button', { name: 'Allow to transmit' }),
    within(rowOf('Chrome on Windows')).getByRole('button', { name: 'Remove' }),
    within(rowOf('Shack laptop')).getByRole('button', { name: 'Remove' }),
    within(rowOf('Den PC')).getByRole('button', { name: 'Remove' }),
    within(person(GUEST)).getAllByRole('button', { name: 'Remove' })[0],
    within(rowOf('Chrome on Windows')).getByRole('button', { name: 'Revoke station controls' }),
  ]
  for (const button of buttons()) {
    await streamedClick(button)
    await streamedEnter(button)
    await act(async () => { await Promise.resolve() })
    expect(actions, button.textContent!).toEqual([])
    expect(screen.getByRole('alert').textContent).toBe(EN['remote.lan.atShackOnly'])
  }
  // CONTROL: the same buttons pressed at the shack, with a stream still connected, each send, stamped as the shack's.
  for (const button of buttons()) {
    fireEvent.click(button)
    await act(async () => { await Promise.resolve() })
  }
  await waitFor(() => expect(actions).toHaveLength(7))
  expect(origins).toEqual(Array(7).fill('shack'))
})

// The card's other buttons take the same road: refused through a stream and said in the card's alert.
const cardPresses: [string, () => RemoteStationStatus, () => HTMLElement, RemoteStationAction][] = [
  ['Turn off Remote', station, () => screen.getByRole('button', { name: EN['remote.disable'] }), { type: 'disable' }],
  ['Turn on Remote', () => ({ ...station(), phase: 'disabled' }), () => screen.getByRole('button', { name: EN['remote.enable'] }), { type: 'enable' }],
  ['Revoke station access', station, () => screen.getByRole('button', { name: EN['remote.revokeStation'] }), { type: 'forget' }],
  ['Invite', station, () => {
    fireEvent.change(screen.getByLabelText(EN['remote.account.email']), { target: { value: 'n0call@example.test' } })
    return screen.getByRole('button', { name: EN['remote.account.invite'] })
  }, { type: 'invite', email: 'n0call@example.test', role: 'operate' }],
  ['Sign in', () => ({ ...station(), phase: 'unpaired' }), () => {
    fireEvent.change(screen.getByLabelText(EN['remote.stationName']), { target: { value: 'Shack' } })
    return screen.getByRole('button', { name: EN['remote.account.signIn'] })
  }, { type: 'signIn', name: 'Shack' }],
]
for (const [name, status, control, sent] of cardPresses) {
  it(`ONLY THE SHACK DECIDES: ${name} pressed through a stream sends nothing and says why; at the shack it goes`, async () => {
    const actions = mount(status())
    await list()
    const button = control()
    await streamedClick(button)
    await streamedEnter(button)
    await act(async () => { await Promise.resolve() })
    expect(actions).toEqual([])
    expect(screen.getByRole('alert').textContent).toBe(EN['remote.lan.atShackOnly'])
    // CONTROL: the same press at the shack, with the stream still connected, goes, stamped as the shack's.
    fireEvent.click(button)
    await waitFor(() => expect(actions).toEqual([sent]))
    expect(origins).toEqual(['shack'])
  })
}

it('the station’s own refusal of a streamed press is said in the same words', async () => {
  mount(station(), action => action.type === 'stationPermission' ? 'atShackOnly' : null)
  await list()
  fireEvent.click(within(rowOf('Chrome on Windows')).getByRole('button', { name: 'Revoke station controls' }))
  expect((await screen.findByRole('alert')).textContent).toBe(EN['remote.lan.atShackOnly'])
})

it('a refused Remove on this network’s list says what it needs, as the network card said it', async () => {
  mount(station(), action => action.type === 'lanRevoke' ? 'removeElsewhere' : null)
  await list()
  fireEvent.click(within(rowOf('Den PC')).getByRole('button', { name: 'Remove' }))
  expect((await screen.findByRole('alert')).textContent).toBe(EN['remote.removeElsewhere'])
})

it('the details under each row hold what the old lists showed: keys, ids, platform, how it was added, the approval', async () => {
  mount(station())
  await list()
  const details = (name: string) => rowOf(name).querySelector('details.remote-device-details')!
  const laptop = details('Shack laptop')
  expect((laptop as HTMLDetailsElement).open, 'folded').toBe(false)
  for (const said of [`Key ${short(key('1'))}`, `Key ${short(key('b'))}`, LAPTOP_LAN.slice(-6), OWN_LAPTOP.slice(-6),
    'Nexus on Windows', 'Home network: paired with this station.', EN['remote.account.addedByAccount'], 'Approved until you remove it.'])
    expect(laptop.textContent, said).toContain(said)
  const browser = details('Chrome on Windows')
  for (const said of [`Key ${short(key('a'))}`, OWN_BROWSER.slice(-6), 'Approved at this station.'])
    expect(browser.textContent, said).toContain(said)
  // The keys of a device that waits for nothing are only in its details.
  const row = rowOf('Chrome on Windows').cloneNode(true) as HTMLElement
  row.querySelector('details')!.remove()
  expect(row.textContent).not.toContain(short(key('a')))
})

it('a station on this network alone, never signed in, still lists its paired computers', async () => {
  mount({ phase: 'unpaired', origin: 'https://remote.invalid', stationId: null, accountId: null, pairingId: null, pairingCode: null,
    expiresAt: null, error: null, devices: [], lan })
  await list()
  expect(names(person('You'))).toEqual(['Shack laptop', 'Den PC', 'Guest notebook'])
})

// @vitest-environment jsdom
// The question at the shack (the operator, 2026-10-02, "Nexus asks you, with the key": "When a browser
// asks to stream, Nexus at the shack pops up "Let Chrome on Windows stream this station? Key XXXX XXXX"
// with Approve / Deny ... No hunting through Settings."). Approve is Settings' approve, and the pop-up
// must never approve by default or on Enter alone.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { APPROVAL_ARM_MS, APPROVAL_POLL_MS, RemoteApprovalPrompt } from './RemoteApprovalPrompt'
import { StreamInputDispatcher } from './stream-input'
import type { RemoteStationAction, RemoteStationStatus } from './types'
import { EN } from '../i18n/en'

const KEY = 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90'
// The key as both ends show it: the first 128 bits of the fingerprint, eight groups of four (S3-L1).
const SHORT = 'A1B2 C3D4 E5F6 0718 293A 4B5C 6D7E 8F90'
const TITLE = 'Let Chrome on Windows stream this station?'

type Device = RemoteStationStatus['devices'][number]
function station(devices: Device[], extra: Partial<RemoteStationStatus> = {}): RemoteStationStatus {
  return { phase: 'connected', origin: 'https://remote-staging.hamradiotools.io', stationId: crypto.randomUUID(), accountId: crypto.randomUUID(),
    pairingId: null, pairingCode: null, expiresAt: null, error: null, devices, pinnedDevices: [], ...extra }
}
const asking = (extra: Partial<Device> = {}): Device =>
  ({ id: crypto.randomUUID(), name: 'Chrome on Windows', approved: 0, expiresAt: Date.now() + 600000, key: KEY, ...extra })

let status: RemoteStationStatus
const commands: string[] = []
const actions: RemoteStationAction[] = []
beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: false })
  commands.length = 0; actions.length = 0
  window.__TAURI_INTERNALS__ = { invoke: (async (command: string, input?: unknown) => {
    commands.push(command)
    if (command === 'get_remote_station_status') return status
    if (command !== 'remote_station_action') throw new Error('unexpectedCommand')
    const action = (input as { action: RemoteStationAction }).action
    // The poll's own Refresh is how it reads the requests; the answers are what these tests are about.
    if (action.type !== 'refresh') actions.push(action)
    if (action.type === 'allowTransmit') status = { ...status, heldDevices: (status.heldDevices ?? []).filter(id => id !== action.deviceId) }
    if (action.type === 'device') {
      const id = action.deviceId
      status = { ...status, devices: action.approve ? status.devices.map(d => d.id === id ? { ...d, approved: 1 } : d) : status.devices.filter(d => d.id !== id),
        pinnedDevices: action.approve && action.key ? [...(status.pinnedDevices ?? []), id] : status.pinnedDevices }
    }
    return status
  }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
})
afterEach(() => { cleanup(); vi.useRealTimers(); delete window.__TAURI_INTERNALS__ })
const flush = () => act(async () => { await vi.advanceTimersByTimeAsync(0) })
const wait = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms) })
const dialog = () => screen.queryByRole('dialog')
const approve = () => screen.getByRole('button', { name: 'Approve' }) as HTMLButtonElement
const deny = () => screen.getByRole('button', { name: 'Deny' }) as HTMLButtonElement

it('a browser asking to stream: the shack asks with its name and the key the site shows, and Approve is Settings\' approve', async () => {
  const device = asking()
  status = station([device])
  render(<RemoteApprovalPrompt />)
  await flush()
  const box = dialog()!
  expect(box, 'the question is up without opening Settings').toBeTruthy()
  // Screen readers get the question as the dialog's name and the key in its description.
  expect(screen.getByRole('dialog', { name: TITLE })).toBe(box)
  expect(box.getAttribute('aria-describedby') && document.getElementById(box.getAttribute('aria-describedby')!)?.textContent).toContain(SHORT)
  expect(box.querySelector('.remote-approval-key')?.textContent).toBe(SHORT)
  // What Approve lets it do, said plainly (the operator, 2026-10-03: a streaming browser is the operator at the shack).
  expect(box.textContent).toMatch(/asks to stream this station and operate it as you would here, transmit included/)
  await wait(APPROVAL_ARM_MS)
  fireEvent.click(approve())
  await flush()
  expect(actions).toEqual([{ type: 'device', deviceId: device.id, approve: true, key: KEY, kind: 'browser' }])
  expect(dialog(), 'answered: it goes').toBeNull()
})

it('NEVER APPROVES BY DEFAULT: Deny holds the keyboard when it opens, and Approve takes no click for its first second', async () => {
  status = station([asking()])
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(dialog()).toBeTruthy()
  // Enter alone presses whatever holds the keyboard, and that is Deny, never Approve.
  expect(document.activeElement, 'Deny has the keyboard').toBe(deny())
  expect(document.activeElement).not.toBe(approve())
  // A click meant for what was under the pointer when it appeared reaches nothing.
  expect(approve().disabled).toBe(true)
  fireEvent.click(approve())
  await flush()
  expect(actions, 'no approval from a click in the first second').toEqual([])
  await wait(APPROVAL_ARM_MS - 10)
  expect(approve().disabled).toBe(true)
  await wait(10)
  expect(approve().disabled, 'then it can be pressed, deliberately').toBe(false)
})

it('Escape or a click outside closes it and answers nothing; it does not come back for the same browser and key', async () => {
  status = station([asking()])
  render(<RemoteApprovalPrompt />)
  await flush()
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  await flush()
  expect(dialog()).toBeNull()
  expect(actions, 'Escape is not an answer').toEqual([])
  await wait(APPROVAL_POLL_MS)
  expect(dialog(), 'not asked again this session').toBeNull()
  // Control: a different browser asking is asked about.
  status = station([...status.devices, asking({ name: 'Firefox on Android', key: 'b'.repeat(64) })], { stationId: status.stationId })
  await wait(APPROVAL_POLL_MS)
  expect(screen.getByRole('dialog', { name: 'Let Firefox on Android stream this station?' })).toBeTruthy()
})

it('Deny refuses a waiting request at the service; for a browser approved before, it leaves that approval exactly as it is', async () => {
  const waiting = asking()
  status = station([waiting])
  render(<RemoteApprovalPrompt />)
  await flush()
  fireEvent.click(deny())
  await flush()
  expect(actions).toEqual([{ type: 'device', deviceId: waiting.id, approve: false }])
  expect(dialog()).toBeNull()
  cleanup(); actions.length = 0
  // Approved before its key was pinned: it cannot stream until approved again here, so it is asked about.
  const before = asking({ approved: 1, name: 'Chrome on Windows', expiresAt: Date.now() + 30 * 86400000 })
  status = station([before], { pinnedDevices: [] })
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(dialog()?.textContent).toMatch(/was approved before, but it can’t stream until you approve it again here/)
  expect(dialog()?.textContent).toMatch(/When it streams, it can operate this station as you would here, transmit included/)
  fireEvent.click(deny())
  await flush()
  expect(actions, 'its approval is not touched').toEqual([])
  expect(dialog()).toBeNull()
})

it('asks about nothing it should not: a pinned browser, a request with no key to compare, or a station with Remote off (which asks the service nothing)', async () => {
  const pinned = asking({ approved: 1 })
  status = station([pinned, asking({ key: null, name: 'Old page' })], { pinnedDevices: [pinned.id] })
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(dialog()).toBeNull()
  expect(commands, 'Remote on: it reads the requests the way Refresh does').toContain('remote_station_action')
  cleanup(); commands.length = 0
  status = station([asking()], { phase: 'disabled' })
  render(<RemoteApprovalPrompt />)
  await flush()
  await wait(APPROVAL_POLL_MS)
  expect(commands.filter(c => c === 'remote_station_action'), 'Remote off: no request to the service').toEqual([])
  // Positive control for that absence: the same station with Remote on is asked.
  status = { ...status, phase: 'connected' }
  await wait(APPROVAL_POLL_MS)
  expect(commands).toContain('remote_station_action')
  expect(dialog()).toBeTruthy()
})

it('a computer running Nexus asking to connect: the shack names it as one, with its platform and its key in eight groups, and Approve pins that key', async () => {
  const device = asking({ name: 'Shack laptop', kind: 'computer', platform: 'macos' })
  status = station([device])
  render(<RemoteApprovalPrompt />)
  await flush()
  const box = screen.getByRole('dialog', { name: 'Let Shack laptop operate this station?' })
  expect(box.getAttribute('aria-describedby') && document.getElementById(box.getAttribute('aria-describedby')!)?.textContent)
    .toBe(`Nexus on macOS, signed in to your Remote account, asks to operate this station over the internet as you would here, transmit included. Approve it only if this key matches the one Nexus shows on that computer: ${SHORT}.`)
  expect(box.querySelector('.remote-approval-key')?.textContent).toBe(SHORT)
  expect(box.textContent, 'never the browser\'s words').not.toMatch(/stream/)
  await wait(APPROVAL_ARM_MS)
  fireEvent.click(approve())
  await flush()
  // The kind it was shown as goes with the press, as its key does: the shack grants and pins as shown.
  expect(actions).toEqual([{ type: 'device', deviceId: device.id, approve: true, key: KEY, kind: 'computer' }])
  expect(dialog(), 'answered: it goes').toBeNull()
})

it('a computer approved before its key was pinned is asked again, by name, platform and key, as a browser is', async () => {
  const device = asking({ name: 'Shack laptop', kind: 'computer', platform: 'linux', approved: 1, expiresAt: Date.now() + 86400000 })
  status = station([device], { pinnedDevices: [] })
  render(<RemoteApprovalPrompt />)
  await flush()
  const box = screen.getByRole('dialog', { name: 'Let Shack laptop operate this station?' })
  expect(document.getElementById(box.getAttribute('aria-describedby')!)?.textContent)
    .toBe(`Nexus on Linux was approved before, but it can’t connect until you approve it again here. Connected, it can operate this station as you would here, transmit included. Approve it only if this key matches the one Nexus shows on that computer: ${SHORT}.`)
  // Deny leaves its approval exactly as it is, as for a browser.
  fireEvent.click(deny())
  await flush()
  expect(actions).toEqual([])
  expect(dialog()).toBeNull()
})

// A device added by signing in to the account is listed approved by the service, and the station grants it nothing until
// it is approved here (`remote_service::account::GRANTS_ON_SIGN_IN`). The station says so (`approveAtStation`), and the
// question is the one a new device is asked, never "approved before": the station never approved it.
it('a device added by signing in, waiting for Approve here, is asked as a new one and never as approved before', async () => {
  const computer = asking({ name: 'DESKTOP-7Q', kind: 'computer', platform: 'windows', approved: 1, addedBy: 'account' })
  status = station([computer], { account: { signingIn: false, added: [], unconfirmed: { [computer.id]: 'approveAtStation' }, removals: [], people: [] } })
  render(<RemoteApprovalPrompt />)
  await flush()
  const box = screen.getByRole('dialog', { name: 'Let DESKTOP-7Q operate this station?' })
  expect(document.getElementById(box.getAttribute('aria-describedby')!)?.textContent)
    .toBe(`Nexus on Windows, signed in to your Remote account, asks to operate this station over the internet as you would here, transmit included. Approve it only if this key matches the one Nexus shows on that computer: ${SHORT}.`)
  await wait(APPROVAL_ARM_MS)
  fireEvent.click(approve())
  await flush()
  expect(actions).toEqual([{ type: 'device', deviceId: computer.id, approve: true, key: KEY, kind: 'computer' }])
  cleanup(); actions.length = 0
  const browser = asking({ approved: 1, addedBy: 'account' })
  status = station([browser], { account: { signingIn: false, added: [], unconfirmed: { [browser.id]: 'approveAtStation' }, removals: [], people: [] } })
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(dialog()?.textContent).toMatch(/A browser signed in to your Remote account asks to stream this station/)
  expect(dialog()?.textContent, 'never approved here').not.toMatch(/approved before/)
})

it('a computer asking: the question shows this station\'s key too, read out in the same call; a browser\'s does not', async () => {
  const stationKey = '0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0'
  const shown = 'This station’s key: 0F1E 2D3C 4B5A 6978 8796 A5B4 C3D2 E1F0'
  status = station([asking({ name: 'Shack laptop', kind: 'computer', platform: 'windows' })], { stationKey })
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(screen.getByRole('dialog').textContent).toContain(shown)
  cleanup()
  status = station([asking()], { stationKey })
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(screen.getByRole('dialog').textContent).not.toContain(shown)
})

// A device added by signing in has control at once but is held from transmit until it is allowed here. The shack
// asks about it the way it asks about a new one, and the answer is the card's own Allow to transmit: the same
// shack-only action, under the same name. Not now leaves it held.
const held = (extra: Partial<Device> = {}): Device => asking({ name: 'DESKTOP-7Q', kind: 'computer', platform: 'windows', approved: 1,
  untilRemoved: 1, role: 'operate', addedBy: 'account', ...extra })
const allow = () => screen.getByRole('button', { name: EN['remote.account.allowTransmit'] }) as HTMLButtonElement
const notNow = () => screen.getByRole('button', { name: EN['remote.approval.notNow'] }) as HTMLButtonElement

it('a held sign-in device: the shack asks to allow it to transmit, and Allow to transmit is the card\'s own shack-only action', async () => {
  const device = held()
  status = station([device], { pinnedDevices: [device.id], heldDevices: [device.id] })
  render(<RemoteApprovalPrompt />)
  await flush()
  const box = screen.getByRole('dialog', { name: 'Allow DESKTOP-7Q to transmit?' })
  expect(document.getElementById(box.getAttribute('aria-describedby')!)?.textContent)
    .toBe(`Nexus on Windows signed in to your Remote account. It can listen and tune now, and transmit waits for you. Allow it only if this key matches the one Nexus shows on that computer: ${SHORT}.`)
  expect(box.querySelector('.remote-approval-key')?.textContent).toBe(SHORT)
  // One name for the act, here and on the card; never the pairing's Approve.
  expect(screen.queryByRole('button', { name: 'Approve' })).toBeNull()
  expect(box.textContent).not.toMatch(/approv/i)
  // Never by default: Not now holds the keyboard, and Allow takes no click for its first second.
  expect(document.activeElement).toBe(notNow())
  expect(allow().disabled).toBe(true)
  await wait(APPROVAL_ARM_MS)
  fireEvent.click(allow())
  await flush()
  expect(actions).toEqual([{ type: 'allowTransmit', deviceId: device.id }])
  expect(dialog(), 'answered: it goes').toBeNull()
})

/** A click made over the stream's picture of the shack, on `button`. */
function streamedClick(button: Element) {
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => button })
  const stream = new StreamInputDispatcher(window)
  try {
    act(() => {
      for (const action of ['down', 'up'] as const)
        stream.handle({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0, modifiers: 0,
          pointerType: 'mouse', clicks: 1 } as Parameters<StreamInputDispatcher['handle']>[0])
    })
  } finally { stream.dispose(); delete (document as { elementFromPoint?: unknown }).elementFromPoint }
}

it('ONLY THE SHACK DECIDES: Allow to transmit or Approve pressed through a stream sends nothing, says why, and the question stays', async () => {
  const device = held()
  status = station([device], { pinnedDevices: [device.id], heldDevices: [device.id] })
  render(<RemoteApprovalPrompt />)
  await flush()
  await wait(APPROVAL_ARM_MS)
  expect(allow().disabled, 'premise: armed').toBe(false)
  streamedClick(allow())
  await flush()
  expect(actions, 'a held device cannot allow itself').toEqual([])
  expect(screen.getByRole('alert').textContent).toBe(EN['remote.lan.atShackOnly'])
  expect(screen.getByRole('dialog', { name: 'Allow DESKTOP-7Q to transmit?' }), 'still asked').toBeTruthy()
  // CONTROL: the same press at the shack goes.
  fireEvent.click(allow())
  await flush()
  expect(actions).toEqual([{ type: 'allowTransmit', deviceId: device.id }])

  cleanup(); actions.length = 0
  const waiting = asking()
  status = station([waiting])
  render(<RemoteApprovalPrompt />)
  await flush()
  await wait(APPROVAL_ARM_MS)
  streamedClick(approve())
  await flush()
  expect(actions, 'nothing approved through a stream').toEqual([])
  expect(screen.getByRole('alert').textContent).toBe(EN['remote.lan.atShackOnly'])
  streamedClick(deny())
  await flush()
  expect(actions, 'nor denied').toEqual([])
  expect(dialog(), 'the question stays').toBeTruthy()
})

it('a held browser is asked about in a browser\'s words', async () => {
  const device = held({ name: 'Owner phone', kind: 'browser', platform: null })
  status = station([device], { pinnedDevices: [device.id], heldDevices: [device.id] })
  render(<RemoteApprovalPrompt />)
  await flush()
  const box = screen.getByRole('dialog', { name: 'Allow Owner phone to transmit?' })
  expect(document.getElementById(box.getAttribute('aria-describedby')!)?.textContent)
    .toBe(`A browser signed in to your Remote account. It can listen and tune now, and transmit waits for you. Allow it only if this key matches the one the browser shows: ${SHORT}.`)
})

it('Not now, Escape or a click outside leave a held device held: nothing is sent, and it is not asked again this session', async () => {
  const device = held()
  status = station([device], { pinnedDevices: [device.id], heldDevices: [device.id] })
  render(<RemoteApprovalPrompt />)
  await flush()
  fireEvent.click(notNow())
  await flush()
  expect(dialog()).toBeNull()
  expect(actions, 'Not now sends nothing: the device stays held').toEqual([])
  await wait(APPROVAL_POLL_MS)
  expect(dialog()).toBeNull()
  cleanup()
  render(<RemoteApprovalPrompt />)
  await flush()
  fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
  await flush()
  expect(dialog()).toBeNull()
  expect(actions).toEqual([])
})

it('asks to allow only where the card offers Allow to transmit: never a device not held, unconfirmed or removed here', async () => {
  const free = held({ name: 'Free' }), unconfirmed = held({ name: 'Unconfirmed', key: 'c'.repeat(64) }),
    removed = held({ name: 'Removed', key: 'd'.repeat(64) })
  status = station([free, unconfirmed, removed], { pinnedDevices: [free.id, unconfirmed.id, removed.id], heldDevices: [unconfirmed.id, removed.id],
    account: { signingIn: false, added: [], unconfirmed: { [unconfirmed.id]: 'tokenAudience' }, removals: [removed.id], people: [] } })
  render(<RemoteApprovalPrompt />)
  await flush()
  expect(dialog()).toBeNull()
  // Positive control: the same station holding Free is asked about it.
  status = { ...status, heldDevices: [free.id, unconfirmed.id, removed.id] }
  await wait(APPROVAL_POLL_MS)
  expect(screen.getByRole('dialog', { name: 'Allow Free to transmit?' })).toBeTruthy()
})

// The remote PC's own line while it is held (shown by the client banner): it says it can listen and tune, and names the
// shack's act by the pop-up's name. It offers no time in any language: the station sends none, and a hold whose owner
// email was never confirmed never lifts by itself, so no time may ever read as a coming unlock.
it('the held remote PC is told it listens and tunes, and that transmit waits for Allow to transmit at the shack, with no time', async () => {
  const { DE } = await import('../i18n/de'), { ES } = await import('../i18n/es'), { FR } = await import('../i18n/fr'), { JA } = await import('../i18n/ja')
  expect(EN['client.ptt.refusal.onHold']).toContain(EN['remote.account.allowTransmit'])
  expect(EN['client.ptt.refusal.onHold']).toMatch(/listening and tuning/i)
  for (const catalog of [EN, DE, ES, FR, JA] as Record<string, unknown>[]) {
    const line = catalog['client.ptt.refusal.onHold'] as string
    expect(line).toContain(catalog['remote.account.allowTransmit'] as string)
    expect(line, 'no time, no countdown').not.toMatch(/\d|\{\{/)
  }
})

// @vitest-environment jsdom
// The question at the shack (the operator, 2026-10-02, "Nexus asks you, with the key": "When a browser
// asks to stream, Nexus at the shack pops up "Let Chrome on Windows stream this station? Key XXXX XXXX"
// with Approve / Deny ... No hunting through Settings."). Approve is Settings' approve with its transmit
// tick off, and the pop-up must never approve by default or on Enter alone.
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { APPROVAL_ARM_MS, APPROVAL_POLL_MS, RemoteApprovalPrompt } from './RemoteApprovalPrompt'
import type { RemoteStationAction, RemoteStationStatus } from './types'

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

it('a browser asking to stream: the shack asks with its name and the key the site shows, and Approve is Settings\' approve with transmit off', async () => {
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
  expect(actions).toEqual([{ type: 'device', deviceId: device.id, approve: true, transmit: false, key: KEY }])
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

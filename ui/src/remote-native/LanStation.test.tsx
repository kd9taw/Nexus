// @vitest-environment jsdom
//
// Remote over this network's card at the shack (Settings ▸ Station ▸ Remote access).
//
// ONLY AT THE SHACK (the operator's ruling of 2026-10-04): turning it on or off, making a code,
// removing a computer and resetting the key are refused while the press comes through the stream.
// The presses here come from the REAL stream dispatcher, so the mark under test is the one the
// stream makes; each refusal is paired with the same press made at the shack, which goes through.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { LanStation } from './LanStation'
import { StreamInputDispatcher } from './stream-input'
import type { LanStatus, RemoteStationStatus } from './types'

const { getRemoteStationStatus, remoteStationAction } = vi.hoisted(() => ({
  getRemoteStationStatus: vi.fn(),
  remoteStationAction: vi.fn(),
}))
vi.mock('../api', () => ({ getRemoteStationStatus, remoteStationAction }))

const COMPUTER = '8a1f0c2e-77d3-8b41-9c0a-5e2b6d4f1a30'
const KEY = 'ab'.repeat(32)
const DEVICE_KEY = '0123456789abcdef'.repeat(4)

const status = (lan: LanStatus): RemoteStationStatus => ({
  phase: 'unpaired', origin: 'https://remote.invalid', stationId: null, accountId: null, pairingId: null,
  pairingCode: null, expiresAt: null, devices: [], error: null, lan,
})
const listening: LanStatus = {
  on: true, listening: '192.168.1.20:42075', key: KEY,
  devices: [{ id: COMPUTER, name: 'Den PC', key: DEVICE_KEY }],
}
const pairing: LanStatus = { ...listening, pairing: { code: '0123456789abcdef', closesAt: 0 } }

// jsdom never lays out: `elementFromPoint` does not exist. Each press says what is under the pointer.
let under: Element | null = null
let stream: StreamInputDispatcher | null = null
beforeEach(() => {
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
  stream = new StreamInputDispatcher(window)
})
afterEach(() => {
  stream?.dispose()
  stream = null
  cleanup()
  delete (document as { elementFromPoint?: unknown }).elementFromPoint
  getRemoteStationStatus.mockReset()
  remoteStationAction.mockReset()
})

const pointer = (action: 'down' | 'up') =>
  ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
    modifiers: 0, pointerType: 'mouse', clicks: 1 })
/** A press and release over `element`, as the stream delivers them. */
const pressedThroughStream = async (element: Element) => {
  under = element
  act(() => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
  await act(async () => { await Promise.resolve() })
}
const pressedAtShack = async (element: Element) => {
  fireEvent.click(element)
  await act(async () => { await Promise.resolve() })
}

async function showing(lan: LanStatus) {
  getRemoteStationStatus.mockResolvedValue(status(lan))
  remoteStationAction.mockResolvedValue(status(lan))
  render(<LanStation />)
  await screen.findByText(lan.on && lan.listening ? /Listening at/ : /Off|Not listening|Starting/)
}

describe('only at the shack: a press through the stream changes nothing', () => {
  const presses: [string, LanStatus, () => Element, unknown][] = [
    ['the switch', listening, () => screen.getByRole('switch'), { type: 'lanOff' }],
    ['Pair a computer', listening, () => screen.getByRole('button', { name: 'Pair a computer' }), { type: 'lanPair' }],
    ['Cancel pairing', pairing, () => screen.getByRole('button', { name: 'Cancel pairing' }), { type: 'lanCancel' }],
    ['Remove', listening, () => screen.getByRole('button', { name: 'Remove' }), { type: 'lanRevoke', deviceId: COMPUTER }],
    ['Reset network identity', listening, () => screen.getByRole('button', { name: 'Reset network identity' }), { type: 'lanReset' }],
  ]
  for (const [name, lan, control, sent] of presses) {
    it(`${name}: refused through the stream, and said why`, async () => {
      await showing(lan)
      await pressedThroughStream(control())
      expect(remoteStationAction, `${name} went to the station through the stream`).not.toHaveBeenCalled()
      expect(screen.getByRole('alert').textContent).toBe('Only at the station itself: this can’t be done through a stream.')
    })
    it(`CONTROL: ${name} pressed at the shack goes to the station`, async () => {
      await showing(lan)
      await pressedAtShack(control())
      expect(remoteStationAction).toHaveBeenCalledWith(sent)
      expect(screen.queryByText(/Only at the station itself/)).toBeNull()
    })
  }
  it('turning it on is refused through the stream too, and taken at the shack', async () => {
    await showing({ on: false, devices: [] })
    await pressedThroughStream(screen.getByRole('switch'))
    expect(remoteStationAction).not.toHaveBeenCalled()
    await pressedAtShack(screen.getByRole('switch'))
    expect(remoteStationAction).toHaveBeenCalledWith({ type: 'lanOn' })
  })
})

describe('what the card says', () => {
  it('names each reason it is off or not listening', async () => {
    const said: [LanStatus['reason'], boolean, string][] = [
      ['noKey', false, 'Off: this station has no network key. Unlock your operating system’s credential store, then turn this on again.'],
      ['endedAtShack', false, 'Off: remote control was ended here. Turn this on again when you want it.'],
      ['addressGone', false, 'Off: the address it listened at is no longer this computer’s. Turn this on again to listen where this computer is now.'],
      ['chooseAddress', true, 'Not listening: Nexus can’t tell which of this computer’s networks to use.'],
      ['portInUse', true, 'Not listening: another program is using its port.'],
      ['noNetwork', true, 'Not listening: this computer is not on a private network.'],
      ['unavailable', true, 'Not listening: it could not start on this computer.'],
    ]
    for (const [reason, on, sentence] of said) {
      await showing({ on, reason, devices: [] })
      expect(screen.getByRole('status').textContent, reason).toBe(sentence)
      cleanup()
    }
    await showing({ on: false, devices: [] })
    expect(screen.getByRole('status').textContent).toBe('Off.')
  })

  it('shows where it listens, the code with its warning, and each computer by name and key', async () => {
    await showing(pairing)
    expect(screen.getByRole('status').textContent).toBe('Listening at 192.168.1.20:42075. Only computers paired here can connect.')
    expect(screen.getByText('0123 4567 89ab cdef')).toBeTruthy()
    expect(screen.getByText(/Whoever types it in time can operate this station, transmit included/)).toBeTruthy()
    expect(screen.getByText('This station: 192.168.1.20:42075, key ABAB ABAB ABAB ABAB ABAB ABAB ABAB ABAB')).toBeTruthy()
    expect(screen.getByText('Den PC')).toBeTruthy()
    expect(screen.getByText('Key 0123 4567 89AB CDEF 0123 4567 89AB CDEF')).toBeTruthy()
    // One window at a time: no second Pair a computer while one is open.
    expect(screen.queryByRole('button', { name: 'Pair a computer' })).toBeNull()
  })

  it('says what a refused press needs, by name', async () => {
    await showing(listening)
    remoteStationAction.mockRejectedValueOnce('lanFull')
    await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
    expect(screen.getByRole('alert').textContent).toBe('Eight computers are paired already. Remove one to pair another.')
    remoteStationAction.mockRejectedValueOnce('credentialStoreUnavailable')
    await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
    expect(screen.getByRole('alert').textContent).toMatch(/^Unlock your operating system’s credential store/)
  })
})

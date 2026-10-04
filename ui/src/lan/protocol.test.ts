// What the Stations on this network page is told, read strictly (`readTold`), and the small shapes
// it says itself: the code's shape before Pair is worth pressing, a key's grouped fingerprint, and
// its socket's address beside the page.
import { expect, it } from 'vitest'
import { codeShaped, groupedKey, readTold, socketUrl } from './protocol'

const STATION = { id: '60000000-0000-4000-8000-000000000001', addresses: ['192.168.1.20:42075'], key: 'ab'.repeat(32) }
const KEY = `3059301306072a8648ce3d020106082a8648ce3d03010703420004${'cd'.repeat(64)}`

it('reads every message this computer’s Nexus sends, and passes the station’s own through', () => {
  expect(readTold({ type: 'stations', stations: [STATION], computer: 'DEN-PC' })).toEqual({ type: 'stations', stations: [STATION], computer: 'DEN-PC', error: null })
  expect(readTold({ type: 'stations', stations: [], computer: '', error: 'storeUnavailable' })).toMatchObject({ error: 'storeUnavailable' })
  expect(readTold({ type: 'paired', station: STATION })).toEqual({ type: 'paired', station: STATION })
  expect(readTold({ type: 'pairRefused', reason: 'wrongCode' })).toEqual({ type: 'pairRefused', reason: 'wrongCode' })
  const road = { stationId: STATION.id, deviceId: '1d2b7c3a-4e5f-8a6b-9c7d-0e1f2a3b4c5d', sessionId: '50000000-0000-4000-8000-000000000002', stationKey: KEY, address: '192.168.1.20:42075' }
  expect(readTold({ type: 'connected', ...road })).toEqual({ type: 'connected', road })
  expect(readTold({ type: 'connectRefused', reason: 'keyChanged' })).toEqual({ type: 'connectRefused', reason: 'keyChanged' })
  expect(readTold({ type: 'closed', reason: 'stationLeft' })).toEqual({ type: 'closed', reason: 'stationLeft' })
  expect(readTold({ type: 'answerRefused', reason: 'stationKeyMismatch' })).toEqual({ type: 'answerRefused', reason: 'stationKeyMismatch' })
  for (const type of ['operationResponse', 'streamSignal', 'streamState', 'status']) {
    expect(readTold({ type, any: 'thing' })).toEqual({ type: 'station', message: { type, any: 'thing' } })
  }
})

it('refuses anything else: another word, a field too many or too few, a reason never given, a key or an id out of shape', () => {
  for (const told of [
    { type: 'runCommand' },
    { type: 'stations', stations: [STATION], computer: 'X', extra: 1 },
    { type: 'stations', stations: [{ ...STATION, pkcs8: '00' }], computer: 'X' },
    { type: 'stations', stations: [{ ...STATION, key: 'AB'.repeat(32) }], computer: 'X' },
    { type: 'paired' },
    { type: 'pairRefused', reason: 'somethingElse' },
    { type: 'connectRefused', reason: 'wrongCode' },
    { type: 'closed', reason: 'notController' },
    { type: 'answerRefused', reason: 'stationBusy' },
    { type: 'connected', stationId: STATION.id, deviceId: 'x', sessionId: STATION.id, stationKey: KEY, address: '192.168.1.20:42075' },
    { type: 'connected', stationId: STATION.id, deviceId: STATION.id, sessionId: STATION.id, stationKey: 'ab'.repeat(91), address: '192.168.1.20:42075' },
    null, 'stations', [],
  ]) {
    expect(() => readTold(told), JSON.stringify(told)).toThrow()
  }
})

it('takes a code of sixteen hexadecimal characters as typed, either case, with spaces anywhere', () => {
  for (const typed of ['0a1b2c3d4e5f6071', '0A1B 2C3D 4E5F 6071', ' 0a1b 2c3d4e5f6071 ']) expect(codeShaped(typed), typed).toBe(true)
  for (const typed of ['', '0a1b2c3d4e5f607', '0a1b2c3d4e5f60711', '0a1b-2c3d-4e5f-6071', '0a1b2c3d4e5f607g']) expect(codeShaped(typed), typed).toBe(false)
})

it('shows the first 128 bits of a fingerprint in eight groups of four, as the station shows its own', () => {
  expect(groupedKey('0123456789abcdef'.repeat(4))).toBe('0123 4567 89ab cdef 0123 4567 89ab cdef')
})

it('finds its socket beside the page, the launch secret in the path and nothing of the page’s address after it', () => {
  const secret = '5e'.repeat(32)
  expect(socketUrl(`http://127.0.0.1:42076/${secret}/lan.html?lang=de#x`)).toBe(`ws://127.0.0.1:42076/${secret}/socket`)
  expect(socketUrl(`http://127.0.0.1:49152/${secret}/`)).toBe(`ws://127.0.0.1:49152/${secret}/socket`)
})

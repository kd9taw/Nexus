// What the Stations on this network page is told, read strictly (`readTold`), and the small shapes
// it says itself: the code's shape before Pair is worth pressing, and its socket's address beside
// the page.
import { expect, it } from 'vitest'
import { codeShaped, readTold, socketUrl } from './protocol'

const STATION = { id: '60000000-0000-4000-8000-000000000001', addresses: ['192.168.1.20:42075'], key: 'ab'.repeat(32) }
const KEY = `3059301306072a8648ce3d020106082a8648ce3d03010703420004${'cd'.repeat(64)}`
const AWAY = { id: '20000000-0000-4000-8000-0000000000a1', name: 'Shack', key: 'ef'.repeat(32) }
/** This computer's key for a station's internet road, as its fingerprint: SHA-256, lowercase hex. */
const ROAD_KEY = '9f8e7d6c5b4a3928'.repeat(4)
/** A P-256 private key as a road's record holds it (PKCS#8, lowercase hex): never the page's. */
const PRIVATE = `308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420${'5a'.repeat(32)}a14403420004${'cd'.repeat(64)}`

it('reads every message this computer’s Nexus sends, and passes the station’s own through', () => {
  expect(readTold({ type: 'stations', stations: [STATION], computer: 'DEN-PC' })).toEqual({ type: 'stations', stations: [STATION], internet: [], account: [], away: [], computer: 'DEN-PC', error: null, forgetUnreached: null })
  const road = { id: STATION.id, key: ROAD_KEY }
  expect(readTold({ type: 'stations', stations: [STATION], internet: [road], computer: 'DEN-PC' })).toEqual({ type: 'stations', stations: [STATION], internet: [road], account: [], away: [], computer: 'DEN-PC', error: null, forgetUnreached: null })
  expect(readTold({ type: 'stations', stations: [], internet: [], computer: 'DEN-PC', forgetUnreached: 'serviceUnavailable' })).toMatchObject({ forgetUnreached: 'serviceUnavailable' })
  const club = { id: '20000000-0000-4000-8000-0000000000c1', name: 'Club station' }
  expect(readTold({ type: 'chooseStation', stations: [club, { id: STATION.id, name: '' }] })).toEqual({ type: 'chooseStation', stations: [club, { id: STATION.id, name: '' }] })
  expect(readTold({ type: 'signingIn' })).toEqual({ type: 'signingIn' })
  expect(readTold({ type: 'signedIn', stationId: STATION.id, waiting: true, away: false, key: ROAD_KEY })).toEqual({ type: 'signedIn', stationId: STATION.id, waiting: true, away: false, key: ROAD_KEY })
  expect(readTold({ type: 'signedIn', stationId: AWAY.id, waiting: false, away: true, key: ROAD_KEY })).toEqual({ type: 'signedIn', stationId: AWAY.id, waiting: false, away: true, key: ROAD_KEY })
  expect(readTold({ type: 'signInRefused', reason: 'stationNotProven' })).toEqual({ type: 'signInRefused', reason: 'stationNotProven' })
  // A reason this page does not know still ends the sign-in, said as one that did not finish.
  expect(readTold({ type: 'signInRefused', reason: 'somethingNew' })).toEqual({ type: 'signInRefused', reason: 'signInFailed' })
  expect(readTold({ type: 'stationAddress', name: 'Shack' })).toEqual({ type: 'stationAddress', name: 'Shack', reason: null })
  expect(readTold({ type: 'stationAddress', name: '', reason: 'keyChanged' })).toEqual({ type: 'stationAddress', name: '', reason: 'keyChanged' })
  const account = [{ stationId: STATION.id, until: 1_802_592_000, renew: true }]
  expect(readTold({ type: 'stations', stations: [STATION], internet: [road], account, computer: 'DEN-PC' })).toMatchObject({ account })
  expect(readTold({ type: 'stations', stations: [], computer: '', error: 'storeUnavailable' })).toMatchObject({ error: 'storeUnavailable' })
  // A station kept by its service id until this computer first reaches it on its network.
  expect(readTold({ type: 'stations', stations: [STATION], away: [AWAY], computer: 'DEN-PC' })).toEqual({ type: 'stations', stations: [STATION], internet: [], account: [], away: [AWAY], computer: 'DEN-PC', error: null, forgetUnreached: null })
  expect(readTold({ type: 'stations', stations: [], away: [{ ...AWAY, name: '' }], computer: 'DEN-PC' })).toMatchObject({ away: [{ ...AWAY, name: '' }] })
  expect(readTold({ type: 'paired', station: STATION })).toEqual({ type: 'paired', station: STATION })
  for (const typedAddress of ['shack-pc.local:42075', '192.168.1.44:42080', `${'a'.repeat(253)}:42075`]) {
    const kept = { ...STATION, typedAddress }
    expect(readTold({ type: 'stations', stations: [kept], computer: 'DEN-PC' })).toEqual({ type: 'stations', stations: [kept], internet: [], account: [], away: [], computer: 'DEN-PC', error: null, forgetUnreached: null })
  }
  expect(readTold({ type: 'pairRefused', reason: 'wrongCode' })).toEqual({ type: 'pairRefused', reason: 'wrongCode' })
  const open = { stationId: STATION.id, deviceId: '1d2b7c3a-4e5f-8a6b-9c7d-0e1f2a3b4c5d', sessionId: '50000000-0000-4000-8000-000000000002', stationKey: KEY, address: '192.168.1.20:42075' }
  expect(readTold({ type: 'connected', ...open })).toEqual({ type: 'connected', road: open })
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
    { type: 'stations', stations: [{ ...STATION, typedAddress: 'Shack PC:42075' }], computer: 'X' },
    { type: 'stations', stations: [{ ...STATION, typedAddress: 'shack-pc.local' }], computer: 'X' },
    { type: 'stations', stations: [{ ...STATION, typedAddress: `${'a'.repeat(254)}:42075` }], computer: 'X' },
    { type: 'stations', stations: [{ ...STATION, typedAddress: null }], computer: 'X' },
    { type: 'stations', stations: [{ ...STATION, pkcs8: '00' }], computer: 'X' },
    { type: 'stations', stations: [{ ...STATION, key: 'AB'.repeat(32) }], computer: 'X' },
    { type: 'paired' },
    { type: 'pairRefused', reason: 'somethingElse' },
    { type: 'connectRefused', reason: 'wrongCode' },
    { type: 'closed', reason: 'notController' },
    { type: 'answerRefused', reason: 'stationBusy' },
    { type: 'stations', stations: [STATION], internet: ['not-an-id'], computer: 'X' },
    { type: 'stations', stations: [STATION], internet: 'all', computer: 'X' },
    { type: 'internetAdded', stationId: STATION.id, waiting: true },
    { type: 'internetRefused', reason: 'internetNotIncluded' },
    { type: 'signedIn', stationId: STATION.id },
    { type: 'signedIn', stationId: STATION.id, waiting: 'yes' },
    { type: 'signedIn', stationId: STATION.id, waiting: false, key: ROAD_KEY },
    { type: 'signedIn', stationId: STATION.id, waiting: false, away: 'yes', key: ROAD_KEY },
    { type: 'stations', stations: [], away: [{ ...AWAY, id: 'not-an-id' }], computer: 'X' },
    { type: 'stations', stations: [], away: [{ ...AWAY, key: 'EF'.repeat(32) }], computer: 'X' },
    { type: 'stations', stations: [], away: [{ ...AWAY, name: 'N'.repeat(97) }], computer: 'X' },
    { type: 'stations', stations: [], away: [{ ...AWAY, pkcs8: '00' }], computer: 'X' },
    { type: 'stations', stations: [], away: [{ id: AWAY.id, name: 'Shack' }], computer: 'X' },
    { type: 'stations', stations: [], away: Array.from({ length: 9 }, () => AWAY), computer: 'X' },
    { type: 'stations', stations: [], away: AWAY, computer: 'X' },
    { type: 'signInRefused', reason: 7 },
    { type: 'signInRefused', reason: 'x'.repeat(65) },
    { type: 'stationAddress', name: 'Shack', reason: 'wrongCode' },
    { type: 'stationAddress', name: 'Shack', reason: null },
    { type: 'stationAddress', reason: 'keyChanged' },
    { type: 'stations', stations: [STATION], account: [{ stationId: 'not-an-id', until: 1, renew: true }], computer: 'X' },
    { type: 'stations', stations: [STATION], account: [{ stationId: STATION.id, until: 1.5, renew: true }], computer: 'X' },
    { type: 'stations', stations: [STATION], account: [{ stationId: STATION.id, until: 1, renew: 'yes' }], computer: 'X' },
    { type: 'stations', stations: [STATION], account: [{ stationId: STATION.id, until: 1, renew: true, proof: 'x' }], computer: 'X' },
    // A road is told with this computer's key for it, and with that key only as its fingerprint:
    // never missing, never in capitals, never beside or in place of the private key.
    { type: 'stations', stations: [STATION], internet: [STATION.id], computer: 'X' },
    { type: 'stations', stations: [STATION], internet: [{ id: STATION.id }], computer: 'X' },
    { type: 'stations', stations: [STATION], internet: [{ id: STATION.id, key: ROAD_KEY.toUpperCase() }], computer: 'X' },
    { type: 'stations', stations: [STATION], internet: [{ id: STATION.id, key: PRIVATE }], computer: 'X' },
    { type: 'stations', stations: [STATION], internet: [{ id: STATION.id, key: ROAD_KEY, pkcs8: PRIVATE }], computer: 'X' },
    { type: 'stations', stations: [STATION], internet: [{ id: 'not-an-id', key: ROAD_KEY }], computer: 'X' },
    { type: 'signedIn', stationId: STATION.id, waiting: true, away: false },
    { type: 'signedIn', stationId: STATION.id, waiting: true, away: false, key: ROAD_KEY.toUpperCase() },
    { type: 'signedIn', stationId: STATION.id, waiting: true, away: false, key: PRIVATE },
    { type: 'signedIn', stationId: STATION.id, waiting: true, away: false, key: ROAD_KEY, pkcs8: PRIVATE },
    { type: 'signingIn', token: 'x' },
    { type: 'stations', stations: [], computer: 'X', forgetUnreached: 7 },
    { type: 'chooseStation', stations: [{ id: STATION.id, name: 'Shack' }] },
    { type: 'chooseStation', stations: [{ id: 'not-an-id', name: 'A' }, { id: STATION.id, name: 'B' }] },
    { type: 'chooseStation', stations: [{ id: STATION.id, name: 'A', key: KEY }, { id: STATION.id, name: 'B' }] },
    { type: 'chooseStation', stations: Array.from({ length: 17 }, () => ({ id: STATION.id, name: 'A' })) },
    { type: 'connected', stationId: STATION.id, deviceId: 'x', sessionId: STATION.id, stationKey: KEY, address: '192.168.1.20:42075' },
    { type: 'connected', stationId: STATION.id, deviceId: STATION.id, sessionId: STATION.id, stationKey: 'ab'.repeat(91), address: '192.168.1.20:42075' },
    null, 'stations', [],
  ]) {
    expect(() => readTold(told), JSON.stringify(told)).toThrow()
  }
})

it('reads what a look by name found, strictly: each station’s name, address, protocol and key tag', () => {
  const shack = { name: 'Nexus 3F2A 9B1C', address: '192.168.1.20:42075', protocol: 1, key: '3f2a9b1c00c0ffee' }
  expect(readTold({ type: 'found', available: true, shacks: [shack] })).toEqual({ type: 'found', available: true, shacks: [shack] })
  expect(readTold({ type: 'found', available: false, shacks: [] })).toEqual({ type: 'found', available: false, shacks: [] })
  for (const told of [
    { type: 'found', available: true },
    { type: 'found', available: 'yes', shacks: [] },
    { type: 'found', available: true, shacks: [{ ...shack, extra: 1 }] },
    { type: 'found', available: true, shacks: [{ ...shack, address: 'nexus-3f2a9b1c.local:42075' }] },
    { type: 'found', available: true, shacks: [{ ...shack, key: '3F2A9B1C00C0FFEE' }] },
    { type: 'found', available: true, shacks: [{ ...shack, protocol: 1.5 }] },
    { type: 'found', available: true, shacks: [{ ...shack, name: 'N'.repeat(65) }] },
  ]) {
    expect(() => readTold(told), JSON.stringify(told)).toThrow()
  }
})

it('takes a code of sixteen hexadecimal characters as typed, either case, with spaces anywhere', () => {
  for (const typed of ['0a1b2c3d4e5f6071', '0A1B 2C3D 4E5F 6071', ' 0a1b 2c3d4e5f6071 ']) expect(codeShaped(typed), typed).toBe(true)
  for (const typed of ['', '0a1b2c3d4e5f607', '0a1b2c3d4e5f60711', '0a1b-2c3d-4e5f-6071', '0a1b2c3d4e5f607g']) expect(codeShaped(typed), typed).toBe(false)
})

it('finds its socket beside the page, the launch secret in the path and nothing of the page’s address after it', () => {
  const secret = '5e'.repeat(32)
  expect(socketUrl(`http://127.0.0.1:42076/${secret}/lan.html?lang=de#x`)).toBe(`ws://127.0.0.1:42076/${secret}/socket`)
  expect(socketUrl(`http://127.0.0.1:49152/${secret}/`)).toBe(`ws://127.0.0.1:49152/${secret}/socket`)
})

it('reads the station’s key for each internet road, and why a road is held, strictly', () => {
  const key = { id: STATION.id, key: 'ab'.repeat(32) }
  expect(readTold({ type: 'stations', stations: [STATION], stationKeys: [key, { ...key, id: AWAY.id, held: 'stationKeyUnconfirmed' }], computer: 'X' }))
    .toMatchObject({ stationKeys: [{ ...key, held: null }, { ...key, id: AWAY.id, held: 'stationKeyUnconfirmed' }] })
  expect(readTold({ type: 'stations', stations: [], stationKeys: [{ ...key, held: 'stationKeyDisagrees' }], computer: 'X' }))
    .toMatchObject({ stationKeys: [{ held: 'stationKeyDisagrees' }] })
  expect(readTold({ type: 'stations', stations: [STATION], stationKeys: [key, { ...key, codePaired: true }], computer: 'X' }))
    .toMatchObject({ stationKeys: [{ codePaired: false }, { codePaired: true }] })
  for (const codePaired of [false, 'true', 1]) {
    expect(() => readTold({ type: 'stations', stations: [], stationKeys: [{ ...key, codePaired }], computer: 'X' }), String(codePaired)).toThrow()
  }
  expect(readTold({ type: 'stations', stations: [], computer: 'X', dropUnreached: 'serviceUnavailable' }))
    .toMatchObject({ dropUnreached: 'serviceUnavailable', forgetUnreached: null })
  expect(() => readTold({ type: 'stations', stations: [], computer: 'X', dropUnreached: 7 })).toThrow()
  for (const bad of [{ ...key, held: 'later' }, { ...key, key: KEY }, { ...key, extra: 1 }, { id: 'x', key: key.key }]) {
    expect(() => readTold({ type: 'stations', stations: [], stationKeys: [bad], computer: 'X' }), JSON.stringify(bad)).toThrow()
  }
})

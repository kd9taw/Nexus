// The browser's device key (A5). Every key here is made at run time and never written down; a
// memory store stands in for IndexedDB, which the browser probe exercises for real.
import { expect, it } from 'vitest'
import { deviceKey, signOffer, type KeyStore } from './device-key'
import { offerBinding, offerFingerprint, P256_SPKI_PREFIX_HEX } from './stream-protocol'
import { OFFER } from './stream-link.testkit'

const STATION = crypto.randomUUID(), DEVICE = crypto.randomUUID(), SESSION = crypto.randomUUID()
const hex = (bytes: ArrayBuffer | Uint8Array) => Buffer.from(bytes as ArrayBuffer).toString('hex')
const bytes = (text: string) => Uint8Array.from(Buffer.from(text, 'hex'))

/** A key store over `pairs`: two stores over one map are two tabs of one browser. */
function memoryStore(pairs = new Map<string, CryptoKeyPair>(), before?: () => Promise<void>): KeyStore {
  return {
    get: async stationId => { await before?.(); return pairs.get(stationId) },
    keep: async (stationId, pair) => { if (!pairs.has(stationId)) pairs.set(stationId, pair); return pairs.get(stationId)! },
  }
}

it('A5: makes one non-extractable P-256 key per station, keeps it, and gives out only its public half', async () => {
  const pairs = new Map<string, CryptoKeyPair>()
  const key = await deviceKey(STATION, memoryStore(pairs))
  expect(key).not.toBeNull()
  const kept = pairs.get(STATION)!
  expect(kept.privateKey.extractable, 'the private half can never be read out').toBe(false)
  expect(kept.privateKey.algorithm).toMatchObject({ name: 'ECDSA', namedCurve: 'P-256' })
  expect(key!.publicKey).toMatch(new RegExp(`^${P256_SPKI_PREFIX_HEX}04[0-9a-f]{128}$`))
  expect(key!.fingerprint).toBe(hex(await crypto.subtle.digest('SHA-256', bytes(key!.publicKey))))
  // A reload (a new page over the same store) signs with the same key; another station has its own.
  expect((await deviceKey(STATION, memoryStore(pairs)))!.publicKey).toBe(key!.publicKey)
  const other = await deviceKey(crypto.randomUUID(), memoryStore(pairs))
  expect(other!.publicKey).not.toBe(key!.publicKey)
})

it('A5: two tabs making the first key at once both sign with the one kept first', async () => {
  const pairs = new Map<string, CryptoKeyPair>()
  // Both tabs look before either has kept one.
  let release!: () => void
  const looked = new Promise<void>(resolve => { release = resolve })
  let waiting = 2
  const gate = async () => { if (--waiting === 0) release(); await looked }
  const [a, b] = await Promise.all([deviceKey(STATION, memoryStore(pairs, gate)), deviceKey(STATION, memoryStore(pairs, gate))])
  expect(a!.publicKey).toBe(b!.publicKey)
  expect(hex(await crypto.subtle.exportKey('spki', pairs.get(STATION)!.publicKey))).toBe(a!.publicKey)
})

it('A5: never signs with a kept private key that could be read out', async () => {
  const readable = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify']) as CryptoKeyPair
  expect(await deviceKey(STATION, memoryStore(new Map([[STATION, readable]])))).toBeNull()
  // Control: the same store shape with a key that cannot be read out is used.
  const sealed = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']) as CryptoKeyPair
  expect(await deviceKey(STATION, memoryStore(new Map([[STATION, sealed]])))).not.toBeNull()
})

it('A5: the offer signature verifies for exactly this station, device and session', async () => {
  const key = (await deviceKey(STATION, memoryStore()))!
  const signed = (await signOffer(key, OFFER, STATION, DEVICE, SESSION))!
  expect(signed.publicKey).toBe(key.publicKey)
  expect(signed.signature).toMatch(/^[0-9a-f]{128}$/)
  const publicKey = await crypto.subtle.importKey('spki', bytes(signed.publicKey), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify'])
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(offerFingerprint(OFFER)!)))
  const verifies = (station: string, device: string, session: string) => crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' },
    publicKey, bytes(signed.signature), new Uint8Array(offerBinding(digest, station, device, session)))
  expect(await verifies(STATION, DEVICE, SESSION)).toBe(true)
  const elsewhere = crypto.randomUUID()
  expect(await verifies(STATION, DEVICE, elsewhere), 'another session').toBe(false)
  expect(await verifies(STATION, elsewhere, SESSION), 'another device').toBe(false)
  expect(await verifies(elsewhere, DEVICE, SESSION), 'another station').toBe(false)
  // An offer with no single SHA-256 fingerprint is not signed at all.
  const bare = OFFER.split('\r\n').filter(line => !line.startsWith('a=fingerprint:')).join('\r\n')
  expect(await signOffer(key, bare, STATION, DEVICE, SESSION)).toBeNull()
})

it('A5: a browser that cannot keep a key has none, and is asked again next time', async () => {
  let broken = true
  const pairs = new Map<string, CryptoKeyPair>()
  const store: KeyStore = {
    get: async stationId => { if (broken) throw Error('no IndexedDB'); return pairs.get(stationId) },
    keep: async (stationId, pair) => { if (!pairs.has(stationId)) pairs.set(stationId, pair); return pairs.get(stationId)! },
  }
  expect(await deviceKey(STATION, store)).toBeNull()
  broken = false
  expect(await deviceKey(STATION, store)).not.toBeNull()
})

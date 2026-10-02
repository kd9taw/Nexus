// The browser's device key (security review M1; the stream's A5).
//
// One ECDSA P-256 key per station, made in this browser and kept in it. The private half is a
// NON-EXTRACTABLE CryptoKey that lives only in this origin's IndexedDB: nothing, this page
// included, can read it out, and nothing here ever tries. Only the public half leaves, as SPKI in
// lowercase hex, when the page asks for its device or confirms a pairing. The station pins its
// SHA-256 when the operator approves this browser at the radio, and starts a stream only for an
// offer this key signed, over the offer's own DTLS certificate and this session's identity. So a
// relay that forwards the offer cannot stand in for the browser that made it.
//
// A browser without IndexedDB (a locked-down profile) has no key: it can still do everything it
// could before, and the station refuses its stream by name.
import { offerBinding, offerFingerprint, toHex, type OfferSignature } from './stream-protocol'

export type DeviceKey = {
  /** The public half: SPKI DER as lowercase hex, 182 characters. */
  publicKey: string
  /** SHA-256 of that SPKI as lowercase hex: what the station pins, and what both ends show. */
  fingerprint: string
  /** ECDSA P-256 over SHA-256, IEEE P1363 r||s (64 bytes). */
  sign: (data: Uint8Array) => Promise<Uint8Array>
}

/** Where a station's key pair is kept. Split out so a test can hold keys without IndexedDB. */
export type KeyStore = {
  get: (stationId: string) => Promise<CryptoKeyPair | undefined>
  /** Keep `pair` unless one is already kept for the station (another tab may have just made one),
   *  and answer whichever is kept, so every tab signs with the key the service was sent. */
  keep: (stationId: string, pair: CryptoKeyPair) => Promise<CryptoKeyPair>
}

const DATABASE = 'nexus-remote-device-keys'
const STORE = 'keys'

function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1)
    request.onupgradeneeded = () => { request.result.createObjectStore(STORE) }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

/** The browser's own store: IndexedDB, which keeps a non-extractable CryptoKey as it is. */
export function browserKeyStore(): KeyStore {
  function transact<Value>(mode: IDBTransactionMode, work: (store: IDBObjectStore, done: (value: Value) => void) => void): Promise<Value> {
    return database().then(db => new Promise<Value>((resolve, reject) => {
      const transaction = db.transaction(STORE, mode)
      let result: Value
      work(transaction.objectStore(STORE), value => { result = value })
      transaction.oncomplete = () => { db.close(); resolve(result) }
      transaction.onerror = transaction.onabort = () => { db.close(); reject(transaction.error) }
    }))
  }
  return {
    get: stationId => transact<CryptoKeyPair | undefined>('readonly', (store, done) => {
      const found = store.get(stationId)
      found.onsuccess = () => done(found.result as CryptoKeyPair | undefined)
    }),
    // One read-write transaction, which IndexedDB runs alone on this store: two tabs making a key
    // at once both keep the first one written.
    keep: (stationId, pair) => transact<CryptoKeyPair>('readwrite', (store, done) => {
      const found = store.get(stationId)
      found.onsuccess = () => {
        if (found.result) { done(found.result as CryptoKeyPair); return }
        store.put(pair, stationId)
        done(pair)
      }
    }),
  }
}

async function open(stationId: string, store: KeyStore): Promise<DeviceKey | null> {
  const pair = await store.get(stationId) ?? await store.keep(stationId,
    await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']) as CryptoKeyPair)
  // Never sign with a private key that could be read out, whatever put it there.
  if (pair.privateKey.extractable) return null
  // The public half is always exportable (WebCrypto makes it so); this is the one export there is.
  const spki = new Uint8Array(await crypto.subtle.exportKey('spki', pair.publicKey))
  const fingerprint = toHex(new Uint8Array(await crypto.subtle.digest('SHA-256', spki)))
  return {
    publicKey: toHex(spki),
    fingerprint,
    // A copy, so WebCrypto is handed bytes over a plain ArrayBuffer whatever the caller's were.
    sign: async data => new Uint8Array(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new Uint8Array(data))),
  }
}

let browser: KeyStore | null = null
const opened = new WeakMap<KeyStore, Map<string, Promise<DeviceKey | null>>>()
/** This browser's key for a station, made the first time it is asked for. Null where the browser
 *  cannot keep one. Kept for the life of the page; a failure is asked again next time. */
export function deviceKey(stationId: string, store: KeyStore = browser ??= browserKeyStore()): Promise<DeviceKey | null> {
  const keys = opened.get(store) ?? new Map<string, Promise<DeviceKey | null>>()
  opened.set(store, keys)
  const known = keys.get(stationId)
  if (known) return known
  const next = open(stationId, store).catch(() => null)
  keys.set(stationId, next)
  void next.then(key => { if (!key) keys.delete(stationId) })
  return next
}

/** The offer's signature (the contract's README, "Binding a stream to the approved browser"): this
 *  station's key over SHA-256 of the offer's DTLS fingerprint and the three ids. Null for an offer
 *  with no single SHA-256 fingerprint, which the station would refuse anyway. */
export async function signOffer(key: DeviceKey, sdp: string, stationId: string, deviceId: string, sessionId: string): Promise<OfferSignature | null> {
  const fingerprint = offerFingerprint(sdp)
  if (!fingerprint) return null
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(fingerprint)))
  return { publicKey: key.publicKey, signature: toHex(await key.sign(offerBinding(digest, stationId, deviceId, sessionId))) }
}

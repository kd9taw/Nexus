// The station's own key, checked on its answer (security review S3-M1, 2026-10-03).
//
// A5 binds this browser to the station: the station takes only an offer this browser's pinned key
// signed. Nothing bound the station to this browser, so a relay that answered the page's offer itself
// became "the station" here, and was sent every key typed on the picture, every paste and the
// microphone, and could point the browser's ICE checks at the operator's own network. The station
// now signs its answer with its own key, which the service's pairing record holds and lists with the
// station (`stationKey`), over both DTLS fingerprints and the three ids. The page checks that before
// the browser is handed the answer or any candidate behind it, and refuses on anything else:
//
// - `stationKeyMismatch`: signed, but not by that key over this negotiation - forged, replayed from
//   another session or offer, or another station's;
// - `stationNotSigned`: no signature, or no key from the service to check one with - a station from
//   before its key, which an update at the shack fixes.
//
// The key is fetched over the service's signed-in API, never through the relay, so a relay cannot
// choose the key it is checked against. The DTLS handshake then holds the browser to the certificate
// the signed answer names, which only the station has.
//
// THE KEPT KEY (security review S3-L1). Checked afresh against the service's listing, the answer
// held against a relay but not against the service's own records: changed (a compromised database),
// they could list another key and an answer signed with it was taken. So this browser keeps the first
// key the service lists for each station it has a device on, in IndexedDB by station, as it keeps its
// own device key (`device-key.ts`). While the service lists any other key, the stream is refused
// before its answer is looked at:
//
// - `stationKeyChanged`: the service lists a key other than the one kept here. Nothing connects.
//
// It is a check beside the signature, never instead of it: the answer must still be signed with the
// listed key. Only the operator takes a new key, on the page, with both keys shown to compare with
// Nexus at the shack; revoking the station, or removing this browser's approval for it, forgets the
// kept key. A browser that cannot keep one (no IndexedDB) is checked as before, against the listing;
// it cannot keep a device key either, so the station refuses its streams anyway. What it cannot
// answer: the page itself comes from the service, so a service that serves another page is not
// stopped by anything the page keeps.
import { answerBinding, answerSignature, offerFingerprint, P256_SPKI_PREFIX_HEX, STREAM_DEVICE_KEY_HEX_CHARS, toHex } from './stream-protocol'

export type AnswerRefusal = 'stationKeyMismatch' | 'stationNotSigned' | 'stationKeyChanged'

const bytes = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], pair => parseInt(pair, 16))
const sha256 = async (data: Uint8Array) => new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(data)))

/** Is `answer` the station's own, for `offer` from this device in this session? Null when it is, or
 *  why not. `stationKey` is the station's public key (SPKI, lowercase hex) as the service lists it. */
export async function checkAnswer(stationKey: string | null, answer: string, offer: string,
  ids: { stationId: string; deviceId: string; sessionId: string }): Promise<AnswerRefusal | null> {
  const signature = answerSignature(answer)
  if (!stationKey || !signature) return 'stationNotSigned'
  const answerPrint = offerFingerprint(answer), offerPrint = offerFingerprint(offer)
  if (!answerPrint || !offerPrint) return 'stationKeyMismatch'
  try {
    const key = await crypto.subtle.importKey('spki', bytes(stationKey), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify'])
    const signed = answerBinding(await sha256(answerPrint), await sha256(offerPrint), ids.stationId, ids.deviceId, ids.sessionId)
    return await crypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, key, bytes(signature), new Uint8Array(signed)) ? null : 'stationKeyMismatch'
  } catch {
    // A key the browser will not take is not this station's key.
    return 'stationKeyMismatch'
  }
}

/** Where this browser keeps each station's key. Split out so a test can keep keys without IndexedDB. */
export type StationPins = {
  /** Keep `key` unless one is already kept for the station (another tab may have kept one), and
   *  answer whichever is kept. */
  keep: (stationId: string, key: string) => Promise<string>
  /** Keep `key` in place of the one kept: the operator's accept, and nothing else. */
  replace: (stationId: string, key: string) => Promise<void>
  /** Keep nothing for the station. */
  forget: (stationId: string) => Promise<void>
}

const DATABASE = 'nexus-remote-station-keys'
const STORE = 'keys'

/** The browser's own store: IndexedDB, as the device keys are kept, in a database of its own. */
export function browserStationPins(): StationPins {
  function transact<Value>(mode: IDBTransactionMode, work: (store: IDBObjectStore, done: (value: Value) => void) => void): Promise<Value> {
    return new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DATABASE, 1)
      request.onupgradeneeded = () => { request.result.createObjectStore(STORE) }
      request.onsuccess = () => resolve(request.result)
      request.onerror = () => reject(request.error)
    }).then(db => new Promise<Value>((resolve, reject) => {
      const transaction = db.transaction(STORE, mode)
      let result: Value
      work(transaction.objectStore(STORE), value => { result = value })
      transaction.oncomplete = () => { db.close(); resolve(result) }
      transaction.onerror = transaction.onabort = () => { db.close(); reject(transaction.error) }
    }))
  }
  return {
    // One read-write transaction, which IndexedDB runs alone on this store: two tabs seeing the
    // station first at once both keep the first key written.
    keep: (stationId, key) => transact<string>('readwrite', (store, done) => {
      const found = store.get(stationId)
      found.onsuccess = () => {
        if (typeof found.result === 'string') { done(found.result); return }
        store.put(key, stationId)
        done(key)
      }
    }),
    replace: (stationId, key) => transact<void>('readwrite', (store, done) => { store.put(key, stationId); done() }),
    forget: stationId => transact<void>('readwrite', (store, done) => { store.delete(stationId); done() }),
  }
}

let browser: StationPins | null = null
const pins = (store?: StationPins) => store ?? (browser ??= browserStationPins())
/** A key of the shape the service lists one in: P-256 SPKI as lowercase hex, as a device key is. */
const keyShaped = (value: unknown): value is string => typeof value === 'string' && value.length === STREAM_DEVICE_KEY_HEX_CHARS
  && value.startsWith(`${P256_SPKI_PREFIX_HEX}04`) && /^[0-9a-f]+$/.test(value)

/** The key this browser keeps for the station, against `listed`, the one the service lists now: the
 *  first key listed is kept. Null when no key is listed (or none of a key's shape), and where this
 *  browser cannot keep one. */
async function pinned(stationId: string, listed: string | null | undefined, store?: StationPins): Promise<{ kept: string; listed: string } | null> {
  if (!keyShaped(listed)) return null
  try { return { kept: await pins(store).keep(stationId, listed), listed } } catch { return null }
}

/** The stream's check before the answer's (S3-L1): `stationKeyChanged` while the service lists a key
 *  for the station other than the one this browser kept, null otherwise. Keeps the listed key if
 *  none is kept yet. */
export async function checkPin(stationId: string, listed: string | null, store?: StationPins): Promise<AnswerRefusal | null> {
  const pin = await pinned(stationId, listed, store)
  return pin && pin.kept !== pin.listed ? 'stationKeyChanged' : null
}

/** The station's key as the page shows it: the key the service lists now, the SHA-256 of that key
 *  (lowercase hex, its fingerprint), and the fingerprint of the key this browser kept, which differs
 *  only while the service lists another. */
export type StationKeyView = { listed: string; print: string; keptPrint: string }
/** The station's key as the page shows it, keeping the listed key if none is kept yet. Null as for
 *  `checkPin`. */
export async function stationKeyView(stationId: string, listed: string | null | undefined, store?: StationPins): Promise<StationKeyView | null> {
  const pin = await pinned(stationId, listed, store)
  if (!pin) return null
  const print = await fingerprint(pin.listed)
  return { listed: pin.listed, print, keptPrint: pin.kept === pin.listed ? print : await fingerprint(pin.kept) }
}

/** The operator accepts `key`, the one the service lists now and the page showed, for the station. */
export function acceptStationKey(stationId: string, key: string, store?: StationPins): Promise<void> {
  return pins(store).replace(stationId, key)
}

/** The station's key is no longer kept here: the station was revoked, or this browser's approval
 *  removed, on the page. */
export function forgetStationKey(stationId: string, store?: StationPins): Promise<void> {
  return pins(store).forget(stationId)
}

/** SHA-256 of a key's SPKI, lowercase hex: what both ends show, shortened, to compare. */
async function fingerprint(key: string): Promise<string> {
  return toHex(await sha256(bytes(key)))
}

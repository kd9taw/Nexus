// The relay's older lanes, held to this browser's own key (security review S1-M1, 2026-10-03).
//
// The relay stamps the session and device on everything it hands the station, so on its word alone
// a relay that is not the one the operator trusts could act as any approved browser there: take
// control, keep a lease alive, work FT, write the log, listen. The stream's offer has carried this
// browser's signature since A5; the older lanes now carry it too. Every operation request but Stop
// and `state`, and every Listen, goes with a proof: the device key's signature over the lane's label,
// SHA-256 of the message's own bytes (the operation request exactly as it goes on the socket; Listen's
// `listenBody`), the station, device and session ids, and a number this page counts up within the
// session and never uses twice. The station checks it against the key the operator pinned at the
// radio and refuses anything else.
//
// Stop needs none, on purpose: a forged Stop only stops. `state` needs none: it grants nothing, and it
// is how a page learns what Stop is composed from, so Stop never waits on a key.
//
// This file is the shapes and the signed bytes, shared with the relay, which checks a proof's shape
// and hands it on only to a station that advertised it. The signing is `device-key.ts`'s.

/** What an operation request's proof begins with. */
export const OPERATION_LANE = 'nexus-operation/1'
/** What a Listen's proof begins with. */
export const LISTEN_LANE = 'nexus-listen/1'
/** The most a proof adds to a message on the wire: `"proof":{...}` with the key, the largest number
 *  and the signature. Budgets allow for it on top of the message itself. */
export const LANE_PROOF_BYTES = 384

export type LaneProof = { publicKey: string; seq: number; signature: string }

const KEY = /^3059301306072a8648ce3d020106082a8648ce3d03010703420004[0-9a-f]{128}$/
const SIGNATURE = /^[0-9a-f]{128}$/
/** A proof's shape, as the relay checks it before handing it on: exactly these three fields, a P-256
 *  SPKI, a number from 1 up, a 64-byte signature. Whether it holds is the station's to find out. */
export function laneProof(raw: unknown): LaneProof {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidProof')
  const value = raw as Record<string, unknown>
  const keys = Object.keys(value)
  if (keys.length !== 3 || !['publicKey', 'seq', 'signature'].every(key => keys.includes(key))) throw Error('invalidProof')
  if (typeof value.publicKey !== 'string' || !KEY.test(value.publicKey)) throw Error('invalidProof')
  if (!Number.isSafeInteger(value.seq) || (value.seq as number) < 1) throw Error('invalidProof')
  if (typeof value.signature !== 'string' || !SIGNATURE.test(value.signature)) throw Error('invalidProof')
  return { publicKey: value.publicKey, seq: value.seq as number, signature: value.signature }
}

/** The bytes a proof's signature covers: the label, SHA-256 of the message's bytes (the caller
 *  hashes), the station, device and session ids, and the number as eight big-endian bytes. */
export function laneBinding(label: string, bodyDigest: Uint8Array, stationId: string, deviceId: string, sessionId: string, seq: number): Uint8Array {
  const text = new TextEncoder()
  const number = new Uint8Array(8)
  new DataView(number.buffer).setBigUint64(0, BigInt(seq))
  return new Uint8Array([...text.encode(label), ...bodyDigest, ...text.encode(stationId), ...text.encode(deviceId), ...text.encode(sessionId), ...number])
}

/** What a Listen's proof covers: what it asks, and under which lease, in one fixed spelling. */
export function listenBody(listening: boolean, leaseId: string): string {
  return `{"listening":${listening},"leaseId":"${leaseId}"}`
}

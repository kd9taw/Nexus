// The station's own key, checked on its answer (security review S3-M1). The answers are signed here
// with keys made at run time, as the contract's README describes the bytes.
import { expect, it } from 'vitest'
import { checkAnswer } from './station-key'
import { answerSignature, offerFingerprint } from './stream-protocol'
import { ANSWER, FINGERPRINT, OFFER } from './stream-link.testkit'

const IDS = { stationId: crypto.randomUUID(), deviceId: crypto.randomUUID(), sessionId: crypto.randomUUID() }
const hex = (bytes: ArrayBuffer) => Buffer.from(bytes).toString('hex')
const text = new TextEncoder()
const sha256 = async (bytes: Uint8Array) => new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(bytes)))
const line = (signature: string) => `a=nexus-station-signature:${signature}\r\n`
const atSession = (sdp: string, extra: string) => sdp.replace('\r\nm=', `\r\n${extra}m=`)

async function station() {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']) as CryptoKeyPair
  return {
    spki: hex(await crypto.subtle.exportKey('spki', pair.publicKey)),
    /** The signature over this answer's and offer's fingerprints and the three ids. */
    sign: async (answer: string, offer = OFFER, ids = IDS) => hex(await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey,
      new Uint8Array([...text.encode('nexus-stream-answer/1'), ...await sha256(offerFingerprint(answer)!), ...await sha256(offerFingerprint(offer)!),
        ...text.encode(ids.stationId), ...text.encode(ids.deviceId), ...text.encode(ids.sessionId)]))),
  }
}

it('S3-M1: the station\'s own answer holds; with the relay\'s certificate swapped in, the same signature does not', async () => {
  const key = await station()
  const signed = atSession(ANSWER, line(await key.sign(ANSWER)))
  expect(await checkAnswer(key.spki, signed, OFFER, IDS)).toBeNull()
  // A relay keeps the station's signature line and puts its own DTLS certificate in the answer.
  const relayed = signed.split(FINGERPRINT).join(`a=fingerprint:sha-256 ${Array.from({ length: 32 }, () => 'AB').join(':')}`)
  expect(relayed).not.toBe(signed)
  expect(await checkAnswer(key.spki, relayed, OFFER, IDS)).toBe('stationKeyMismatch')
  // Any one id that is not this negotiation's, and a key the browser will not take, are mismatches too.
  for (const ids of [{ ...IDS, sessionId: crypto.randomUUID() }, { ...IDS, deviceId: crypto.randomUUID() }, { ...IDS, stationId: crypto.randomUUID() }])
    expect(await checkAnswer(key.spki, signed, OFFER, ids)).toBe('stationKeyMismatch')
  expect(await checkAnswer(`3059301306072a8648ce3d020106082a8648ce3d03010703420004${'1'.repeat(128)}`, signed, OFFER, IDS)).toBe('stationKeyMismatch')
})

it('S3-M1: one signature line, in the session section, of the right shape, or the answer counts as unsigned', async () => {
  const key = await station()
  const signature = await key.sign(ANSWER)
  expect(answerSignature(atSession(ANSWER, line(signature)))).toBe(signature)
  expect(answerSignature(ANSWER), 'none').toBeNull()
  expect(answerSignature(atSession(ANSWER, line(signature) + line(signature))), 'two').toBeNull()
  expect(answerSignature(`${ANSWER}${line(signature)}`), 'in a media section').toBeNull()
  expect(answerSignature(atSession(ANSWER, line(signature.slice(2)))), 'short').toBeNull()
  expect(answerSignature(atSession(ANSWER, line(signature.toUpperCase()))), 'not lowercase').toBeNull()
  for (const sdp of [ANSWER, `${ANSWER}${line(signature)}`])
    expect(await checkAnswer(key.spki, sdp, OFFER, IDS)).toBe('stationNotSigned')
  // And a signed answer with no key from the service to check it against is the same: unsigned.
  expect(await checkAnswer(null, atSession(ANSWER, line(signature)), OFFER, IDS)).toBe('stationNotSigned')
})

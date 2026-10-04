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
import { answerBinding, answerSignature, offerFingerprint } from './stream-protocol'

export type AnswerRefusal = 'stationKeyMismatch' | 'stationNotSigned'

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

// The receive-audio lane through the room. It validates an envelope, checks a byte
// bound and fans out. It never base64-decodes a payload, never stores one, keeps no
// checkpoint and schedules no alarm: an audio bundle is worthless one tick after it
// was produced, so there is nothing here worth surviving a hibernation.
//
// It is also deliberately outside the ACK'd relays. `ApplicationStreamRelay` and
// `ObservationRelay` are stop-and-wait - one unacknowledged frame per consumer - so
// audio riding them would cap at 1/RTT AND would spend the credit a spectrum row or a
// meter update needs. Audio takes no credit and is acknowledged by nothing.
import type { Peer } from '../remote-monitor/relay'
import {
  AUDIO_CONTROL_BYTES, AUDIO_MESSAGE_BYTES, parseAudioBundle, parseAudioListen, parseAudioState,
} from './audio-protocol'

type Browser = { sessionId: string; deviceId: string; peer: Peer }
/** A station that never advertised the audio lane must never be handed an `audioListen`:
 *  its message parser rejects unknown fields and would drop the whole control socket. */
type Station = { peer: Peer; supported: boolean } | null
/** Consecutive failed deliveries before a listener's socket is given up on. At one
 *  bundle every 60 ms this is about three seconds of a browser that cannot take data. */
const DELIVERY_FAILURES = 50

export class AudioRelay {
  private station: Station = null
  private browsers = new Map<string, Browser>()
  private failures = new Map<string, number>()
  /** Bundles the relay refused to forward. Diagnostics only; nothing acts on it. */
  dropped = 0

  sync(station: Station, browsers: Browser[]): void {
    const next = new Map(browsers.map(b => [b.sessionId, b]))
    if (this.station?.peer !== station?.peer) this.failures.clear()
    for (const id of [...this.failures.keys()]) if (!next.has(id)) this.failures.delete(id)
    this.station = station
    this.browsers = next
  }

  /** A browser asking to start or stop listening. The browser's identity is stamped
   *  HERE and never read off the message: a session may only ever ask for itself. */
  receiveBrowser(sessionId: string, raw: unknown): void {
    const browser = this.browsers.get(sessionId)
    if (!browser) return
    let listen
    try { listen = parseAudioListen(raw) }
    catch { browser.peer.close(1008, 'invalidAudio'); return }
    // Not a protocol error and not a close: a browser whose build knows the lane may
    // legitimately meet a station whose build does not. Say so and carry on.
    if (!this.station?.supported) { this.tell(browser.peer, { type: 'audioState', listening: false, reason: 'audioUnavailable' }); return }
    const message = JSON.stringify({ type: 'audioListen', sessionId, deviceId: browser.deviceId, listening: listen.listening, leaseId: listen.leaseId })
    if (new TextEncoder().encode(message).length > AUDIO_CONTROL_BYTES + 96) { browser.peer.close(1008, 'invalidAudio'); return }
    try { this.station.peer.send(message) }
    catch { this.tell(browser.peer, { type: 'audioState', listening: false, reason: 'audioUnavailable' }) }
  }

  /** A bundle or a state line from the station, addressed to one session. */
  receiveStation(raw: unknown): void {
    if (!this.station) return
    let sessionId: string, body: Record<string, unknown>
    try {
      if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error('invalidAudio')
      const { sessionId: id, ...rest } = raw as Record<string, unknown>
      if (typeof id !== 'string' || !/^[0-9a-f-]{36}$/.test(id)) throw Error('invalidAudio')
      sessionId = id
      body = rest.type === 'audioRx' ? parseAudioBundle(rest) as unknown as Record<string, unknown>
        : parseAudioState(rest) as unknown as Record<string, unknown>
    } catch { this.station.peer.close(1008, 'invalidAudio'); return }
    const message = JSON.stringify(body)
    // The bound is checked on the forwarded message, which is what the browser's own
    // reader bounds - not on the station's, which carries an extra session id.
    if (new TextEncoder().encode(message).length > AUDIO_MESSAGE_BYTES) { this.station.peer.close(1008, 'invalidAudio'); return }
    const browser = this.browsers.get(sessionId)
    // A listener that has already gone. Dropping is correct: audio is never retried.
    if (!browser) { this.dropped++; return }
    try {
      browser.peer.send(message)
      this.failures.delete(sessionId)
    } catch {
      this.dropped++
      const count = (this.failures.get(sessionId) ?? 0) + 1
      this.failures.set(sessionId, count)
      // A browser that has stopped taking data for seconds is not coming back on this
      // socket. Closing it is what makes the station stop encoding, because the station
      // stops when nobody is listening.
      if (count >= DELIVERY_FAILURES) { this.failures.delete(sessionId); try { browser.peer.close(1011, 'audioBacklog') } catch { /* already gone */ } }
    }
  }

  private tell(peer: Peer, state: { type: 'audioState'; listening: boolean; reason: string }): void {
    try { peer.send(JSON.stringify(state)) } catch { /* the close path already owns this socket */ }
  }
}

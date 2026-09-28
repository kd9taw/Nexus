// The stream's signalling lane through the room: an offer, its answer and the trickled candidates
// that set up a direct WebRTC link between one page and its station, and the station's word on
// whether it is streaming. Once that link is up the picture, the receive audio and every control
// message travel on it, never through here. The wire contract is remote/test/fixtures/stream/.
//
// The audio lane's shape (audio-relay.ts), deliberately: it validates an envelope, checks a byte
// bound, stamps the page's identity and routes. No checkpoint, no storage, no alarm and no ACK
// credit - a signal is worthless the moment its negotiation is over, and one lost to a hibernation
// is a negotiation the page retries. It never interprets an SDP; the two ends do that.
//
// WHO MAY SIGNAL is decided where it always is. The relay admits the page's session as the
// observation lane does (an approved device on an entitled account), stamps who it is, and hands
// the station the lease the page claims; the station alone decides whether that session holds
// station control (A3) before any WebRTC state exists there. The relay adds two refusals of its
// own, both about the service rather than the radio: a station that never advertised the lane is
// never handed a message (its parser would refuse it and drop the control socket), and an OFFER
// from an account whose command entitlement has lapsed is refused here, the same deadline the
// operation lane holds every command to - starting a stream is starting to command the station.
//
// AND A LAPSE ENDS A STREAM ALREADY RUNNING. Once up, a stream commands the station peer to peer,
// where no relay can gate it; so the relay remembers which sessions the station said are streaming,
// and when one's deadline lapses it tells the station `streamEnd` (roomToStationEnd), once, with
// `remoteOff` when the account's Remote access was switched off by hand (operator decision,
// 2026-09-27: "within about 2 s") and `serviceAccessExpired` otherwise. The station ends that
// session's transmit presence at once and says its last `streamState` to the page itself.
import type { Peer } from '../remote-monitor/relay'
import {
  STREAM_BROWSER_BYTES, STREAM_STATION_BYTES, parseBrowserSignal, parseStationMessage, type StationStreamMessage, type StreamStateReason,
} from './stream-protocol'

/** `commandUntil` is the room's per-session command deadline (room.ts `commandDeadline`): zero, or
 *  past, means this account may not start commanding the station. `accessOff` says the last reading
 *  of it found Remote access switched off by hand, which is what a stream it ends is told. */
type Browser = { sessionId: string; deviceId: string; peer: Peer; commandUntil: number; accessOff?: boolean }
type Station = { peer: Peer; supported: boolean } | null
/** A negotiation is one offer plus a handful of candidates each way, and a page that reconnects
 *  after a drop does it again. Forty-eight signals in ten seconds is several whole negotiations; a
 *  page over it is looping, and what it sends is refused with `tryLater`, never forwarded. */
export const STREAM_RATE_WINDOW_MS = 10_000
export const STREAM_RATE_LIMIT = 48
/** While a stream runs, how stale the room's reading of its session's entitlement may be. The room
 *  re-reads as the page's socket heartbeats arrive, about one a second, so Remote switched off by
 *  hand ends the stream within about 2 s (operator decision, 2026-09-27); the command lane's own 2 s
 *  budget would allow about 3. One indexed single-row read a second, for a session that is already
 *  carrying live video. */
export const STREAM_ENTITLEMENT_CHECK_MS = 1000

const bytes = (text: string) => new TextEncoder().encode(text).length

export class StreamRelay {
  private station: Station = null
  private browsers = new Map<string, Browser>()
  private rates = new Map<string, number[]>()
  /** Sessions the station has said are streaming, and has not since said are not. */
  private live = new Set<string>()
  /** Station messages the relay could not deliver. Diagnostics only; nothing acts on it. */
  dropped = 0

  sync(station: Station, browsers: Browser[]): void {
    const next = new Map(browsers.map(b => [b.sessionId, b]))
    for (const id of [...this.rates.keys()]) if (!next.has(id)) this.rates.delete(id)
    for (const id of [...this.live]) if (!next.has(id)) this.live.delete(id)
    this.station = station
    this.browsers = next
  }

  /** A page's signal. Its identity is stamped HERE: a session may only ever signal as itself. */
  receiveBrowser(sessionId: string, raw: unknown, now: number): void {
    const browser = this.browsers.get(sessionId)
    if (!browser) return
    let signal
    try { signal = parseBrowserSignal(raw) }
    catch { browser.peer.close(1008, 'invalidStream'); return }
    // A close always goes through: ending a negotiation is never what a budget protects against.
    if (signal.payload.kind !== 'close') {
      const recent = (this.rates.get(sessionId) ?? []).filter(at => now - at < STREAM_RATE_WINDOW_MS)
      if (recent.length >= STREAM_RATE_LIMIT) { this.refuse(browser.peer, 'tryLater'); return }
      recent.push(now)
      this.rates.set(sessionId, recent)
    }
    // Not a protocol error and not a close: a page that knows the lane may meet a station that does
    // not. Say so and carry on.
    if (!this.station?.supported) { this.refuse(browser.peer, 'streamUnavailable'); return }
    // Read defensively, as the operation lane does: an absent deadline is "not entitled", never
    // "entitled forever".
    const commandUntil = Number.isSafeInteger(browser.commandUntil) ? browser.commandUntil : 0
    if (signal.payload.kind === 'offer' && now >= commandUntil) { this.refuse(browser.peer, 'serviceAccessExpired'); return }
    const message = JSON.stringify({ type: 'streamSignal', sessionId, deviceId: browser.deviceId, leaseId: signal.leaseId, payload: signal.payload })
    if (bytes(message) > STREAM_STATION_BYTES) { browser.peer.close(1008, 'invalidStream'); return }
    if (signal.payload.kind === 'close') this.live.delete(sessionId)
    try { this.station.peer.send(message) }
    catch { this.refuse(browser.peer, 'streamUnavailable') }
  }

  /** Is this session's stream running, on the station's own word? */
  streaming(sessionId: string): boolean { return this.live.has(sessionId) }

  /** End every running stream whose session may no longer command the station. */
  enforce(now: number): void {
    for (const sessionId of [...this.live]) {
      const browser = this.browsers.get(sessionId)
      if (browser && now >= (Number.isSafeInteger(browser.commandUntil) ? browser.commandUntil : 0)) this.end(sessionId, browser.accessOff ? 'remoteOff' : 'serviceAccessExpired')
    }
  }
  private end(sessionId: string, reason: 'remoteOff' | 'serviceAccessExpired'): void {
    this.live.delete(sessionId)
    try { this.station?.peer.send(JSON.stringify({ type: 'streamEnd', sessionId, reason })) }
    catch { this.dropped++ }
  }

  /** A signal or a state from the station, addressed to one session. */
  receiveStation(raw: unknown, now: number): void {
    if (!this.station) return
    let message: StationStreamMessage, forwarded: string
    try {
      // A station that never advertised the lane has no business sending on it.
      if (!this.station.supported) throw Error('invalidStream')
      message = parseStationMessage(raw)
      const { sessionId: _address, ...rest } = message
      forwarded = JSON.stringify(rest)
      if (bytes(forwarded) > STREAM_BROWSER_BYTES) throw Error('invalidStream')
    } catch { this.station.peer.close(1008, 'invalidStream'); return }
    const sessionId = message.sessionId, browser = this.browsers.get(sessionId)
    // A page that has already gone. Dropping is correct: its negotiation went with it.
    if (!browser) { this.dropped++; return }
    try { browser.peer.send(forwarded) }
    catch { this.dropped++ }
    if (message.type !== 'streamState') return
    if (!message.streaming) { this.live.delete(sessionId); return }
    this.live.add(sessionId)
    // Admitted on an offer that went before the deadline lapsed: ended as soon as it is up.
    this.enforce(now)
  }

  private refuse(peer: Peer, reason: StreamStateReason): void {
    try { peer.send(JSON.stringify({ type: 'streamState', streaming: false, reason })) }
    catch { /* the close path already owns this socket */ }
  }
}

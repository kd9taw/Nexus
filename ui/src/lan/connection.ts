// One road to a station on this network, as the stream view uses a connection: the same four parts
// the hosted connection hands it (operations, stream, audio, source), carried on the loopback
// socket to this computer's Nexus, which carries them on to the station over the pinned TLS road
// (src-tauri/src/lan_client/road.rs).
//
// - The keys never enter this page. The offer goes unsigned from here; this computer's Nexus signs
//   it with its own key for the station (A5) on its way. The answer is checked there against the
//   station's pinned key before this page is handed it, and checked again here with the hosted
//   page's own code (S3-M1), against the same key.
// - No STUN and no relay: the stream goes direct on the shack's own network, with no internet.
// - The status line is the station's word on whether its rig is keyed, for the ▲ TX and the
//   microphone's mute (M9), as the relay's observation is on the hosted road: it is handed to the
//   view as the one reading a frame carries, and nothing else.
import { OperationClient } from '../remote-web/operation-client'
import { StreamLink, browserStream, type StreamEnvironment } from '../remote-web/stream-link'
import { AudioLink } from '../remote-web/audio-listen'
import { checkAnswer } from '../remote-web/station-key'
import type { MonitorSource } from '../remote-monitor/session'
import type { MonitorFrame } from '../remote-monitor/protocol'
import type { LanRoad } from './protocol'

/** The operation version every request on this road is answered at: the station's lane's own. */
export const LAN_OPERATION_VERSION = 4

/** The stream's environment on this road: the browser's own, with no ICE server at all. */
export function lanStream(): StreamEnvironment {
  const browser = browserStream()
  return { ...browser, peer: () => browser.peer([]) }
}

/** The station's status line as the one reading a monitor frame carries. */
function statusFrame(keyed: boolean | null, sequence: number, session: string): MonitorFrame {
  return {
    version: 2, source: 'native', epoch: session, sequence, generatedAtMs: 0,
    station: {
      call: '', grid: '', amplifier: null,
      radio: {
        id: 0, name: '', dialMhz: null, band: '', mode: '', rigMode: null, catConnected: null, rigKeyed: keyed,
        nexusBusy: false, rigDialMhz: null,
        readings: { cat: null, dial: null, mode: null, ptt: keyed === null ? null : { connectionGeneration: 1, readSequence: sequence, ageMs: 0 } },
      },
    },
  }
}

export class LanConnection {
  readonly operations: OperationClient
  readonly stream: StreamLink
  /** Listen without a picture is the relay's lane, which this road does not carry: it is never
   *  offered here, and anything asked of it is refused. The stream's own audio rides its WebRTC
   *  channel. */
  readonly audio: AudioLink
  readonly source: MonitorSource
  private keyed: boolean | null = null
  private heard = false
  private sequence = 0
  private open = true

  /** `send` hands a message to this computer's Nexus on the page's socket; it throws when the socket
   *  will not take it. */
  constructor(private readonly send: (text: string) => void, readonly road: LanRoad, env: StreamEnvironment = lanStream()) {
    const ids = { stationId: road.stationId, deviceId: road.deviceId, sessionId: road.sessionId }
    this.operations = new OperationClient(message => this.carry(message), true, () => performance.now(), undefined, LAN_OPERATION_VERSION)
    this.stream = new StreamLink((payload, leaseId) => this.carry(JSON.stringify({ type: 'streamSignal', leaseId, payload })), {
      ...env,
      relayServers: undefined,
      signOffer: undefined,
      verifyAnswer: (answer, offer) => checkAnswer(road.stationKey, answer, offer, ids),
    })
    this.audio = new AudioLink(() => { throw new Error('stationUnsupported') }, env.audio)
    this.source = {
      id: `lan-${road.stationId}`, kind: 'native',
      read: async () => {
        if (!this.open || !this.heard) throw new Error('stationUnavailable')
        return statusFrame(this.keyed, ++this.sequence, road.sessionId)
      },
    }
    this.operations.open()
  }

  private carry(text: string): void {
    if (!this.open) throw new Error('stationUnavailable')
    this.send(text)
  }

  /** One of the station's own messages, for the part it is for. A malformed one costs what it was
   *  for, never the road: the operation client refuses its own, the stream ends on its own. */
  receive(message: Record<string, unknown>, bytes: number): void {
    if (message.type === 'operationResponse') {
      try { this.operations.receive(message, bytes) } catch { /* refused by the client itself */ }
    } else if (message.type === 'streamSignal' || message.type === 'streamState') {
      this.stream.receive(message)
    } else if (message.type === 'status') {
      this.heard = true
      this.keyed = typeof message.rigKeyed === 'boolean' ? message.rigKeyed : null
    }
  }

  /** This computer's Nexus refused the station's answer: the stream ends here, saying why. */
  answerRefused(reason: string): void { this.stream.close(reason) }

  /** The road has closed: nothing more can be sent, and every part says so. */
  closed(): void {
    if (!this.open) return
    this.open = false
    this.operations.disconnected()
    this.stream.disconnected()
    this.audio.close()
  }

  /** Permanent: the page has finished with this road. */
  dispose(): void {
    this.closed()
    this.stream.dispose()
  }
}

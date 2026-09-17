// The cloud retains only bounded routing metadata. Native Nexus alone owns
// permission, leases, command windows, deduplication and durable outcomes.
import type { Peer } from '../remote-monitor/relay'
import { object } from './display-validation'
import { operationEvent, operationId, operationRequest, operationResponse } from './operation-protocol'
import { controlVersion, parseOperationVersion, type OperationVersion } from './operation-version'
import { OPERATION_RATE_LIMIT, OPERATION_RATE_WINDOW_MS } from './operation-limits'
// `commandUntil` is how long this browser's account may still command the station: the adapter
// sets it from the entitlement it read at admission or at the last renewal, and it is deliberately
// shorter than the observer lease (see OPERATION_ENTITLEMENT_MS). Zero means never.
type Browser = { sessionId: string; deviceId: string; peer: Peer; commandUntil: number }
/** The requests that may not be forwarded once the entitlement behind the session has lapsed:
 *  everything that CHANGES the station. Exported because the adapter has to recognise the same
 *  set one step earlier, to refresh the entitlement before this relay is asked - two spellings
 *  of "which commands are gated" would drift, and the drift would be silent and open. */
export function entitledCommand(type: unknown): boolean {
  return type === 'stationControl' || type === 'logManual' || type === 'logChange'
}
type Pending = {
  requestId: string
  sessionId: string
  deviceId: string
  at: number
  mutation: boolean
  stop?: true
  operationVersion?: OperationVersion
}
export type OperationCheckpoint = {
  version: 1
  pending: { requestId: string; at: number; mutation: boolean; stop?: true; operationVersion?: OperationVersion }[]
}
// Socket close notifications can race revocation. A lost send must close its
// peer without aborting the authoritative revocation/checkpoint operation.
function deliver(peer: Peer, message: string): boolean {
  try {
    peer.send(message)
    return true
  } catch {
    try {
      peer.close(1011, 'stationUnavailable')
    } catch {}
    return false
  }
}
export class OperationRelay {
  private station: { peer: Peer; supported: boolean; operationVersion?: number } | null = null
  private browsers = new Map<string, Browser>()
  private pending = new Map<string, Pending>()
  private rates = new Map<string, number[]>()
  private stopRates = new Map<string, number[]>()
  sync(station: { peer: Peer; supported: boolean; operationVersion?: number } | null, browsers: Browser[], now: number): void {
    const next = new Map(browsers.map((b) => [b.sessionId, b]))
    for (const [id] of this.browsers)
      if (!next.has(id)) {
        if (this.station?.supported && this.station.peer === station?.peer)
          deliver(this.station.peer, JSON.stringify({ type: 'operationDisconnect', sessionId: id }))
        this.rates.delete(id)
        this.stopRates.delete(id)
      }
    if (this.station?.peer !== station?.peer) {
      for (const p of this.pending.values())
        this.error(p, p.mutation ? 'operationUnknown' : 'stationUnavailable')
      this.pending.clear()
    }
    this.station = station
    this.browsers = next
    for (const [id, p] of this.pending) if (!next.has(p.sessionId)) this.pending.delete(id)
    this.expire(now)
  }
  private error(p: Pending, error: string) {
    const peer = this.browsers.get(p.sessionId)?.peer
    if (peer)
      deliver(peer, JSON.stringify({ type: 'operationResponse', requestId: p.requestId, error }))
  }
  receiveBrowser(sessionId: string, raw: unknown, now: number): void {
    const browser = this.browsers.get(sessionId)
    if (!browser) return
    try {
      const versioned = !!raw && typeof raw === 'object' && 'operationVersion' in raw
      const wire = object(raw, ['type', 'request', ...(versioned ? ['operationVersion'] : [])])
      if (wire.type !== 'operationRequest') throw Error()
      const browserVersion = versioned ? parseOperationVersion(wire.operationVersion) : 1
      if (!browserVersion || versioned && browserVersion === 1) throw Error()
      const request = operationRequest(wire.request)
      if ((request.type === 'stationControl' || request.type === 'logChange' || request.type === 'activationExport' || request.type === 'programExport') && !versioned) throw Error()
      const p = {
        requestId: request.requestId,
        sessionId,
        deviceId: browser.deviceId,
        at: now,
        mutation: request.type === 'logManual' || request.type === 'stationControl' || request.type === 'stopTransmit' || request.type === 'logChange',
        ...(request.type === 'stopTransmit' ? { stop: true as const } : {})
      }
      // THE PER-COMMAND ENTITLEMENT GATE, and it is first because entitlement outranks every
      // other reason to refuse - an account that may not command must not learn the station's
      // capabilities from a more specific refusal either.
      //
      // Until this existed the operation lane was entitlement-safe only by accident: the room
      // happens to expire observers before dispatching a message, so a session whose lease had
      // run out no longer appeared in `browsers`. Nothing here said so, no test asserted it, and
      // the lease it inherited was a minute long. This states the rule in the lane that forwards
      // the command, so a reordering upstream cannot silently re-open unmetered radio control.
      //
      // TWO THINGS ARE DELIBERATELY NOT GATED.
      // - STOP. An operator who can no longer pay must still be able to unkey a transmitter;
      //   refusing a Stop over billing would leave a real radio keyed with its only remote
      //   control declining to help. Safety outranks entitlement, every time.
      // - READS. `state` is how the browser learns the stationBootId, leaseId and transmitEpoch
      //   that a Stop is composed from, so gating reads would gate Stop by the back door; and a
      //   lapsed session should be able to see what it is lapsing out of. Observation is
      //   untouched here and ends on its own lease.
      // What IS gated is every command that changes the station: stationControl, logManual and
      // logChange - `mutation` minus `stop`.
      //
      // `stationUnavailable` rather than a truer code because OPERATION_ERRORS is a closed list
      // that every shipped browser parses strictly: a new code would fail their parser and take
      // the socket down. It is honest about the outcome - the command did not reach the station
      // and nothing changed - and it is NOT `operationUnknown`, which would wrongly offer the
      // operator a "did that land?" check for a command that was never sent.
      //
      // The deadline is read defensively rather than trusted from the type. A caller that omits
      // it leaves `undefined`, and `now >= undefined` is false - which would wave the command
      // through. An absent deadline has to mean "not entitled", never "entitled forever".
      const commandUntil = Number.isSafeInteger(browser.commandUntil) ? browser.commandUntil : 0
      if (entitledCommand(request.type) && now >= commandUntil) {
        this.error(p, 'stationUnavailable')
        return
      }
      if (!this.station?.supported) {
        this.error(p, 'stationUnsupported')
        return
      }
      const stationVersion = parseOperationVersion(this.station.operationVersion) ?? 1
      const version = Math.min(browserVersion, stationVersion) as OperationVersion
      // A station older than v4 cannot parse a log change at all: refusing here keeps it off that wire.
      if ((request.type === 'stationControl' && version < controlVersion(request.action)) || ((request.type === 'stopTransmit' || request.type === 'logChange' || request.type === 'activationExport' || request.type === 'programExport') && version < 4)) {
        this.error(p, 'stationUnsupported')
        return
      }
      const rates = p.stop ? this.stopRates : this.rates
      const rate = (rates.get(sessionId) ?? []).filter((at) => now - at < OPERATION_RATE_WINDOW_MS)
      const mine = [...this.pending.values()].filter((pending) => pending.sessionId === sessionId && !!pending.stop === !!p.stop)
      // One bounded Stop can pass a pending write/control and a heartbeat. It
      // has its own rate budget; ordinary requests cannot consume that budget.
      if (rate.length >= (p.stop ? 2 : OPERATION_RATE_LIMIT) || mine.length >= (p.stop ? 1 : 2) || (!p.stop && p.mutation && mine.some((p) => p.mutation))) {
        this.error(p, 'remoteBusy')
        return
      }
      if (this.pending.has(p.requestId)) {
        this.error(p, 'requestConflict')
        return
      }
      rate.push(now)
      rates.set(sessionId, rate)
      this.pending.set(p.requestId, { ...p, operationVersion: version })
      if (
        !deliver(
          this.station.peer,
          JSON.stringify({
            type: 'operationRequest',
            sessionId,
            deviceId: browser.deviceId,
            ...(version >= 2 ? { operationVersion: version } : {}),
            request
          })
        )
      ) {
        this.pending.delete(p.requestId)
        this.error(p, p.mutation ? 'operationUnknown' : 'stationUnavailable')
      }
    } catch {
      browser.peer.close(1008, 'invalidOperation')
    }
  }
  receiveStation(raw: unknown): void {
    if (!this.station?.supported) {
      this.station?.peer.close(1008, 'invalidOperation')
      return
    }
    try {
      if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw Error()
      const { sessionId, ...value } = raw as Record<string, unknown>
      if (!operationId(sessionId)) throw Error()
      // Operation v5: a settled control, pushed. The relay holds nothing for it - no pending
      // entry, no rate, nothing to checkpoint - so it is validated and handed on, and the room
      // returns without its per-message checkpoint (the audio lane's path). Only a station that
      // advertised v5 may push at all; the station itself pushes only for a control the browser
      // sent at v5, which is what keeps an older page - whose socket would close on the unknown
      // type - from ever seeing one.
      if (value.type === 'operationEvent') {
        if ((parseOperationVersion(this.station.operationVersion) ?? 1) < 5) throw Error()
        const event = operationEvent(value)
        const peer = this.browsers.get(sessionId)?.peer
        if (peer) deliver(peer, JSON.stringify(event))
        return
      }
      const response = operationResponse(value),
        p = this.pending.get(response.requestId)
      // A closed observer or expired request may leave a legitimate late reply.
      if (!p) return
      if (p.sessionId !== sessionId) throw Error()
      if ('value' in response && !!p.stop !== ('stop' in response.value)) throw Error()
      this.pending.delete(response.requestId)
      if ('value' in response && 'phase' in response.value && (p.operationVersion ?? 2) < 4) {
        delete response.value.transmitEpoch
        response.value.txArmed = false
        if (response.value.controls) response.value.controls.capabilities = response.value.controls.capabilities.filter(c => c !== 'ftRuntime' && c !== 'ftSettings' && c !== 'qsoLogging' && c !== 'ftOperate' && c !== 'ftCall' && c !== 'ftExchange' && c !== 'ftMessages')
      }
      if ('value' in response && 'controls' in response.value && response.value.controls) {
        // Keep capability vocabulary compatible even with pre-v3 stations that
        // advertised extra hints under v2. This only removes hints; it cannot
        // grant a command. Historical checkpoints predate v3 and default to v2.
        const version = p.operationVersion ?? 2
        if (version === 1) delete response.value.controls
        else if (version === 2) response.value.controls.capabilities = response.value.controls.capabilities.filter(c => ['decoder', 'radio', 'amplifier'].includes(c))
      }
      const peer = this.browsers.get(sessionId)?.peer
      if (peer) deliver(peer, JSON.stringify(response))
    } catch {
      this.station?.peer.close(1008, 'invalidOperation')
    }
  }
  expire(now: number) {
    for (const [id, p] of this.pending)
      if (now - p.at >= 7000) {
        this.pending.delete(id)
        this.error(p, p.mutation ? 'operationUnknown' : 'stationUnavailable')
      }
  }
  nextDeadline(): number | null {
    return this.pending.size
      ? Math.min(...[...this.pending.values()].map((p) => p.at + 7000))
      : null
  }
  checkpoint(sessionId: string): OperationCheckpoint {
    return {
      version: 1,
      pending: [...this.pending.values()]
        .filter((p) => p.sessionId === sessionId)
        .map(({ requestId, at, mutation, stop, operationVersion }) => ({ requestId, at, mutation, ...(stop ? { stop } : {}), ...(operationVersion ? { operationVersion } : {}) }))
    }
  }
  restore(sessionId: string, raw: OperationCheckpoint): void {
    const b = this.browsers.get(sessionId)
    if (!b || raw.version !== 1 || !Array.isArray(raw.pending) || raw.pending.length > 3)
      throw Error('invalidOperationCheckpoint')
    if (raw.pending.filter(p => p.stop).length > 1 || raw.pending.filter(p => !p.stop).length > 2) throw Error('invalidOperationCheckpoint')
    const seen = new Set<string>()
    for (const p of raw.pending) {
      object(p, ['requestId', 'at', 'mutation', ...('stop' in p ? ['stop'] : []), ...('operationVersion' in p ? ['operationVersion'] : [])])
      if (
        !operationId(p.requestId) ||
        !Number.isSafeInteger(p.at) ||
        p.at < 0 ||
        typeof p.mutation !== 'boolean' ||
        ('stop' in p && (p.stop !== true || p.operationVersion !== 4 || !p.mutation)) ||
        seen.has(p.requestId) ||
        ('operationVersion' in p && !parseOperationVersion(p.operationVersion)) ||
        this.pending.has(p.requestId)
      )
        throw Error('invalidOperationCheckpoint')
      seen.add(p.requestId)
    }
    for (const p of raw.pending) this.pending.set(p.requestId, { ...p, sessionId, deviceId: b.deviceId })
  }
}

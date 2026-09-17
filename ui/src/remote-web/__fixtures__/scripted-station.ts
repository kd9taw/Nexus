// A SCRIPTED STATION on a link of a chosen round trip, for the responsiveness twin and the panel
// test. It answers the operation lane the way `operations.rs` does today — a rig-touching control
// is `pending` by construction, its outcome is read back by the browser's `result` poll once the
// radio loop has applied it a CAT time later — and feeds the instrument stream the way the station
// does: a snapshot sample every 500 ms, sent whenever it holds the browser's credit. Every message
// that leaves the browser is logged with its fake-clock time, and every render-path notification
// is counted, so two sessions can be compared for identity.
import { vi } from 'vitest'
import { OperationClient } from '../operation-client'
import { WheelTuning } from '../wheel-tuning'
import { ApplicationClient } from '../application-client'
import { pendingControlStorage } from '../control-storage'
import { applicationCommands } from '../application-capabilities'
import type { OperationState } from '../operation-protocol'

/** The first application version whose stream carries the satellite state a Stop is checked against. */
export const SCRIPTED_APPLICATION_VERSION = 13
/** The radio's own time from CAT command to readback, as the plan budgets it. */
export const SCRIPTED_CAT_MS = 150
const DIAL: Record<string, number> = { '40m': 7.2, '20m': 14.2, '17m': 18.13 }
const BOOT = '11111111-1111-4111-8111-111111111111', LEASE = '22222222-2222-4222-8222-222222222222'

/** `version` 4 leaves the browser to discover a control's outcome by its `result` poll; 5 is
 * push-completion — the station sends the settled outcome, with the state a `state` read would
 * return, the moment its radio loop lands the target. `push` turns that event off at v5 and
 * `pollSettles: false` makes every `result` read answer pending forever: between them a test can
 * hold each confirmation path on its own. */
export type ScriptedOptions = {
  push?: boolean
  pollSettles?: boolean
  /** Whether the station NAMES `outcomePush` in its capabilities, as a real v5 station does the
   * moment the relay agrees v5 (684bfb70): the page trusts that word, never its own version, to
   * defer the post-command polls. Defaults to `push`; a pushing station that stays silent is not
   * a station that ships, and it makes the page poll and push at once. */
  outcomePush?: boolean
  /** The radio's own time from CAT command to readback. */
  catMs?: number
}
export function scriptedStation(rttMs: number, version: 4 | 5 = 4, options: ScriptedOptions = {}) {
  const push = options.push ?? version >= 5, pollSettles = options.pollSettles ?? true
  const outcomePush = options.outcomePush ?? push, catMs = options.catMs ?? SCRIPTED_CAT_MS
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance', 'Date', 'requestAnimationFrame', 'cancelAnimationFrame'] })
  const half = rttMs / 2
  const wire: { at: number; type: string }[] = []
  const notifications = { operations: 0, tuning: 0 }
  const radio = { dialMhz: 14.2, band: '20m', sideband: 'USB', operatingMode: 'phone', source: 'native', catOk: true,
    txEnabled: false, transmitting: false, rigKeyed: false, tuning: false, txBusyReason: null as string | null }
  let revision = 1, sequence = 1, windowId = crypto.randomUUID()
  const state = (): OperationState => ({ stationBootId: BOOT, allowed: true, phase: 'controlling', leaseId: LEASE, revision, commandWindowId: windowId,
    nextSequence: sequence, leaseRemainingMs: 5000, actions: [], txArmed: false, transmitEpoch: '000000000000002a',
    controls: { context: { radioId: 1, radioConnection: 7, ampConnection: null, ampReadSequence: null },
      capabilities: ['frequency', 'bandSelection', ...(outcomePush ? ['outcomePush' as const] : [])] } })
  const completions = new Map<string, { done: boolean }>()
  const values = new Map<string, string>()
  const storage = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v) }, removeItem: (k: string) => { values.delete(k) } }
  const operations = new OperationClient(raw => {
    const { request } = JSON.parse(raw)
    wire.push({ at: performance.now(), type: request.type })
    setTimeout(() => { // arrives at the station
      const reply = (value: unknown) => setTimeout(() => operations.receive({ type: 'operationResponse', requestId: request.requestId, value }), half)
      switch (request.type) {
        case 'state': case 'heartbeat': reply(state()); break
        case 'stationControl': {
          // Queued for the radio loop: the windows are consumed now, the radio moves a CAT time later.
          revision++; sequence++; windowId = crypto.randomUUID()
          const completion = { done: false }, action = request.action
          completions.set(request.requestId, completion)
          setTimeout(() => {
            if (action.action === 'radio.frequency') radio.dialMhz = action.dialMhz
            else if (action.action === 'radio.band') { radio.band = action.band; radio.dialMhz = DIAL[action.band] }
            completion.done = true
            if (push) setTimeout(() => operations.receiveEvent({ type: 'operationEvent', operationId: request.requestId,
              value: { operation: 'stationControl', operationId: request.requestId, outcome: 'applied', evidence: 'radioReadback' }, state: state() }), half)
          }, catMs)
          reply({ operation: 'stationControl', operationId: request.requestId, outcome: 'pending' })
          break
        }
        case 'result': {
          const completion = completions.get(request.operationId)
          reply({ operation: 'stationControl', operationId: request.operationId, ...(completion?.done && pollSettles ? { outcome: 'applied', evidence: 'radioReadback' } : { outcome: 'pending' }) })
          break
        }
        case 'stopTransmit': reply({ stop: 'accepted' }); break
      }
    }, half)
  }, true, () => performance.now(), undefined, version, pendingControlStorage(() => storage, 'scripted-station', async (_key, run) => run()))
  operations.subscribe(() => { notifications.operations++ })
  // The instrument stream: one sample per 500 ms on the station's clock, sent on the credit it holds.
  let topics: string[] = [], credit: string | null = null, frameRevision = 0
  const choices = ['40m', '20m', '17m'].map(band => ({ band, group: 'HF', dialMhz: DIAL[band], mode: 'USB', label: band, note: '', tx: true }))
  const sample = (command: string): unknown => command === 'get_snapshot' ? { mycall: 'TEST', mygrid: 'EN52', mode: 'FT8', activeRadioId: 1, radio: { ...radio }, link: { tier: 'FT8' } }
    : command === 'get_settings' ? { bandChoices: { cw: choices, phone: choices } }
    : command === 'get_remote_satellite_state' ? { capturedAtMs: Date.now(), settings: { mygrid: 'EN52', rotatorConfigured: false, satDopplerOff: false, satVfoMap: 'off', radioPegged: false }, track: null, held: null }
    : {}
  const application = new ApplicationClient(raw => {
    const message = JSON.parse(raw)
    wire.push({ at: performance.now(), type: message.type })
    setTimeout(() => {
      if (message.type === 'applicationSubscribe') { topics = message.topics; credit = message.requestId }
      else if (message.type === 'applicationFrameAck') credit = message.nextRequestId
    }, half)
  }, () => { throw new Error('closed') }, SCRIPTED_APPLICATION_VERSION)
  const sampler = setInterval(() => {
    if (!credit || !topics.length) return
    const requestId = credit
    credit = null; frameRevision++
    const updates = topics.map(command => ({ type: 'applicationResult', requestId, command, revision: frameRevision, baseRevision: null, ageMs: 0, data: sample(command), removed: [] }))
    setTimeout(() => application.receive({ type: 'applicationFrame', requestId, updates }), half)
  }, 500)
  application.open()
  application.receive({ type: 'applicationCapabilities', version: SCRIPTED_APPLICATION_VERSION, commands: applicationCommands(SCRIPTED_APPLICATION_VERSION) })
  const failures: string[] = []
  const tuning = new WheelTuning(operations, application, error => failures.push(error.message))
  tuning.subscribe(() => { notifications.tuning++ })
  tuning.activate()
  operations.open()
  const close = () => { clearInterval(sampler); tuning.dispose(); operations.disconnected(); application.disconnected(); vi.useRealTimers() }
  return { operations, tuning, application, wire, notifications, failures, radio, close, rttMs, version }
}

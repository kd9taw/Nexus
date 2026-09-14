import { afterEach, expect, it, vi } from 'vitest'
import { OperationClient } from './operation-client'
import { OperationRelay } from './operation-relay'
import {
  LOG_CAPABILITIES,
  logChange,
  logChangeCapabilities,
  operationRequest,
  operationValue,
  type LogChange,
  type OperationState
} from './operation-protocol'
import {
  SETTINGS_KEYS,
  WITHHELD_SETTINGS_KEYS,
  WRITABLE_CONTROL_SETTINGS_KEYS,
  WRITABLE_LOGGING_SETTINGS_KEYS
} from './configuration-schema'

const id = () => crypto.randomUUID()
const revision = 'b'.repeat(64)
const envelope = (change: unknown) => ({ type: 'logChange', requestId: id(), stationBootId: id(), leaseId: id(),
  expectedRevision: 1, commandWindowId: id(), clientSequence: 1, change })
const state = (capabilities: string[]): OperationState => ({ stationBootId: id(), allowed: true, phase: 'controlling', leaseId: id(),
  revision: 1, commandWindowId: id(), nextSequence: 1, leaseRemainingMs: 5000, actions: ['log.manual'], txArmed: false,
  transmitEpoch: null,
  controls: { context: { radioId: 1, radioConnection: null, ampConnection: null, ampReadSequence: null }, capabilities } as OperationState['controls'] })
const logging: LogChange = { kind: 'settings', revision, values: { autoLog: true } }
const control: LogChange = { kind: 'settings', revision, values: { contestCheck: '73' } }
const mixed: LogChange = { kind: 'settings', revision, values: { autoLog: true, contestCheck: '73' } }
afterEach(() => vi.useRealTimers())

it('writes only settings on the allow-list, each in its own type, against the document revision it showed', () => {
  const macros = { activeCwProfile: 0, band: ['QRZ?'], chat: ['73'], cw: [], cwProfiles: [], qso: ['RR73'] }
  for (const change of [logging, control, mixed,
    { kind: 'settings', revision, values: { decodeDepth: 2, decodeFLowHz: 300, contestCqZone: 4, apDecode: false, propEngine: 'p533' } },
    { kind: 'settings', revision, values: { macros } }]) {
    expect(logChange(change)).toEqual(change)
    expect(operationRequest(envelope(change)).type).toBe('logChange')
  }
  // Positive control on the denial: the same change naming a setting off the list is refused.
  for (const values of [
    { rigPort: 'COM3' }, { serialPort: '/dev/ttyUSB0' }, { rigModel: 1 }, { licenseClass: 'Extra' }, { mycall: 'W1AW' },
    { audioOut: 'Speakers' }, { betaUpdates: true }, { clusterHosts: [] }, { saveWav: 'all' }, { txEven: true }, { dialMhz: 14.074 },
    { tunePowerPct: 100 }, { alertCq: true }, { soundTxState: true }, { specialOp: 'hound' }, { fdActive: true }, { lotwAutoUpload: true },
    { clublogApiKey: 'secret' }, { autoLog: true, rigPort: 'COM3' }, {},
    { autoLog: 'yes' }, { decodeDepth: '2' }, { contestCheck: 7 }, { macros: [] }, { units: null }
  ])
    expect(() => logChange({ kind: 'settings', revision, values }), JSON.stringify(values)).toThrow()
  for (const bad of [
    { kind: 'settings', revision: 'B'.repeat(64), values: { autoLog: true } },
    { kind: 'settings', revision: 'b'.repeat(63), values: { autoLog: true } },
    { kind: 'settings', values: { autoLog: true } },
    { kind: 'settings', revision, values: null },
    { kind: 'settings', revision, values: [true] },
    { kind: 'settings', revision, values: { autoLog: true }, target: { call: 'W1AW', whenUnix: 1, key: 'a'.repeat(64) } }
  ])
    expect(() => logChange(bad), JSON.stringify(bad)).toThrow()
  // A key inherited from the object prototype is not a setting either.
  expect(() => logChange({ kind: 'settings', revision, values: JSON.parse('{"__proto__":{"autoLog":true}}') })).toThrow()
  expect(logChangeCapabilities(logging)).toEqual(['settingsLogging'])
  expect(logChangeCapabilities(control)).toEqual(['settingsControl'])
  expect(logChangeCapabilities(mixed)).toEqual(['settingsLogging', 'settingsControl'])
  const operationId = id()
  const saved = { operation: 'logChange', operationId, outcome: 'applied', evidence: 'settingsSaved' }
  expect(operationValue(saved)).toEqual(saved)
})

it('keeps the browser allow-list inside the exposed settings and off anything that keys, tunes, sounds or names a device', () => {
  const writable: string[] = [...WRITABLE_CONTROL_SETTINGS_KEYS, ...WRITABLE_LOGGING_SETTINGS_KEYS]
  expect(writable.length, 'positive control: the lists are really read').toBeGreaterThan(20)
  expect(new Set(writable).size).toBe(writable.length)
  for (const key of writable) {
    expect(SETTINGS_KEYS as readonly string[], key).toContain(key)
    expect(WITHHELD_SETTINGS_KEYS as readonly string[], key).not.toContain(key)
  }
  for (const key of ['mycall', 'licenseClass', 'serialPort', 'rigModel', 'audioIn', 'audioOut', 'txLevel', 'betaUpdates', 'clusterHosts',
    'saveWav', 'saveQsoWav', 'dialMhz', 'band', 'txEven', 'txWatchdogMin', 'tunePowerPct', 'soundTxState', 'announceVerbosity', 'alertCq',
    'specialOp', 'qsyEnabled', 'radios', 'activeRadio', 'ampFollowBand', 'rxGain', 'fdActive', 'lotwAutoUpload', 'js8Autoreply',
    'wantedCalls', 'blockedCalls', 'pounceThreshold', 'satDopplerOff', 'rotatorPort', 'unassistedMode', 'openingRegional'])
    expect(writable, key).not.toContain(key)
})

function open(capabilities: string[]) {
  vi.useFakeTimers()
  const sent: Record<string, any>[] = []
  const client = new OperationClient(s => sent.push(JSON.parse(s)), true, () => 1000, { read: () => null, write: () => {} }, 4)
  client.open()
  const reply = (value: unknown) => client.receive({ type: 'operationResponse', requestId: sent[sent.length - 1]!.request.requestId, value })
  reply(state(capabilities))
  return { client, reply, changes: () => sent.filter(m => m.request.type === 'logChange').map(m => m.request) }
}

it('sends a settings change only when the station offers every grant it needs, so an older desktop is never sent one', async () => {
  const cases: [string[], LogChange[], LogChange[]][] = [
    [['settingsLogging'], [logging], [control, mixed]],
    [['settingsControl'], [control], [logging, mixed]],
    [['settingsLogging', 'settingsControl'], [logging, control, mixed], []],
    // A desktop from before this change offers neither.
    [['qsoLogging', 'logEdit', 'qslMarks', 'otaHunt', 'otaActivation'], [], [logging, control, mixed]]
  ]
  for (const [capabilities, allowed, refused] of cases) {
    for (const change of refused) {
      const { client, changes } = open(capabilities)
      await expect(client.change(change)).rejects.toMatchObject({ message: 'notController', sent: false })
      expect(changes()).toEqual([])
      client.disconnected()
    }
    for (const change of allowed) {
      const { client, reply, changes } = open(capabilities)
      const result = client.change(change)
      const [request] = changes()
      expect(request).toMatchObject({ type: 'logChange', change })
      reply({ operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' })
      expect(await result).toMatchObject({ outcome: 'applied', evidence: 'settingsSaved' })
      client.disconnected()
    }
  }
})

it('an older hosted page tolerates the settings hints, which are bounded tokens it drops', () => {
  for (const hint of ['settingsControl', 'settingsLogging']) {
    expect(LOG_CAPABILITIES).toContain(hint)
    expect(hint).toMatch(/^[a-z][a-zA-Z0-9]{0,31}$/)
  }
})

it('the relay carries a settings change at v4 and closes a page that names a setting off the list', () => {
  const station = { send: vi.fn<(s: string) => void>(), close: vi.fn() }
  const browser = { send: vi.fn<(s: string) => void>(), close: vi.fn() }
  const sessionId = id(), deviceId = id(), relay = new OperationRelay()
  relay.sync({ peer: station, supported: true, operationVersion: 4 }, [{ sessionId, deviceId, peer: browser }], 100)
  const request = envelope(control)
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 3, request }, 101)
  expect(JSON.parse(browser.send.mock.lastCall![0]).error).toBe('stationUnsupported')
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4, request }, 102)
  expect(JSON.parse(station.send.mock.lastCall![0])).toMatchObject({ type: 'operationRequest', operationVersion: 4, request })
  const value = { operation: 'logChange', operationId: request.requestId, outcome: 'applied', evidence: 'settingsSaved' }
  relay.receiveStation({ type: 'operationResponse', sessionId, requestId: request.requestId, value })
  expect(JSON.parse(browser.send.mock.lastCall![0]).value).toEqual(value)
  station.send.mockClear()
  relay.receiveBrowser(sessionId, { type: 'operationRequest', operationVersion: 4,
    request: envelope({ kind: 'settings', revision, values: { rigPort: 'COM3' } }) }, 2000)
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidOperation')
  expect(station.send).not.toHaveBeenCalled()
})

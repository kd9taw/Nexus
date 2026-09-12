import { expect, it, vi } from 'vitest'
import { ACK_TIMEOUT_MS, ObservationRelay, observerDeadline } from './relay'
import type { BrowserIdentity, StationAccess } from './relay'
import fixtures from './fixtures.v2.json'

const access = (): StationAccess => ({ stationId: 'station-a', accountId: 'account-a', enabled: true,
  policyVersion: 1, stationGeneration: 1, devices: [{ id: 'browser-a', generation: 1, approved: true }] })
const browser = (): BrowserIdentity => ({ accountId: 'account-a', deviceId: 'browser-a', deviceGeneration: 1, expiresAt: 120000 })
const trial = () => ({ accountId: 'account-a', enabled: true, expiresAt: 120000,
  state: 'active' as const, startedAt: 0, source: 'trial' })
const peer = () => ({ send: vi.fn<(message: string) => void>(), close: vi.fn<(code: number, reason: string) => void>() })
const frame = (sequence: number) => JSON.stringify({ ...fixtures.spe, source: 'native', sequence })
const ack = (sequence: number) => JSON.stringify({ type: 'ack', epoch: fixtures.spe.epoch, sequence })
function connected() {
  const relay = new ObservationRelay(access())
  const station = peer()
  relay.connectStation({ accountId: 'account-a', stationId: 'station-a', generation: 1, expiresAt: 120000 }, station, 0)
  const viewer = peer()
  relay.connectObserver('session-a', browser(), trial(), viewer, 0)
  return { relay, station, viewer }
}

it('separates valid account identity, service entitlement and device approval', () => {
  expect(observerDeadline(access(), browser(), trial(), 0)).toBe(60000)
  for (const identity of [{ ...browser(), accountId: 'account-b' }, { ...browser(), deviceId: 'unknown' },
    { ...browser(), deviceGeneration: 2 }, { ...browser(), expiresAt: 0 }]) {
    expect(() => observerDeadline(access(), identity, trial(), 0)).toThrow('deviceNotApproved')
  }
  expect(() => observerDeadline({ ...access(), enabled: false }, browser(), trial(), 0)).toThrow()
  for (const entitlement of [{ ...trial(), enabled: false }, { ...trial(), expiresAt: 0 }, { ...trial(), accountId: 'account-b' }]) {
    expect(() => observerDeadline(access(), browser(), entitlement, 0)).toThrow('serviceAccessExpired')
  }
})

it('routes real frames only to approved observers and asks idle stations to stop publishing', () => {
  const { relay, station, viewer } = connected()
  expect(station.send.mock.calls.map(([s]) => JSON.parse(s))).toEqual([{ type: 'watch', enabled: false }, { type: 'watch', enabled: true }])
  const outsider = peer()
  expect(() => relay.connectObserver('outside', { ...browser(), accountId: 'account-b' }, trial(), outsider, 0)).toThrow()
  relay.receiveStation(station, frame(1), 0)
  expect(JSON.parse(viewer.send.mock.calls[0][0]).frame.station.radio.rigDialMhz).toBe(14.074)
  expect(outsider.send).not.toHaveBeenCalled()
  relay.disconnectObserver('session-a')
  expect(JSON.parse(station.send.mock.lastCall![0])).toEqual({ type: 'watch', enabled: false })
  relay.receiveStation(station, frame(2), 100)
  const replacement = peer()
  relay.connectObserver('session-b', browser(), trial(), replacement, 100)
  expect(replacement.send).not.toHaveBeenCalled()
})

it('bounds slow readers to one frame, coalesces to the latest, and rejects old acknowledgements', () => {
  const { relay, station, viewer } = connected()
  relay.receiveStation(station, frame(1), 0)
  for (let sequence = 2; sequence <= 100; sequence++) relay.receiveStation(station, frame(sequence), sequence)
  expect(viewer.send).toHaveBeenCalledTimes(1)
  relay.receiveObserver('session-a', ack(1), 500)
  expect(viewer.send).toHaveBeenCalledTimes(2)
  const next = JSON.parse(viewer.send.mock.calls[1][0]).frame
  expect(next.sequence).toBe(100)
  expect(next.station.radio.readings.cat.ageMs).toBe(400)
  relay.receiveObserver('session-a', ack(1), 501)
  relay.receiveStation(station, frame(101), 502)
  expect(viewer.send).toHaveBeenCalledTimes(2)
  relay.expire(500 + ACK_TIMEOUT_MS)
  expect(viewer.close).toHaveBeenCalledWith(1008, 'observerTooSlow')
})

it('caps observers at four and a failed browser cannot stop other viewers', () => {
  const { relay, station, viewer } = connected()
  const other = [peer(), peer(), peer()]
  other.forEach((p, i) => relay.connectObserver(`session-${i}`, browser(), trial(), p, 0))
  expect(() => relay.connectObserver('fifth', browser(), trial(), peer(), 0)).toThrow('observerLimit')
  other[0].send.mockImplementation(() => { throw new Error('socketGone') })
  relay.receiveStation(station, frame(1), 0)
  expect(viewer.send).toHaveBeenCalledTimes(1)
  expect(other[1].send).toHaveBeenCalledTimes(1)
  expect(other[2].send).toHaveBeenCalledTimes(1)
  expect(other[0].close).toHaveBeenCalledWith(1011, 'observerUnavailable')
})

it('revocation ends active access and replay or account recovery cannot reapprove a device', () => {
  const { relay, station, viewer } = connected()
  relay.receiveStation(station, frame(1), 0)
  const revoked = { ...access(), policyVersion: 2, devices: [{ id: 'browser-a', generation: 2, approved: false }] }
  relay.updateAccess(revoked, 1)
  expect(viewer.close).toHaveBeenCalledWith(1008, 'browserAccessEnded')
  expect(() => relay.updateAccess(access(), 2)).toThrow('obsoleteStationAccess')
  expect(() => relay.connectObserver('recovered', { ...browser(), expiresAt: 240000 }, trial(), peer(), 2)).toThrow()
  relay.receiveStation(station, frame(2), 3)
  expect(viewer.send).toHaveBeenCalledTimes(1)
})

it('expiry is absolute; acknowledgements cannot renew identity or entitlement', () => {
  const { relay, station, viewer } = connected()
  relay.renewObserver('session-a', browser(), { ...trial(), expiresAt: 800 }, 0)
  relay.receiveStation(station, frame(1), 0)
  relay.receiveObserver('session-a', ack(1), 799)
  expect(relay.nextDeadline()).toBe(800)
  relay.receiveStation(station, frame(2), 800)
  expect(viewer.close).toHaveBeenCalledWith(1008, 'browserAccessEnded')
  expect(viewer.send).toHaveBeenCalledTimes(1)
})

it('observer messages have no hardware command or invoke surface', () => {
  for (const payload of [{ type: 'invoke', command: 'set_ptt', args: { on: true } },
    { type: 'ack', epoch: fixtures.spe.epoch, sequence: 1, command: 'amp_command' }, { type: 'stop' }]) {
    const { relay, station, viewer } = connected()
    const before = station.send.mock.calls.length
    relay.receiveObserver('session-a', JSON.stringify(payload), 0)
    expect(viewer.close).toHaveBeenCalledWith(1008, 'invalidAcknowledgement')
    expect(station.send.mock.calls.slice(before).map(([s]) => JSON.parse(s))).toEqual([{ type: 'watch', enabled: false }])
  }
})

it('rejects fixtures, incompatible frames and retired station sockets; disable closes both ends', () => {
  for (const payload of [JSON.stringify(fixtures.spe), JSON.stringify({ ...fixtures.spe, source: 'native', version: 1 }), 'x'.repeat(16385)]) {
    const { relay, station, viewer } = connected()
    relay.receiveStation(station, payload, 0)
    expect(station.close).toHaveBeenCalledWith(1008, 'invalidObservation')
    expect(viewer.send).not.toHaveBeenCalled()
  }
  const { relay, station, viewer } = connected()
  relay.updateAccess({ ...access(), enabled: false, policyVersion: 2, stationGeneration: 2 }, 1)
  expect(station.close).toHaveBeenCalledWith(1008, 'stationAccessEnded')
  expect(viewer.close).toHaveBeenCalledWith(1001, 'stationOffline')
  expect(() => relay.receiveStation(station, frame(1), 2)).toThrow('stationNotConnected')
})

it('supports repeated authenticated app restarts while retaining same-epoch reconnect ordering', () => {
  const relay = new ObservationRelay(access())
  let station = peer()
  const identity = { accountId: 'account-a', stationId: 'station-a', generation: 1, expiresAt: 120000 }
  for (let restart = 0; restart < 32; restart++) {
    station = peer()
    relay.connectStation(identity, station, restart)
    relay.receiveStation(station, JSON.stringify({ ...fixtures.spe, source: 'native', epoch: `process-${restart}`, sequence: 10 }), restart)
    expect(relay.checkpoint().station?.peer).toBe(station)
  }
  const final = peer()
  relay.connectStation(identity, final, 40)
  const viewer = peer()
  relay.connectObserver('resumed', browser(), trial(), viewer, 40)
  relay.receiveStation(final, JSON.stringify({ ...fixtures.spe, source: 'native', epoch: 'process-31', sequence: 9 }), 40)
  expect(viewer.send).not.toHaveBeenCalled()
  relay.receiveStation(final, JSON.stringify({ ...fixtures.spe, source: 'native', epoch: 'process-31', sequence: 11 }), 41)
  expect(viewer.send).toHaveBeenCalledTimes(1)
})

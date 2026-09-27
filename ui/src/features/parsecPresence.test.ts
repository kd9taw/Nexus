// Parsec presence mode's UI words: where the switch appears, the Settings readout, and the
// status-lane item that says why a transmission stopped while the operator was away.
import { describe, expect, it } from 'vitest'
import { parsecStatusText, parsecStopLane, parsecSwitchPlacement } from './parsecPresence'
import { EN } from '../i18n/en'

describe('where the switch appears', () => {
  it('is offered on Windows, explained elsewhere, and never on the Remote page', () => {
    expect(parsecSwitchPlacement(true, false)).toBe('offered')
    expect(parsecSwitchPlacement(false, false)).toBe('unavailable')
    expect(parsecSwitchPlacement(true, true)).toBe('hidden')
    expect(parsecSwitchPlacement(false, true)).toBe('hidden')
  })
})

describe('the Settings readout', () => {
  it('names each thing the watcher can find', () => {
    expect(parsecStatusText('connected')).toBe(EN['settings.transmit.parsecStop.status.connected'])
    expect(parsecStatusText('notConnected')).toBe(
      EN['settings.transmit.parsecStop.status.notConnected'],
    )
    expect(parsecStatusText('unreadable')).toBe(EN['settings.transmit.parsecStop.status.unreadable'])
    expect(parsecStatusText('starting')).toBe(EN['settings.transmit.parsecStop.status.starting'])
  })

  it('reads a status it does not know as "checking", never as a claim', () => {
    expect(parsecStatusText('somethingNew')).toBe(EN['settings.transmit.parsecStop.status.starting'])
  })
})

describe('the status lane after a stop', () => {
  // 2026-09-27 14:32:05 UTC
  const AT = Date.UTC(2026, 8, 27, 14, 32, 5) / 1000

  it('says nothing while nothing was stopped', () => {
    expect(parsecStopLane(null)).toBeNull()
    expect(parsecStopLane(undefined)).toBeNull()
    expect(parsecStopLane({ status: 'connected', stoppedAt: null, stopped: [] })).toBeNull()
    expect(parsecStopLane({ status: 'notConnected', stoppedAt: AT, stopped: [] })).toBeNull()
  })

  it('says a Parsec drop stopped the transmission, when, and what', () => {
    const lane = parsecStopLane({ status: 'notConnected', stoppedAt: AT, stopped: ['ptt'] })
    expect(lane).not.toBeNull()
    expect(lane!.tier).toBe('warning')
    expect(lane!.message).toBe(EN['shell.lane.parsecStop.message'])
    expect(lane!.detail).toContain('14:32:05')
    expect(lane!.detail).toContain(EN['shell.lane.parsecStop.what.ptt'])
    expect(lane!.detail).not.toMatch(/\{\{/)
  })

  it('names every transmission it stopped', () => {
    const lane = parsecStopLane({ status: 'unreadable', stoppedAt: AT, stopped: ['tune', 'rtty', 'psk'] })
    for (const k of ['tune', 'rtty', 'psk'] as const) {
      expect(lane!.detail).toContain(EN[`shell.lane.parsecStop.what.${k}`])
    }
  })
})

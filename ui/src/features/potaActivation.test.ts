import { describe, expect, it } from 'vitest'
import type { Activation, LoggedActivation, LoggedQso } from '../types'
import {
  activationFiles,
  activationFilter,
  activationParks,
  isLegacyUsRef,
  mixedCallsNote,
  parkProgress,
  potaDupe,
  potaKeys,
  spanLabel,
  toUtcMidnight,
} from './potaActivation'

// 2026-10-25 10:40:00Z; the next 00:00Z is 48 000 s later.
const T0 = 1_788_000_000
const MIDNIGHT = T0 + 48_000

const act: Activation = {
  program: 'POTA',
  reference: 'US-1234',
  qsoCount: 1,
  parks: [{ reference: 'US-1234', count: 1, p2p: 0 }],
  utcDay: Math.floor(T0 / 86_400),
}

function qso(over: Partial<LoggedQso> & { whenUnix: number }): LoggedQso {
  return {
    call: 'W1ABC',
    band: '20m',
    mode: 'SSB',
    ota: { myProgram: 'POTA', myRef: 'US-1234' },
    ...over,
  } as LoggedQso
}

const entry = { call: 'W1ABC', band: '20m', mode: 'SSB', whenUnix: T0 + 600 }

describe('potaDupe — POTA’s key: call, band, mode, today, my park (+ theirs)', () => {
  it('same call, band, mode, UTC day and park is a dupe, naming the park and the contact', () => {
    const earlier = qso({ whenUnix: T0 })
    expect(potaDupe([earlier], act, entry)).toEqual({ park: 'US-1234', qso: earlier })
  })

  it('USB is SSB, as the upload check folds it', () => {
    expect(potaDupe([qso({ whenUnix: T0, mode: 'USB' })], act, entry)?.park).toBe('US-1234')
  })

  it('the next UTC day is a new activation day: not a dupe', () => {
    const late = qso({ whenUnix: MIDNIGHT - 30 })
    expect(potaDupe([late], act, { ...entry, whenUnix: MIDNIGHT - 1 })?.park).toBe('US-1234')
    expect(potaDupe([late], act, { ...entry, whenUnix: MIDNIGHT + 30 })).toBeNull()
  })

  it('a back-dated entry is checked against the contacts of the day typed, not today', () => {
    const yesterday = qso({ whenUnix: T0 - 86_400 })
    expect(potaDupe([yesterday], act, entry)).toBeNull()
    expect(potaDupe([yesterday], act, { ...entry, whenUnix: T0 - 86_400 + 60 })).toEqual({ park: 'US-1234', qso: yesterday })
    expect(potaDupe([qso({ whenUnix: T0 })], act, { ...entry, whenUnix: T0 - 86_400 + 60 })).toBeNull()
  })

  it('another park, band, mode, call or hunted park is not a dupe', () => {
    expect(potaDupe([qso({ whenUnix: T0, ota: { myRef: 'US-5678' } })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, band: '40m' })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, mode: 'CW' })], act, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0, call: 'W1ABC/P' })], act, entry)).toBeNull()
    const p2p = { myRef: 'US-1234', theirProgram: 'POTA', theirRef: 'US-0099' }
    expect(potaDupe([qso({ whenUnix: T0, ota: p2p })], act, entry)).toBeNull()
    expect(
      potaDupe([qso({ whenUnix: T0, ota: { ...p2p, theirRef: 'us-0099' } })], act, { ...entry, theirProgram: 'POTA', theirRef: 'US-0099' })
        ?.park,
    ).toBe('US-1234')
  })

  it('a summit or another program’s reference is no part of POTA’s key', () => {
    const summit = qso({ whenUnix: T0, ota: { myRef: 'US-1234', theirProgram: 'SOTA', theirRef: 'W9/WI-001' } })
    expect(potaDupe([summit], act, entry)?.qso).toBe(summit)
  })

  it('a hunter’s two-fer is a dupe only once each of their parks is already in today', () => {
    const pair = { ...entry, theirProgram: 'POTA', theirRef: 'US-0001, US-0002' }
    const first = qso({ whenUnix: T0, ota: { myRef: 'US-1234', theirProgram: 'POTA', theirRef: 'US-0001' } })
    expect(potaDupe([first], act, pair)).toBeNull()
    const second = qso({ whenUnix: T0 + 60, ota: { myRef: 'US-1234', theirProgram: 'POTA', theirRef: 'US-0002' } })
    expect(potaDupe([first, second], act, pair)).toEqual({ park: 'US-1234', qso: second })
    const both = qso({ whenUnix: T0, ota: { myRef: 'US-1234', theirProgram: 'POTA', theirRef: 'US-0002,US-0001' } })
    expect(potaDupe([both], act, { ...entry, theirProgram: 'POTA', theirRef: 'US-0001' })?.qso).toBe(both)
  })

  it('a two-fer contact is a dupe at whichever activation park it shares', () => {
    const twoFer: Activation = { ...act, reference: 'US-1234,US-5678', parks: undefined }
    const earlier = qso({ whenUnix: T0, ota: { myRef: 'US-1234,US-5678' } })
    expect(potaDupe([earlier], twoFer, entry)?.park).toBe('US-1234')
  })

  it('not activating, or paused since launch: no POTA dupe at all', () => {
    expect(potaDupe([qso({ whenUnix: T0 })], { ...act, restored: true }, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0 })], null, entry)).toBeNull()
    expect(potaDupe([qso({ whenUnix: T0 })], { ...act, reference: null, parks: [] }, entry)).toBeNull()
  })
})

describe('the rest of the activator’s arithmetic', () => {
  it('potaKeys folds the same way the counter does: one key per POTA park of theirs', () => {
    expect(potaKeys(' w1abc ', '20M', 'lsb', 'pota', ' us-1 ; us-2 ; US-1')).toEqual(potaKeys('W1ABC', '20m', 'SSB', 'POTA', 'US-1,US-2'))
    expect(potaKeys('W1ABC', '20m', 'SSB', 'POTA', 'US-1,US-2')).toHaveLength(2)
    expect(potaKeys('W1ABC', '20m', 'SSB', 'SOTA', 'W9/WI-001')).toEqual(potaKeys('W1ABC', '20m', 'SSB', null, null))
    expect(potaKeys('W1ABC/P', '20m', 'SSB', null, null)).not.toEqual(potaKeys('W1ABC', '20m', 'SSB', null, null))
  })

  it('parkProgress: 7 of 10 is 3 to go; 10 is valid', () => {
    expect(parkProgress(7)).toEqual({ count: 7, toGo: 3, valid: false })
    expect(parkProgress(10)).toEqual({ count: 10, toGo: 0, valid: true })
    expect(parkProgress(12)).toEqual({ count: 12, toGo: 0, valid: true })
  })

  it('activationParks reads the two-fer list when the counts have not arrived', () => {
    expect(activationParks({ ...act, reference: 'US-1234,US-5678', parks: undefined })).toEqual(['US-1234', 'US-5678'])
    expect(activationParks(null)).toEqual([])
  })

  it('isLegacyUsRef: K- is the pre-2024 US prefix', () => {
    expect(isLegacyUsRef('k-1234')).toBe(true)
    expect(isLegacyUsRef('US-1234')).toBe(false)
    expect(isLegacyUsRef('KA-1234')).toBe(false)
  })
})

describe('this activation, as the Logbook and the export see it', () => {
  // A two-fer started at 23:30Z, running across 0000Z.
  const DAY = 86_400
  const start = MIDNIGHT - 1_800
  const twoFer: Activation = {
    program: 'POTA',
    reference: 'US-1234,US-5678',
    qsoCount: 3,
    parks: [
      { reference: 'US-1234', count: 3, p2p: 0 },
      { reference: 'US-5678', count: 3, p2p: 0 },
    ],
    startedAt: start,
    utcDay: Math.floor(MIDNIGHT / DAY),
  }

  it('is the first park, whole, from 0000Z of the day it started — both days of a run across 0000Z', () => {
    expect(activationFilter(twoFer)).toEqual({ myRef: 'US-1234', fromUnix: MIDNIGHT - DAY })
    expect(activationFilter(null)).toBeNull()
    expect(activationFilter({ ...twoFer, reference: null, parks: [] })).toBeNull()
    // No start time (a station older than the record): no filter rather than every contact ever.
    expect(activationFilter({ ...twoFer, startedAt: 0 })).toBeNull()
  })

  it('its files: each park on each day since it started, oldest day first, parks in its order', () => {
    const f = (reference: string, dayStartUnix: number, qsos = 5, callsign = 'K1ABC'): LoggedActivation => ({
      program: 'POTA',
      reference,
      dayStartUnix,
      date: new Date(dayStartUnix * 1000).toISOString().slice(0, 10),
      callsign,
      qsos,
    })
    const today = MIDNIGHT
    const yesterday = MIDNIGHT - DAY
    const list = [
      f('US-5678', today),
      f('US-1234', today),
      f('US-9999', today), // another park: not this activation's
      f('US-1234', yesterday - DAY), // the day before it started: not this activation's
      f('US-5678', yesterday),
      f('US-1234', yesterday),
    ]
    expect(activationFiles(list, twoFer).map((a) => `${a.reference}@${a.dayStartUnix}`)).toEqual([
      `US-1234@${yesterday}`,
      `US-5678@${yesterday}`,
      `US-1234@${today}`,
      `US-5678@${today}`,
    ])
    expect(activationFiles(list, null)).toEqual([])
  })

  it('reads spans in hours and minutes, and the time to 0000Z', () => {
    expect(spanLabel(0)).toBe('0 m')
    expect(spanLabel(59 * 60 + 59)).toBe('59 m')
    expect(spanLabel(3_900)).toBe('1 h 05 m')
    expect(toUtcMidnight(T0)).toBe(48_000)
    expect(toUtcMidnight(MIDNIGHT)).toBe(DAY)
  })
})

describe('mixedCallsNote', () => {
  it('names each call with its count when a park’s day holds more than one, and nothing for one', () => {
    expect(
      mixedCallsNote({
        reference: 'US-1234',
        count: 10,
        p2p: 0,
        calls: [
          { callsign: 'KD9TAW', count: 6 },
          { callsign: 'KD9TAW/M', count: 4 },
          { callsign: null, count: 1 },
        ],
      }),
    ).toBe('US-1234: contacts under 3 calls (KD9TAW 6, KD9TAW/M 4, no call 1) — POTA counts each call separately')
    expect(mixedCallsNote({ reference: 'US-1234', count: 6, p2p: 0, calls: [{ callsign: 'KD9TAW', count: 6 }] })).toBeNull()
    expect(mixedCallsNote({ reference: 'US-1234', count: 6, p2p: 0 })).toBeNull()
  })
})

// The JS8 cockpit's INVARIANT vocabulary: the tables transcribed from JS8Call as protocol
// facts (NOTICE credits them). Pinned here so a typo in a command text — which is what goes on
// the air in a directed frame — is caught by a test, not by a station that never answers.
import { describe, expect, it } from 'vitest'
import {
  JS8_COMMANDS, JS8_CQS, JS8_SPEEDS, JS8_SPEED_LIST, JS8_QUICK_QUERIES,
  ageLabel, bandActivityByOffset, countBits, dtLabel, estimateFrames, fmtSnr, js8ListedStations, js8UnreadFirst,
  js8ShownOffsetRows, js8UnreadFrom, utcClock,
} from './js8Vocab'
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry } from './types'

describe('the 32-command table (varicode.cpp:46-84, leading space included)', () => {
  it('has 32 unique ids 0..31 in order and the exact wire texts', () => {
    expect(JS8_COMMANDS.map((c) => c.id)).toEqual([...Array(32).keys()])
    expect(JS8_COMMANDS.map((c) => c.text)).toEqual([
      ' SNR?', ' DIT DIT', ' NACK', ' HEARING?', ' GRID?', '>', ' STATUS?', ' STATUS', ' HEARING',
      ' MSG', ' MSG TO:', ' QUERY', ' QUERY MSGS', ' QUERY CALL', ' ACK', ' GRID', ' INFO?', ' INFO',
      ' FB', ' HW CPY?', ' SK', ' RR', ' QSL?', ' QSL', ' CMD', ' SNR', ' NO', ' YES', ' 73',
      ' HEARTBEAT SNR', ' AGN?', ' ',
    ])
  })
  it('the quick queries are the five one-click autoreply asks, by id', () => {
    expect(JS8_QUICK_QUERIES.map((q) => [q.id, q.label])).toEqual([
      [0, 'SNR?'], [4, 'GRID?'], [16, 'INFO?'], [3, 'HEARING?'], [12, 'QUERY MSGS'],
    ])
  })
})

describe('CQ variants and speeds', () => {
  it('eight CQ variants in upstream index order (varicode.cpp:283-292)', () => {
    expect(JS8_CQS).toEqual(['CQ CQ CQ', 'CQ DX', 'CQ QRP', 'CQ CONTEST', 'CQ FIELD', 'CQ FD', 'CQ CQ', 'CQ'])
  })
  it('speeds carry Speed::ALL index, ALL.TXT letter, period and the §97.119 frame cap', () => {
    expect(JS8_SPEED_LIST.map((s) => [s.key, s.idx, s.letter, s.periodS, s.maxFrames])).toEqual([
      ['slow', 0, 'E', 30, 19], ['normal', 1, 'A', 15, 39], ['fast', 2, 'B', 10, 59], ['turbo', 3, 'C', 6, 99],
    ])
    expect(JS8_SPEEDS.normal.label).toBe('Normal')
  })
})

describe('formatters are invariant (no locale, no comma)', () => {
  it('fmtSnr signs and pads like JS8Call (+07 / -12 / +00)', () => {
    expect(fmtSnr(7)).toBe('+07')
    expect(fmtSnr(-12)).toBe('-12')
    expect(fmtSnr(0)).toBe('+00')
  })
  it('ageLabel picks s/m/h', () => {
    expect(ageLabel(4_000)).toBe('4s')
    expect(ageLabel(150_000)).toBe('2m')
    expect(ageLabel(7_200_000)).toBe('2h')
  })
  it('utcClock is HH:MM:SS UTC', () => {
    expect(utcClock(Date.UTC(2026, 8, 5, 13, 4, 9))).toBe('13:04:09')
  })
  it('countBits counts the rx-speed mask', () => {
    expect(countBits(15)).toBe(4)
    expect(countBits(2)).toBe(1)
    expect(countBits(0)).toBe(0)
  })
})

describe('estimateFrames — the composer’s pre-send estimate (an approximation of compose::frames)', () => {
  it('empty text with no command is nothing to send', () => {
    expect(estimateFrames('', null, '', 'KD9TAW', 'normal')).toBe(0)
  })
  it('a directed command with no text is one frame', () => {
    expect(estimateFrames('W1AW', 0, '', 'KD9TAW', 'normal')).toBe(1)
  })
  it('plain text carries the MYCALL: prefix and packs ~10 chars/frame at Normal, ~13 elsewhere', () => {
    // "KD9TAW: HELLO" = 13 chars → 2 frames at Normal (10/frame), 1 frame at Fast (13/frame)
    expect(estimateFrames('', null, 'HELLO', 'KD9TAW', 'normal')).toBe(2)
    expect(estimateFrames('', null, 'HELLO', 'KD9TAW', 'fast')).toBe(1)
  })
  it('a directed message is one header frame plus the text frames', () => {
    expect(estimateFrames('W1AW', null, 'HELLO THERE OM', 'KD9TAW', 'normal')).toBe(1 + 2)
  })
})

describe('bandActivityByOffset — JS8Call’s offset-bucketed band-activity table', () => {
  const row = (atMs: number, freqHz: number, text: string, extra: Partial<Js8ActivityRow> = {}): Js8ActivityRow => ({
    atMs, speed: 'normal', freqHz, snrDb: -8, dtS: 0.1, from: 'W0IND', text,
    directedToMe: false, mine: false, complete: true, lowConf: false, ...extra,
  })

  it('keeps the NEWEST decode per offset, ordered by offset', () => {
    const out = bandActivityByOffset([
      row(3_000, 1500, 'THIRD'),
      row(1_000, 700, 'FIRST'),
      row(2_000, 1500, 'SECOND'),
    ])
    expect(out.map((r) => [r.offsetHz, r.text])).toEqual([
      [700, 'FIRST'],
      [1500, 'THIRD'],
    ])
  })

  it('merges a decode within the ±10 Hz tolerance and re-keys the bucket to the new offset', () => {
    const out = bandActivityByOffset([row(1_000, 1500, 'OLD'), row(2_000, 1508, 'NEW')])
    expect(out.length).toBe(1)
    expect(out[0].offsetHz).toBe(1508)
    expect(out[0].text).toBe('NEW')
  })

  it('keeps two buckets when the offsets are further apart than the tolerance', () => {
    const out = bandActivityByOffset([row(1_000, 1500, 'A'), row(2_000, 1511, 'B')])
    expect(out.map((r) => r.offsetHz)).toEqual([1500, 1511])
  })

  it('carries the DT and the row semantics the pane paints with', () => {
    const out = bandActivityByOffset([row(1_000, 900, 'HI', { dtS: -0.42, directedToMe: true, lowConf: true })])
    expect(out[0].dtS).toBeCloseTo(-0.42)
    expect(out[0].directedToMe).toBe(true)
    expect(out[0].lowConf).toBe(true)
  })

  it('is empty for an empty feed', () => {
    expect(bandActivityByOffset([])).toEqual([])
  })

  // JS8Call joins a decode to a bucket within the rxThreshold of the NEW decode's submode
  // (mainwindow.cpp:3971): 10 Hz at Slow and Normal, the default (JS8Submode.cpp:62), 16 at Fast
  // and 32 at Turbo (:122-123).
  it('joins within the new decode\'s own speed tolerance: 10 Hz Slow/Normal, 16 Fast, 32 Turbo', () => {
    for (const [speed, tol] of [['slow', 10], ['normal', 10], ['fast', 16], ['turbo', 32]] as const) {
      const join = bandActivityByOffset([row(1_000, 1500, 'A', { speed }), row(2_000, 1500 + tol, 'B', { speed })])
      expect(join.map((r) => r.offsetHz), `${speed}: ${tol} Hz joins`).toEqual([1500 + tol])
      const apart = bandActivityByOffset([row(1_000, 1500, 'A', { speed }), row(2_000, 1501 + tol, 'B', { speed })])
      expect(apart.map((r) => r.offsetHz), `${speed}: ${tol + 1} Hz stays apart`).toEqual([1500, 1501 + tol])
    }
  })

  it('takes the tolerance from the decode being filed, not from the bucket', () => {
    const turboJoins = bandActivityByOffset([row(1_000, 1500, 'A'), row(2_000, 1530, 'B', { speed: 'turbo' })])
    expect(turboJoins.map((r) => r.offsetHz), 'a Turbo decode 30 Hz from a Normal bucket joins it').toEqual([1530])
    const normalApart = bandActivityByOffset([row(1_000, 1500, 'A', { speed: 'turbo' }), row(2_000, 1530, 'B')])
    expect(normalApart.map((r) => r.offsetHz), 'a Normal decode 30 Hz from a Turbo bucket does not').toEqual([1500, 1530])
  })

  // JS8Call files a decode at `frequencyOffset()`, the int the decoder's float frequency is handed
  // in as (decodedtext.cpp:248 → decodedtext.h:80): truncated, never rounded.
  it('files a decode at its offset truncated to whole hertz', () => {
    expect(bandActivityByOffset([row(1_000, 1500.9, 'A')]).map((r) => r.offsetHz), '1500.9 Hz files at 1500').toEqual([1500])
    // 1510.6 Hz files at 1510, inside Normal's 10 Hz of 1500; rounded it would be 1511, outside.
    const out = bandActivityByOffset([row(1_000, 1500, 'A'), row(2_000, 1510.6, 'B')])
    expect(out.map((r) => [r.offsetHz, r.text]), 'a truncated offset joins its neighbour').toEqual([[1510, 'B']])
  })

  // An offset already filed takes the decode as it is (mainwindow.cpp:3969); only a new offset
  // looks for a neighbour to take over (:3970-3981).
  it('files a decode at its own offset when that offset is already filed, whatever else is in range', () => {
    // A Turbo bucket at 1500 and a Normal one at 1520 stand apart (20 Hz is over Normal's 10). A
    // Turbo decode at 1520 lands on 1520, though 1500 is within Turbo's 32 Hz.
    const out = bandActivityByOffset([
      row(1_000, 1500, 'A', { speed: 'turbo' }),
      row(2_000, 1520, 'B'),
      row(3_000, 1520, 'C', { speed: 'turbo' }),
    ])
    expect(out.map((r) => [r.offsetHz, r.text])).toEqual([[1500, 'A'], [1520, 'C']])
  })

  // A new offset takes over the first filed offset `generateOffsets` meets counting up from
  // offset − range (mainwindow.cpp:3972-3979, :3730-3739): the lowest in range, not the oldest.
  it('a new offset takes over the lowest filed offset in range, not the one filed first', () => {
    const out = bandActivityByOffset([row(1_000, 1520, 'A'), row(2_000, 1500, 'B'), row(3_000, 1510, 'C')])
    expect(out.map((r) => [r.offsetHz, r.text])).toEqual([[1510, 'C'], [1520, 'A']])
  })
})

describe('dtLabel — JS8Call’s Time Delta face (whole ms, signed)', () => {
  it('renders whole milliseconds', () => {
    expect(dtLabel(0.12)).toBe('120 ms')
    expect(dtLabel(0)).toBe('0 ms')
  })
  it('keeps the sign — an early station reads negative', () => {
    expect(dtLabel(-0.4)).toBe('-400 ms')
  })
})

// JS8Call's callsign aging for its call-activity list (mainwindow.cpp:10209-10233): off by default
// (CallsignAging 0, Configuration.cpp:1853); set, a call last heard that many whole minutes ago or
// more is left off, unless it is selected or has an unread message to me (:10220-10231).
describe('js8ListedStations — the Stations list under JS8Call’s callsign aging', () => {
  const NOW = 1_800_000_000_000
  const heard = (call: string, agoMs: number): Js8Heard => ({
    call, grid: null, snrDb: -10, freqHz: 1500, speed: 'normal', lastMs: NOW - agoMs,
    lastHb: false, lastCq: false, storedMsgs: 0,
  })
  const msg = (from: string, to: string, state: Js8InboxEntry['state']): Js8InboxEntry => ({
    id: 1, from, to, text: 'HELLO', path: [], state, atMs: NOW - 3_600_000, freqHz: 1500, snrDb: -10,
  })
  const stations = [heard('K1ABC', 10 * 60_000), heard('N0XYZ', 10 * 60_000 - 1_000), heard('K2DEF', 60_000)]
  const listed = (o: Partial<Parameters<typeof js8ListedStations>[2]>, inbox: Js8InboxEntry[] = []) =>
    js8ListedStations(stations, inbox, { agingMin: 10, nowMs: NOW, selectedCall: '', myCall: 'KD9TAW', ...o }).map((h) => h.call)

  it('off (0, the default), lists every station in order', () => {
    expect(listed({ agingMin: 0 })).toEqual(['K1ABC', 'N0XYZ', 'K2DEF'])
  })
  it('leaves off a call heard the aging’s whole minutes ago or more, and keeps 9:59', () => {
    expect(listed({})).toEqual(['N0XYZ', 'K2DEF'])
  })
  it('keeps the selected call, however old', () => {
    expect(listed({ selectedCall: 'K1ABC' }), 'selected').toEqual(['K1ABC', 'N0XYZ', 'K2DEF'])
  })
  it('keeps a call with an unread message to me, or to my base call, and no other', () => {
    expect(listed({}, [msg('K1ABC', 'KD9TAW', 'unread')]), 'unread, to me').toEqual(['K1ABC', 'N0XYZ', 'K2DEF'])
    expect(listed({ myCall: 'KD9TAW/P' }, [msg('K1ABC', 'KD9TAW', 'unread')]), 'unread, to my base call').toEqual(['K1ABC', 'N0XYZ', 'K2DEF'])
    expect(listed({}, [msg('K1ABC', 'KD9TAW', 'read')]), 'read').toEqual(['N0XYZ', 'K2DEF'])
    expect(listed({}, [msg('K1ABC', 'W1AW', 'unread')]), 'unread, for another station').toEqual(['N0XYZ', 'K2DEF'])
  })
})

describe('js8UnreadFrom / js8UnreadFirst — JS8Call’s flag and "pin messages to the top"', () => {
  const msg = (from: string, to: string, state: Js8InboxEntry['state']): Js8InboxEntry => ({
    id: 1, from, to, text: 'HELLO', path: [], state, atMs: 0, freqHz: 1500, snrDb: -10,
  })
  it('counts an unread message to my call or my base call, as refreshInboxCounts does', () => {
    const inbox = [
      msg('K1ABC', 'KD9TAW', 'unread'),
      msg('K2DEF', 'KD9TAW', 'read'),
      msg('N0XYZ', '@FUN', 'unread'),
      msg('W1AW', 'K9OTHER', 'unread'),
      msg('W2AW', 'KD9TAW', 'store'),
    ]
    expect([...js8UnreadFrom(inbox, 'KD9TAW')], 'to me').toEqual(['K1ABC'])
    expect([...js8UnreadFrom(inbox, 'kd9taw/p ')], 'to my base call').toEqual(['K1ABC'])
    expect([...js8UnreadFrom(inbox, '')], 'no call of mine, nothing is to me').toEqual([])
  })
  it('lifts those stations to the top and keeps every other order as it was', () => {
    // Not in call order, so a sort by call cannot pass for the stable partition.
    const rows = ['D1D', 'B1B', 'C1C', 'A1A'].map((call) => ({ call }))
    expect(js8UnreadFirst(rows, new Set(['A1A', 'C1C'])).map((r) => r.call)).toEqual(['C1C', 'A1A', 'D1D', 'B1B'])
    expect(js8UnreadFirst(rows, new Set()).map((r) => r.call), 'none unread').toEqual(['D1D', 'B1B', 'C1C', 'A1A'])
  })
})

describe('js8ShownOffsetRows — Band activity under JS8Call’s ActivityAging', () => {
  const NOW = 1_800_000_000_000
  const rows = [
    { offsetHz: 700, atMs: NOW - 2 * 60_000 },
    { offsetHz: 1200, atMs: NOW - (2 * 60_000 - 1_000) },
    { offsetHz: 1500, atMs: NOW - 30 * 60_000 },
  ].map((r) => ({ ...r, snrDb: -10, dtS: 0, speed: 'normal' as const, text: 'X', directedToMe: false, mine: false, lowConf: false }))
  const shown = (o: Partial<Parameters<typeof js8ShownOffsetRows>[1]>) =>
    js8ShownOffsetRows(rows, { agingMin: 2, nowMs: NOW, selectedHz: null, ...o }).map((r) => r.offsetHz)
  it('leaves off a row heard the aging’s whole minutes ago or more, and keeps 1:59', () => {
    expect(shown({})).toEqual([1200])
  })
  it('keeps the selected offset’s row, however old', () => {
    expect(shown({ selectedHz: 1500 })).toEqual([1200, 1500])
  })
  it('0 keeps every row, in order', () => {
    expect(shown({ agingMin: 0 })).toEqual([700, 1200, 1500])
  })
})

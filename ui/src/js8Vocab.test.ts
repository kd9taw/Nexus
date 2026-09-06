// The JS8 cockpit's INVARIANT vocabulary: the tables transcribed from JS8Call as protocol
// facts (NOTICE credits them). Pinned here so a typo in a command text — which is what goes on
// the air in a directed frame — is caught by a test, not by a station that never answers.
import { describe, expect, it } from 'vitest'
import {
  JS8_COMMANDS, JS8_CQS, JS8_SPEEDS, JS8_SPEED_LIST, JS8_QUICK_QUERIES,
  ageLabel, countBits, estimateFrames, fmtSnr, utcClock,
} from './js8Vocab'

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

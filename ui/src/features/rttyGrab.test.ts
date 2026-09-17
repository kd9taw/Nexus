import { describe, expect, it } from 'vitest'
import { classifyGrab, grabAt, tokenAt } from './rttyGrab'

// Only example callsigns here (W1AW, K1ABC, W9XYZ, N0CALL and compounds of them) — this is
// the table a public repo publishes.

describe('tokenAt — the whitespace token under a caret offset', () => {
  const text = 'CQ TEST DE W1AW/P W1AW/P\r\nK'

  it('finds the token the offset falls inside, keeping the slash native selection breaks at', () => {
    expect(tokenAt(text, text.indexOf('W1AW') + 2)).toBe('W1AW/P')
    expect(tokenAt(text, text.indexOf('/P') + 1)).toBe('W1AW/P')
  })

  it('takes the token on the LEFT when the caret sits just past its last character', () => {
    // caretPositionFromPoint answers an insertion point: a click on the right half of the last
    // character lands AFTER it, on the space. That is still a click on the word.
    const end = text.indexOf('W1AW/P') + 'W1AW/P'.length
    expect(text[end]).toBe(' ')
    expect(tokenAt(text, end)).toBe('W1AW/P')
  })

  it('takes the token on the RIGHT when the caret sits at its first character', () => {
    expect(tokenAt(text, text.indexOf('DE'))).toBe('DE')
  })

  it('treats CR/LF as whitespace and answers nothing between two spaces', () => {
    expect(tokenAt(text, text.length)).toBe('K')
    expect(tokenAt(text, text.indexOf('\r') + 1)).toBe('')
    expect(tokenAt('CQ  W1AW', 3)).toBe('')
  })

  it('clamps an offset outside the string instead of reading past it', () => {
    expect(tokenAt('W1AW', 99)).toBe('W1AW')
    expect(tokenAt('W1AW', -3)).toBe('W1AW')
    expect(tokenAt('', 0)).toBe('')
  })
})

describe('classifyGrab — calls', () => {
  it.each([
    ['W1AW', 'W1AW'],
    ['K1ABC', 'K1ABC'],
    ['N0CALL', 'N0CALL'],
    ['w9xyz', 'W9XYZ'],
    // Compound calls, both halves of the slash.
    ['W1AW/P', 'W1AW/P'],
    ['VE3/K1ABC', 'VE3/K1ABC'],
    ['W9XYZ/7', 'W9XYZ/7'],
    ['KH6/W9XYZ/P', 'KH6/W9XYZ/P'],
    // Edge punctuation is stripped exactly as the sequencer's tokenizer strips it.
    ['W1AW?', 'W1AW'],
    ['(K1ABC),', 'K1ABC'],
  ])('%s → call %s', (token, call) => {
    expect(classifyGrab(token)).toEqual({ kind: 'call', value: call })
  })

  it('fills a FIGS-damaged call AS COPIED — the operator corrects it, the grab never refuses it', () => {
    // W1AW with the lost figures shift printing its 1 on the letters plane (Q). There is no
    // digit left in it at all, so a digit-requiring rule refuses the one call the operator most
    // needed help with.
    expect(classifyGrab('WQAW')).toEqual({ kind: 'call', value: 'WQAW' })
    expect(classifyGrab('KQABC/P')).toEqual({ kind: 'call', value: 'KQABC/P' })
  })
})

describe('classifyGrab — what is never a call (no-op, no toast)', () => {
  it.each([
    // The brief's list, every entry.
    '599', '5NN', 'CQ', 'DE', 'TU', 'TEST', 'QRZ', 'AGN', 'K', 'KN', 'SK', '73', 'RR73',
    // The sequencer's own keywords beyond it (seq.rs KEYWORDS, mirrored).
    'QSL', 'PSE', 'NAME', 'QTH', 'UR', 'RST', 'BK', 'NR',
    // 599 printed on the letters plane is still a report (seq.rs normalize_rst).
    'TOO', 'TNN',
    // Maidenhead squares, 4 and 6 characters. (RR73 above is one too.)
    'EN61', 'FN31PR', 'fn31',
    // Numbers, junk, bare punctuation, broken slashes, and words run into figures: a call
    // ends in a LETTER, which is the one rule that refuses NR001 and TU73.
    '14', '0400', '100W', '20M', 'NR001', 'TU73', '?', '', '/', 'W1AW/', '/W1AW', 'W1AW//P',
  ])('%j → nothing', (token) => {
    expect(classifyGrab(token)).toBeNull()
  })
})

describe('classifyGrab — a contest exchange (built now, wired when the contest strip lands)', () => {
  const QTH = new Set(['IL', 'WI', 'ON'])
  const contest = { contest: { zone: true, qth: (code: string) => QTH.has(code) } }

  it('reads a CQ zone 1–40, with or without its leading zero', () => {
    expect(classifyGrab('5', contest)).toEqual({ kind: 'zone', value: '5' })
    expect(classifyGrab('05', contest)).toEqual({ kind: 'zone', value: '5' })
    expect(classifyGrab('40', contest)).toEqual({ kind: 'zone', value: '40' })
    for (const bad of ['0', '00', '41', '599']) expect(classifyGrab(bad, contest)).toBeNull()
  })

  it('reads a zone that arrived on the letters plane (unshift-on-space garble)', () => {
    // "599 05 05" from a sender without USOS prints "599 PT PT" here: the space unshifts.
    expect(classifyGrab('PT', contest)).toEqual({ kind: 'zone', value: '5' })
    expect(classifyGrab('QR', contest)).toEqual({ kind: 'zone', value: '14' })
    expect(classifyGrab('RP', contest)).toEqual({ kind: 'zone', value: '40' })
    expect(classifyGrab('TP', contest), '50 is no CQ zone').toBeNull()
  })

  it('reads a QTH code from the active domain — before the letters-plane zone it could also be', () => {
    expect(classifyGrab('IL', contest)).toEqual({ kind: 'qth', value: 'IL' })
    expect(classifyGrab('on', contest)).toEqual({ kind: 'qth', value: 'ON' })
    // WI is also zone 28 on the letters plane; the domain answers first.
    expect(classifyGrab('WI', contest)).toEqual({ kind: 'qth', value: 'WI' })
  })

  it('still grabs calls, and still refuses the report', () => {
    expect(classifyGrab('W1AW', contest)).toEqual({ kind: 'call', value: 'W1AW' })
    expect(classifyGrab('599', contest)).toBeNull()
    expect(classifyGrab('5NN', contest)).toBeNull()
  })

  it('POSITIVE CONTROL: outside a contest the same tokens are not exchange values', () => {
    // Without this, a classifier that always answered zone/QTH would pass the block above.
    for (const token of ['5', '05', 'PT', 'IL', 'ON']) expect(classifyGrab(token)).toBeNull()
    // …and WI outside a contest is neither a state nor a zone nor a two-letter call.
    expect(classifyGrab('WI')).toBeNull()
  })
})

describe('grabAt — the offset and the classifier together', () => {
  it('grabs the compound call under the caret from a live-looking transcript', () => {
    const text = 'RYRYRY CQ TEST DE VE3/K1ABC VE3/K1ABC CQ\r\n'
    expect(grabAt(text, text.indexOf('K1ABC') + 3)).toEqual({ kind: 'call', value: 'VE3/K1ABC' })
    expect(grabAt(text, text.indexOf('TEST') + 1)).toBeNull()
  })
})

import { describe, expect, it } from 'vitest'
import {
  expandRttyMacro,
  frameForAir,
  isStopLikeLabel,
  resolveRttySet,
  rttySetId,
  unknownRttyTokens,
  withRttyEntry,
  withRttySetReset,
} from './rttyMacros'
import type { RttyMacroProfile } from '../types'

// Only example callsigns here (W1AW, K1ABC, W9XYZ, N0CALL).

/** A translator that shows WHICH key a built-in caption came from, so a test can tell a
 *  translated caption from a saved one. */
const tr = (key: string) => `«${key}»`

describe('the built-in sets', () => {
  it('Everyday is the set the cockpit shipped with, plus four empty keys', () => {
    expect(resolveRttySet([], 'everyday', tr)).toEqual([
      { key: 'F1', label: 'CQ', text: 'CQ CQ CQ DE {MYCALL} {MYCALL} K', custom: false },
      { key: 'F2', label: '«rtty.macro.answer.label»', text: '{CALL} DE {MYCALL} {MYCALL} K', custom: false },
      { key: 'F3', label: '«rtty.macro.exchange.label»', text: '{CALL} DE {MYCALL} UR 599 599 K', custom: false },
      { key: 'F4', label: '73', text: '{CALL} DE {MYCALL} TU 73 SK', custom: false },
      { key: 'F5', label: '', text: '', custom: false },
      { key: 'F6', label: '', text: '', custom: false },
      { key: 'F7', label: '', text: '', custom: false },
      { key: 'F8', label: '', text: '', custom: false },
    ])
  })

  it('Contest is the run/S&P set, one message per key', () => {
    expect(resolveRttySet([], 'contest', tr).map((s) => [s.key, s.label, s.text])).toEqual([
      ['F1', 'CQ', 'CQ TEST {MYCALL} {MYCALL} CQ'],
      ['F2', '«rtty.macro.exch.label»', '{CALL} 599 {EXCH} {EXCH}'],
      ['F3', 'TU', 'TU {MYCALL} CQ'],
      ['F4', '«rtty.macro.myCall.label»', '{MYCALL} {MYCALL}'],
      ['F5', '«rtty.macro.hisCall.label»', '{CALL}'],
      ['F6', '«rtty.macro.spExch.label»', 'TU 599 {EXCH} {EXCH}'],
      ['F7', 'AGN', 'AGN? AGN?'],
      ['F8', 'B4', '{CALL} QSO B4 TU {MYCALL}'],
    ])
  })

  it('names Contest only by its id — anything else is Everyday', () => {
    expect(rttySetId('contest')).toBe('contest')
    for (const other of ['', 'everyday', 'Contest', undefined, null]) expect(rttySetId(other)).toBe('everyday')
  })
})

describe('saved entries fold over the built-ins', () => {
  const saved: RttyMacroProfile[] = [
    { name: 'contest', macros: [{ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05' }] },
    { name: 'everyday', macros: [{ key: 'F5', label: 'Rig', text: 'RIG HERE IS 100W' }] },
  ]

  it('replaces exactly the saved keys, in their own words, and translates the rest', () => {
    const contest = resolveRttySet(saved, 'contest', tr)
    expect(contest[1]).toEqual({ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05', custom: true })
    expect(contest[0]).toEqual({ key: 'F1', label: 'CQ', text: 'CQ TEST {MYCALL} {MYCALL} CQ', custom: false })
    const everyday = resolveRttySet(saved, 'everyday', tr)
    expect(everyday[4]).toEqual({ key: 'F5', label: 'Rig', text: 'RIG HERE IS 100W', custom: true })
    expect(everyday[1].label, 'a built-in caption is still resolved through the translator').toBe(
      '«rtty.macro.answer.label»',
    )
  })

  it('an entry emptied on purpose is an empty key, not the built-in', () => {
    const cleared = [{ name: 'everyday', macros: [{ key: 'F1', label: '', text: '' }] }]
    expect(resolveRttySet(cleared, 'everyday', tr)[0]).toEqual({ key: 'F1', label: '', text: '', custom: true })
  })

  it('writes one key, removes one key and resets one set — touching nothing else', () => {
    const one = withRttyEntry(saved, 'contest', 'F1', { label: 'CQ', text: 'CQ W1AW TEST' })
    expect(one[0].macros.map((m) => m.key), 'kept in F-key order').toEqual(['F1', 'F2'])
    expect(one[1], 'the other set is untouched').toBe(saved[1])
    const replaced = withRttyEntry(one, 'contest', 'F1', { label: 'CQ', text: 'CQ TEST W1AW' })
    expect(replaced[0].macros.filter((m) => m.key === 'F1')).toEqual([{ key: 'F1', label: 'CQ', text: 'CQ TEST W1AW' }])
    const removed = withRttyEntry(replaced, 'contest', 'F2', null)
    expect(removed[0].macros.map((m) => m.key)).toEqual(['F1'])
    expect(resolveRttySet(removed, 'contest', tr)[1].custom, 'the built-in is back').toBe(false)
    expect(withRttySetReset(removed, 'contest')).toEqual([{ name: 'contest', macros: [] }, saved[1]])
    // The input is never mutated — the cockpit keeps rendering its old copy until the save lands.
    expect(saved[0].macros).toHaveLength(1)
  })

  it('adds a set that has never been saved, and keeps a key this build does not know', () => {
    expect(withRttyEntry([], 'everyday', 'F8', { label: 'QRZ', text: 'QRZ?' })).toEqual([
      { name: 'everyday', macros: [{ key: 'F8', label: 'QRZ', text: 'QRZ?' }] },
    ])
    const future = [{ name: 'contest', macros: [{ key: 'F9', label: 'X', text: 'X' }] }]
    expect(withRttyEntry(future, 'contest', 'F1', { label: 'CQ', text: 'CQ' })[0].macros.map((m) => m.key)).toEqual([
      'F1',
      'F9',
    ])
  })
})

describe('the tokens', () => {
  const ctx = { mycall: 'W9XYZ', call: 'k1abc ', exch: '05 IL' }

  it('fills MYCALL, CALL, RST (599, the RTTY report) and EXCH', () => {
    expect(expandRttyMacro('{CALL} {RST} {EXCH} {EXCH} DE {MYCALL}', ctx)).toEqual({
      text: 'K1ABC 599 05 IL 05 IL DE W9XYZ',
    })
    expect(expandRttyMacro('tu {mycall} cq', ctx), 'token names ignore case').toEqual({ text: 'tu W9XYZ cq' })
  })

  it('never turns RTTY text into CW conventions', () => {
    // cw::expand sends `!` as the worked call and {RST} as 5NN.
    expect(expandRttyMacro('AGN? ! {RST}', ctx)).toEqual({ text: 'AGN? ! 599' })
  })

  it('refuses a message whose token has nothing to fill it', () => {
    expect(expandRttyMacro('{CALL} 599', { ...ctx, call: ' ' })).toEqual({ missing: 'call' })
    expect(expandRttyMacro('CQ {MYCALL}', { ...ctx, mycall: '' })).toEqual({ missing: 'mycall' })
    // Outside a contest there is no exchange to send.
    expect(expandRttyMacro('{CALL} 599 {EXCH}', { ...ctx, exch: null })).toEqual({ missing: 'exch' })
    // POSITIVE CONTROL: the same messages go when the value is there.
    expect(expandRttyMacro('{CALL} 599 {EXCH}', ctx)).toEqual({ text: 'K1ABC 599 05 IL' })
  })

  it('flags every token it does not know, and a stray brace, and refuses to send them', () => {
    expect(unknownRttyTokens('CQ {MYCALL} {MYCALL} {NAME} {QTH} {NAME}')).toEqual(['{NAME}', '{QTH}'])
    expect(unknownRttyTokens('CQ {MYCALL')).toEqual(['{'])
    expect(unknownRttyTokens('CQ MYCALL}')).toEqual(['}'])
    expect(unknownRttyTokens('{CALL} {RST} {EXCH} {MYCALL} {call}')).toEqual([])
    expect(expandRttyMacro('{CALL} {NAME}', ctx)).toEqual({ unknown: '{NAME}' })
  })
})

describe('a macro may not be captioned as a stop', () => {
  it('refuses Stop, Esc and Abort, in any case and inside a caption', () => {
    for (const label of ['Stop', 'STOP', 'esc', 'Esc Stop', 'Stop TX', 'Abort', 'Escape']) {
      expect(isStopLikeLabel(label), label).toBe(true)
    }
  })

  it('POSITIVE CONTROL: ordinary captions pass, including ones merely containing those letters', () => {
    for (const label of ['CQ', 'TU', 'B4', 'Exch', 'Stopwatch', 'Descent', 'Rescue', '']) {
      expect(isStopLikeLabel(label), label).toBe(false)
    }
  })
})

describe('on the air', () => {
  it('each message starts a line of its own and ends in one space', () => {
    expect(frameForAir('CQ TEST W9XYZ W9XYZ CQ')).toBe('\r\nCQ TEST W9XYZ W9XYZ CQ ')
    expect(frameForAir('  TU W9XYZ CQ  ')).toBe('\r\nTU W9XYZ CQ ')
  })
})

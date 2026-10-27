// THE CW TURN CUE, DRAWN: which F-key a role lights in the set the dock shows, and the lamp's glyph,
// words, glow and speech for every state of the station's turn read. By value, state by state.
import { describe, expect, it } from 'vitest'
import { cueKeys, lamp, pace } from './cwTurn'
import { CONTEST_LAYOUT_ROLES, CW_CONTEST_LAYOUT } from './esmRoles'
import { EN } from '../i18n'
import type { CwTurn } from '../types'

/** The everyday built-in set, as the dock sends it. */
const EVERYDAY = [
  { key: 'F1', text: 'CQ CQ DE {MYCALL} {MYCALL} K' },
  { key: 'F2', text: '! DE {MYCALL} {MYCALL} K' },
  { key: 'F3', text: '! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN' },
  { key: 'F4', text: '! DE {MYCALL} TU 73 SK' },
  { key: 'F5', text: '{MYCALL}' },
  { key: 'F6', text: '! ' },
  { key: 'F7', text: 'AGN AGN' },
  { key: 'F8', text: '? ' },
]
const everyday = { slots: EVERYDAY, contest: false, roles: {}, lastKey: null }
const contest = { slots: CW_CONTEST_LAYOUT, contest: true, roles: CONTEST_LAYOUT_ROLES, lastKey: null }

const turn = (t: Partial<CwTurn>): CwTurn => ({
  state: 'idle',
  role: null,
  hint: null,
  call: null,
  over: null,
  window: null,
  ...t,
})

describe('the key a role lights', () => {
  it('in the everyday set: the keys every set keeps by place, and my call and AGN by what they send', () => {
    expect(cueKeys('cq', everyday)).toEqual(['F1'])
    expect(cueKeys('answer', everyday)).toEqual(['F2'])
    expect(cueKeys('reply', everyday)).toEqual(['F3'])
    expect(cueKeys('close', everyday)).toEqual(['F4'])
    expect(cueKeys('myCall', everyday)).toEqual(['F5'])
    expect(cueKeys('again', everyday)).toEqual(['F7'])
    // A contest's own steps have no everyday key.
    expect(cueKeys('exchange', everyday)).toEqual([])
    expect(cueKeys('tu', everyday)).toEqual([])
    expect(cueKeys(null, everyday)).toEqual([])
  })

  it('in a profile laid out its own way: my call and AGN where they are, and nothing on an empty key', () => {
    const own = [
      { key: 'F1', text: 'CQ DE {MYCALL} K' },
      { key: 'F2', text: '' },
      { key: 'F6', text: 'PSE AGN' },
      { key: 'F8', text: '{MYCALL}' },
    ]
    const ctx = { slots: own, contest: false, roles: {}, lastKey: null }
    expect(cueKeys('myCall', ctx)).toEqual(['F8'])
    expect(cueKeys('again', ctx)).toEqual(['F6'])
    expect(cueKeys('answer', ctx), 'F2 sends nothing').toEqual([])
  })

  it("in a contest: the contest layout's step for the role", () => {
    expect(cueKeys('answer', contest), 'answering a CQ in S&P: my call').toEqual(['F4'])
    expect(cueKeys('myCall', contest)).toEqual(['F4'])
    expect(cueKeys('reply', contest), 'running, a caller in: his call and my exchange').toEqual(['F2'])
    expect(cueKeys('exchange', contest)).toEqual(['F6'])
    expect(cueKeys('tu', contest)).toEqual(['F3'])
    expect(cueKeys('again', contest)).toEqual(['F8'])
    expect(cueKeys('close', contest), 'a contest contact ends on TU').toEqual([])
  })

  it('"send it again" is the key last sent, or none after typed text', () => {
    expect(cueKeys('repeat', { ...everyday, lastKey: 'F3' })).toEqual(['F3'])
    expect(cueKeys('repeat', everyday)).toEqual([])
  })
})

describe('the lamp', () => {
  it('says each state in a glyph and words, and lights the key the way the state means', () => {
    const cases: [CwTurn, string[], string, string, string[], string | null][] = [
      [turn({}), [], '◌', EN['cw.turn.idle'], [], null],
      [turn({ state: 'theySend', call: 'W1ABC' }), [], '≋', 'W1ABC sending', [], null],
      [turn({ state: 'theySend' }), [], '≋', EN['cw.turn.theySend.bare'], [], null],
      [turn({ state: 'finishing', role: 'answer', call: 'W1ABC' }), ['F2'], '≋', 'W1ABC finishing…', ['F2'], 'soon'],
      [turn({ state: 'yourTurn', role: 'reply', call: 'W1ABC' }), ['F3'], '▶︎', 'Your turn · F3', ['F3'], 'now'],
      [turn({ state: 'yourTurn', hint: 'type' }), [], '▶︎', EN['cw.turn.type'], [], null],
      [turn({ state: 'yourTurn', role: 'myCall', hint: 'partial' }), ['F5'], '▶︎', 'Probably you · F5 once', ['F5'], 'now'],
      [turn({ state: 'hold', call: 'W2XYZ' }), [], '⏸︎', 'Hold · W2XYZ', [], null],
      [turn({ state: 'hold', hint: 'splitOff' }), [], '⇡', EN['cw.turn.splitOff'], [], null],
      [turn({ state: 'unsure', role: 'again' }), ['F7'], '?', 'Not sure · F7', ['F7'], 'maybe'],
      [turn({ state: 'youSend' }), [], '●', EN['cw.turn.youSend'], [], null],
      [turn({ state: 'listen' }), [], '◔', EN['cw.turn.listen'], [], null],
      [turn({ state: 'listen', hint: 'late' }), [], '◔', EN['cw.turn.late'], [], null],
      [turn({ state: 'closing', role: 'close' }), ['F4'], '✓', EN['cw.turn.closing'], ['F4'], 'now'],
      [turn({ state: 'closing' }), [], '✓', EN['cw.turn.closing.bare'], [], null],
      [turn({ state: 'idle', role: 'cq', hint: 'noReply' }), ['F1'], '◌', 'No reply · F1', ['F1'], 'maybe'],
    ]
    for (const [t, keys, glyph, label, lit, glow] of cases) {
      const l = lamp(t, keys, false)
      expect([l.glyph, l.label, l.keys, l.glow], `${t.state}/${t.hint}`).toEqual([glyph, label, lit, glow])
    }
  })

  it('a DX coming back in a pileup: the lamp itself says to send 5NN TU, and no key is lit', () => {
    const l = lamp(turn({ state: 'yourTurn', hint: 'fiveNnTu', call: 'VP8A' }), [], false)
    expect([l.label, l.keys, l.glow]).toEqual(['Your turn · send 5NN TU', [], null])
    expect(l.title.split('\n'), 'the line, why not the Reply key, what the lamp is').toEqual([
      'Your turn · send 5NN TU',
      EN['cw.turn.fiveNnTu.title'],
      EN['cw.turn.title'],
    ])
    expect(l.speech).toEqual({ text: EN['cw.turn.say.fiveNnTu'], at: 'needed' })
  })

  it('with Enter Sends Message running, it says only when: "Enter", and lights no key of its own', () => {
    const l = lamp(turn({ state: 'yourTurn', role: 'myCall' }), ['F4'], true)
    expect([l.label, l.keys, l.glow]).toEqual(['Your turn · Enter', [], null])
    const unsure = lamp(turn({ state: 'unsure', role: 'again' }), ['F8'], true)
    expect([unsure.label, unsure.keys], 'no "Enter" when it is not your turn').toEqual([EN['cw.turn.unsure.bare'], []])
  })

  it('speaks your turn, hold and signing at the default verbosity, "not sure" and "no reply" only at all', () => {
    const at = (t: CwTurn, keys: string[] = []) => lamp(t, keys, false).speech
    expect(at(turn({ state: 'yourTurn', role: 'reply' }), ['F3'])).toEqual({ text: 'Your turn: F3.', at: 'needed' })
    expect(at(turn({ state: 'hold', call: 'W2XYZ' }))).toEqual({ text: "Hold: it is W2XYZ's turn.", at: 'needed' })
    expect(at(turn({ state: 'closing', role: 'close' }), ['F4'])?.at).toBe('needed')
    expect(at(turn({ state: 'unsure', role: 'again' }), ['F7'])?.at).toBe('all')
    expect(at(turn({ state: 'idle', hint: 'noReply', role: 'cq' }), ['F1'])?.at).toBe('all')
    for (const state of ['idle', 'theySend', 'finishing', 'youSend', 'listen'] as const) {
      expect(at(turn({ state })), `${state} says nothing`).toBeNull()
    }
  })
})

describe('the pace hint', () => {
  it('their speed beside mine, with ⇣ when mine is 3 WPM or more faster', () => {
    expect(pace(18, 25)).toEqual({ label: 'them 18 · you 25 WPM ⇣', fast: true })
    expect(pace(18, 21)).toEqual({ label: 'them 18 · you 21 WPM ⇣', fast: true })
    expect(pace(18, 20)).toEqual({ label: 'them 18 · you 20 WPM', fast: false })
    expect(pace(25, 18)).toEqual({ label: 'them 25 · you 18 WPM', fast: false })
    expect(pace(0, 25), 'no recent measurement of theirs').toBeNull()
  })
})

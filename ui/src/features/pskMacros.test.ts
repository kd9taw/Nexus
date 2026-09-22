import { describe, expect, it } from 'vitest'
import {
  expandPskMacro,
  pskSetId,
  resolvePskSet,
  unknownPskTokens,
  withPskEntry,
  withPskSetReset,
} from './pskMacros'
import { resolveRttySet } from './rttyMacros'
import type { KeyboardMacroProfile } from '../types'

// Only example callsigns here (W1AW, K1ABC, W9XYZ, N0CALL).

/** A translator that shows WHICH key a built-in caption came from, so a test can tell a
 *  translated caption from a saved one. */
const tr = (key: string) => `«${key}»`

describe('the built-in sets', () => {
  it('Everyday is the set the cockpit shipped with, plus four empty keys', () => {
    // ⚠️ THE FOUR TEXTS ARE LITERAL, and that is the point of this assertion: they are what a
    // PSK operator's F1–F4 have sent since Phase 2, so an operator who never opens the editor
    // must see the dock they already had. A change here is a change to what goes on the air.
    expect(resolvePskSet([], 'everyday', tr)).toEqual([
      { key: 'F1', label: 'CQ', text: 'CQ CQ CQ de {MYCALL} {MYCALL} pse k', custom: false },
      { key: 'F2', label: '«psk.macro.answer.label»', text: '{CALL} de {MYCALL} {MYCALL} k', custom: false },
      { key: 'F3', label: '«psk.macro.exchange.label»', text: '{CALL} de {MYCALL} ur 599 599 btu k', custom: false },
      { key: 'F4', label: '73', text: '{CALL} de {MYCALL} tnx qso 73 sk', custom: false },
      { key: 'F5', label: '', text: '', custom: false },
      { key: 'F6', label: '', text: '', custom: false },
      { key: 'F7', label: '', text: '', custom: false },
      { key: 'F8', label: '', text: '', custom: false },
    ])
  })

  it('Contest is the run/S&P set, one message per key', () => {
    expect(resolvePskSet([], 'contest', tr).map((s) => [s.key, s.label, s.text])).toEqual([
      ['F1', 'CQ', 'cq test {MYCALL} {MYCALL} cq'],
      ['F2', '«psk.macro.exch.label»', '{CALL} 599 {EXCH} {EXCH}'],
      ['F3', 'TU', 'tu {MYCALL} cq'],
      ['F4', '«psk.macro.myCall.label»', '{MYCALL} {MYCALL}'],
      ['F5', '«psk.macro.hisCall.label»', '{CALL}'],
      ['F6', '«psk.macro.spExch.label»', 'tu 599 {EXCH} {EXCH}'],
      ['F7', 'AGN', 'agn? agn?'],
      ['F8', 'B4', '{CALL} qso b4 tu {MYCALL}'],
    ])
  })

  it('names Contest only by its id — anything else is Everyday', () => {
    expect(pskSetId('contest')).toBe('contest')
    for (const other of ['', 'everyday', 'Contest', undefined, null]) expect(pskSetId(other)).toBe('everyday')
  })

  // ⚠️ THE TWO MODES SHARE THE SET MODEL, NOT THE MESSAGES. `features/macroSets.ts` is one
  // implementation on purpose; the texts are each mode's own, because Baudot has one case and
  // PSK31's varicode is full ASCII with the lower-case letters the SHORT ones. A refactor that
  // pointed PSK at RTTY's table would pass every test above except this one.
  //
  // Scoped to the keys whose text HAS PROSE IN IT, deliberately. Three contest keys are nothing
  // but tokens and a report — `{CALL} 599 {EXCH} {EXCH}`, `{MYCALL} {MYCALL}`, `{CALL}` — and
  // those agree between the modes because there is no case in them to disagree about. Asserting
  // over them too would demand a difference that would have to be invented.
  it('does NOT reuse RTTY’s texts — every built-in with prose in it is PSK’s own, mixed case', () => {
    const prose = (text: string) => /[A-Za-z]/.test(text.replace(/\{[^}]*\}/g, ''))
    let compared = 0
    for (const set of ['everyday', 'contest'] as const) {
      const psk = resolvePskSet([], set, tr)
      const rtty = resolveRttySet([], set, tr)
      expect(psk.map((s) => s.key), `${set}: both docks bind the same eight keys`).toEqual(
        rtty.map((s) => s.key),
      )
      const shared = psk.filter((s) => {
        const theirs = rtty.find((r) => r.key === s.key)!.text
        if (!prose(s.text) && !prose(theirs)) return false
        compared += 1
        return theirs === s.text
      })
      expect(shared.map((s) => s.key), `${set}: these keys carry RTTY's text verbatim`).toEqual([])
    }
    // The scope is not vacuous: most of the sixteen keys DO carry prose and were compared.
    expect(compared, 'keys with prose, across both sets').toBeGreaterThanOrEqual(9)
    // …and PSK's really is the mixed case the mode is for, not shouted Baudot.
    expect(resolvePskSet([], 'contest', tr)[0].text).toContain('cq test')
  })
})

describe('saved entries fold over the built-ins', () => {
  const saved: KeyboardMacroProfile[] = [
    { name: 'contest', macros: [{ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05' }] },
    { name: 'everyday', macros: [{ key: 'F5', label: 'Rig', text: 'rig here is 100w' }] },
  ]

  it('replaces exactly the saved keys, in their own words, and translates the rest', () => {
    const contest = resolvePskSet(saved, 'contest', tr)
    expect(contest[1]).toEqual({ key: 'F2', label: 'Run exch', text: '{CALL} 599 05 05', custom: true })
    expect(contest[0].custom, 'F1 was not saved, so it is the built-in').toBe(false)
    expect(contest[3].label, 'and a built-in caption is still translated').toBe('«psk.macro.myCall.label»')
    const everyday = resolvePskSet(saved, 'everyday', tr)
    expect(everyday[4]).toEqual({ key: 'F5', label: 'Rig', text: 'rig here is 100w', custom: true })
    expect(everyday[1].label, 'the other set is untouched by it').toBe('«psk.macro.answer.label»')
  })

  it('an entry emptied on purpose is an empty key, not the built-in', () => {
    const blanked: KeyboardMacroProfile[] = [{ name: 'everyday', macros: [{ key: 'F1', label: '', text: '' }] }]
    expect(resolvePskSet(blanked, 'everyday', tr)[0]).toEqual({ key: 'F1', label: '', text: '', custom: true })
  })

  it('writes one key, removes one key and resets one set — touching nothing else', () => {
    const one = withPskEntry(saved, 'everyday', 'F1', { label: 'Run', text: 'cq cq de {MYCALL}' })
    expect(one.find((p) => p.name === 'everyday')!.macros).toEqual([
      { key: 'F1', label: 'Run', text: 'cq cq de {MYCALL}' },
      { key: 'F5', label: 'Rig', text: 'rig here is 100w' },
    ])
    expect(one.find((p) => p.name === 'contest'), 'the other set is untouched').toEqual(saved[0])

    const gone = withPskEntry(one, 'everyday', 'F1', null)
    expect(gone.find((p) => p.name === 'everyday')!.macros).toEqual([
      { key: 'F5', label: 'Rig', text: 'rig here is 100w' },
    ])

    const reset = withPskSetReset(saved, 'contest')
    expect(reset.find((p) => p.name === 'contest')!.macros).toEqual([])
    expect(reset.find((p) => p.name === 'everyday'), 'reset is per set').toEqual(saved[1])
  })

  it('adds a set that has never been saved, and keeps a key this build does not know', () => {
    expect(withPskEntry([], 'contest', 'F3', { label: 'TU', text: 'tu de {MYCALL}' })).toEqual([
      { name: 'contest', macros: [{ key: 'F3', label: 'TU', text: 'tu de {MYCALL}' }] },
    ])
    // A file written by a later build: F9 is not ours to drop, and it sorts after the eight.
    const future: KeyboardMacroProfile[] = [
      { name: 'everyday', macros: [{ key: 'F9', label: 'Later', text: 'x' }] },
    ]
    expect(withPskEntry(future, 'everyday', 'F2', { label: 'Reply', text: 'y' })[0].macros).toEqual([
      { key: 'F2', label: 'Reply', text: 'y' },
      { key: 'F9', label: 'Later', text: 'x' },
    ])
  })
})

describe('the tokens', () => {
  const ctx = { mycall: 'W9XYZ', call: 'k1abc', exch: '04 WI' }
  const sent = (text: string) => {
    const r = expandPskMacro(text, ctx)
    return 'text' in r ? r.text : r
  }

  it('fills MYCALL, CALL, RST (599) and EXCH, leaving the mode’s own case alone', () => {
    // The CALLSIGNS go up — the convention, and what the Call box already stores — while the
    // message around them keeps the lower case PSK31 is faster in.
    expect(sent('{CALL} de {MYCALL} ur {RST} {RST} btu k')).toBe('K1ABC de W9XYZ ur 599 599 btu k')
    expect(sent('{CALL} 599 {EXCH} {EXCH}')).toBe('K1ABC 599 04 WI 04 WI')
  })

  it('matches a token whatever its case — the editor accepts one, so the sender must fill it', () => {
    // Case-insensitive on BOTH sides or neither: a `{mycall}` the editor lets through and the
    // sender does not fill would go on the air reading `{mycall}`.
    expect(unknownPskTokens('{mycall} {Call}'), 'the editor accepts these').toEqual([])
    expect(sent('cq de {mycall} {Call}')).toBe('cq de W9XYZ K1ABC')
  })

  it('refuses a message whose token has nothing to fill it', () => {
    expect(expandPskMacro('cq de {MYCALL}', { ...ctx, mycall: '  ' })).toEqual({ missing: 'mycall' })
    expect(expandPskMacro('{CALL} de {MYCALL}', { ...ctx, call: '' })).toEqual({ missing: 'call' })
    // Outside a contest {EXCH} has no value, and an EMPTY sent exchange means the same thing.
    expect(expandPskMacro('599 {EXCH}', { ...ctx, exch: null })).toEqual({ missing: 'exch' })
    expect(expandPskMacro('599 {EXCH}', { ...ctx, exch: '' })).toEqual({ missing: 'exch' })
  })

  it('flags every token it does not know, and a stray brace, and refuses to send them', () => {
    expect(unknownPskTokens('tnx {NAME} es {NAME} {QTH} }')).toEqual(['{NAME}', '{QTH}', '}'])
    expect(unknownPskTokens('{CALL} de {MYCALL}'), 'the known ones are not flagged').toEqual([])
    expect(expandPskMacro('tnx {NAME}', ctx)).toEqual({ unknown: '{NAME}' })
  })
})

describe('what PSK does NOT do to a macro', () => {
  // RTTY frames an F-key message onto a line of its own ending in a space, a contest convention
  // its receivers' parsers depend on. PSK never has, and giving it one here would change what
  // every shipped F1 puts on the air. The expander returns the message and nothing else.
  it('adds no framing: the expanded text is the message, with nothing before or after it', () => {
    const r = expandPskMacro('cq cq de {MYCALL} pse k', { mycall: 'W9XYZ', call: '', exch: null })
    expect('text' in r && r.text).toBe('cq cq de W9XYZ pse k')
  })
})

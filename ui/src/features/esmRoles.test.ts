// @vitest-environment jsdom
// Enter Sends Message — which key sends each step, by value: the contest layout on RTTY's
// contest set as it ships, the CW contest and Field Day sets re-laid out (SIGNED texts, on the
// air), the voice-slot convention, and the role picker's model of the operator's own macros.
//
// jsdom only because the CW cockpit's live sets are imported from the component that sends
// them: the layout must be checked against what is on the air today, not a copy of it.
import { describe, expect, it } from 'vitest'
import { CW_CONTEST_SETS } from '../components/CwCockpit'
import { EN } from '../i18n'
import { ESM_ROLES, type EsmRole } from './esm'
import {
  CONTEST_LAYOUT_ROLES,
  CW_BUILT_IN_ROLES,
  CW_CONTEST_LAYOUT,
  CW_CONTEST_NO_REPORT_LAYOUT,
  CW_FIELD_DAY_LAYOUT,
  cwSendsAnswer,
  RTTY_SET_ROLES,
  VOICE_KEYS,
  VOICE_SLOT_ROLES,
  cwBuiltInRoles,
  esmRoles,
  esmVoiceSlots,
  resolveEsmRole,
  type EsmRoleMap,
  type EsmSlot,
} from './esmRoles'
import type { BuiltinMacro } from './macroSets'
import { resolveRttySet } from './rttyMacros'

/** Every role's message in `slots`, by value: `{ role: text }`, or the refusal. */
const messages = (roles: EsmRoleMap, slots: readonly EsmSlot[]) =>
  Object.fromEntries(
    ESM_ROLES.map((role) => {
      const r = resolveEsmRole(roles, role, slots)
      return [role, 'why' in r ? r : r.text]
    }),
  )

const rttyContest = resolveRttySet(undefined, 'contest', (k) => k)

describe('the contest layout on RTTY\'s contest set, as it ships', () => {
  it('sends each step from its key', () => {
    expect(messages(CONTEST_LAYOUT_ROLES, rttyContest)).toEqual({
      cq: 'CQ TEST {MYCALL} {MYCALL} CQ',
      callExch: '{CALL} {RST} {EXCH} {EXCH}',
      tu: 'TU {MYCALL} CQ',
      myCall: '{MYCALL} {MYCALL}',
      exch: 'TU {RST} {EXCH} {EXCH}',
      again: 'AGN? AGN?',
      b4: '{CALL} QSO B4 TU {MYCALL}',
    })
  })

  it('is the contest set\'s table; Everyday has none', () => {
    expect(RTTY_SET_ROLES).toEqual({ contest: CONTEST_LAYOUT_ROLES, everyday: null })
  })

  it('sends the operator\'s own text on a key they changed', () => {
    const own = resolveRttySet([{ name: 'contest', macros: [{ key: 'F3', label: 'TU', text: 'TU {MYCALL} TEST' }] }], 'contest', (k) => k)
    expect(resolveEsmRole(CONTEST_LAYOUT_ROLES, 'tu', own)).toEqual({ keys: ['F3'], text: 'TU {MYCALL} TEST' })
  })
})

describe('the CW contest and Field Day sets in the contest layout — SIGNED, and not on the air yet', () => {
  /** A set as the signature and the manual table it: key, caption (English), text. */
  const rows = (set: readonly BuiltinMacro[]) =>
    set.map((m) => [m.key, m.labelKey ? EN[m.labelKey] : m.label, m.text])

  it('carries the signed texts, key for key: the contest set with a report', () => {
    expect(rows(CW_CONTEST_LAYOUT)).toEqual([
      ['F1', 'CQ TEST', 'CQ TEST DE {MYCALL} {MYCALL} K'],
      ['F2', 'Exch', '! {RST} {EXCH}'],
      ['F3', 'TU', 'TU {MYCALL}'],
      ['F4', 'My Call', '{MYCALL}'],
      ['F5', 'His Call', '! '],
      ['F6', 'S&P exch', 'TU {RST} {EXCH}'],
      ['F7', 'B4', '! QSO B4'],
      ['F8', 'AGN', 'AGN AGN'],
    ])
  })

  it('carries the signed texts, key for key: the contest set without a report', () => {
    expect(rows(CW_CONTEST_NO_REPORT_LAYOUT)).toEqual([
      ['F1', 'CQ TEST', 'CQ TEST DE {MYCALL} {MYCALL} K'],
      ['F2', 'Exch', '! {EXCH}'],
      ['F3', 'TU', 'TU {MYCALL}'],
      ['F4', 'My Call', '{MYCALL}'],
      ['F5', 'His Call', '! '],
      ['F6', 'S&P exch', 'TU {EXCH}'],
      ['F7', 'B4', '! QSO B4'],
      ['F8', 'AGN', 'AGN AGN'],
    ])
  })

  it('carries the signed texts, key for key: the Field Day set', () => {
    expect(rows(CW_FIELD_DAY_LAYOUT)).toEqual([
      ['F1', 'CQ FD', 'CQ FD DE {MYCALL} {MYCALL} K'],
      ['F2', 'Exch', '! {EXCH}'],
      ['F3', 'TU', 'TU {MYCALL}'],
      ['F4', 'My Call', '{MYCALL}'],
      ['F5', 'His Call', '! '],
      ['F6', 'S&P exch', 'TU {EXCH}'],
      ['F7', 'B4', '! QSO B4'],
      ['F8', 'AGN', 'AGN AGN'],
    ])
  })

  it('gives CW its own S&P caption, not the RTTY set\'s', () => {
    for (const set of [CW_CONTEST_LAYOUT, CW_CONTEST_NO_REPORT_LAYOUT, CW_FIELD_DAY_LAYOUT]) {
      expect(set.find((m) => m.key === 'F6')?.labelKey).toBe('cw.macro.spExch.label')
    }
  })

  it('with a report: his call and the exchange on F2, TU on F3, my call on F4, the S&P exchange on F6', () => {
    expect(messages(CONTEST_LAYOUT_ROLES, CW_CONTEST_LAYOUT)).toEqual({
      cq: 'CQ TEST DE {MYCALL} {MYCALL} K',
      callExch: '! {RST} {EXCH}',
      tu: 'TU {MYCALL}',
      myCall: '{MYCALL}',
      exch: 'TU {RST} {EXCH}',
      again: 'AGN AGN',
      b4: '! QSO B4',
    })
  })

  it('without a report, and at Field Day: the same steps, with no {RST} anywhere', () => {
    expect(messages(CONTEST_LAYOUT_ROLES, CW_CONTEST_NO_REPORT_LAYOUT)).toEqual({
      cq: 'CQ TEST DE {MYCALL} {MYCALL} K',
      callExch: '! {EXCH}',
      tu: 'TU {MYCALL}',
      myCall: '{MYCALL}',
      exch: 'TU {EXCH}',
      again: 'AGN AGN',
      b4: '! QSO B4',
    })
    expect(messages(CONTEST_LAYOUT_ROLES, CW_FIELD_DAY_LAYOUT)).toEqual({
      cq: 'CQ FD DE {MYCALL} {MYCALL} K',
      callExch: '! {EXCH}',
      tu: 'TU {MYCALL}',
      myCall: '{MYCALL}',
      exch: 'TU {EXCH}',
      again: 'AGN AGN',
      b4: '! QSO B4',
    })
    for (const set of [CW_CONTEST_NO_REPORT_LAYOUT, CW_FIELD_DAY_LAYOUT]) {
      expect(set.filter((m) => m.text.includes('{RST}'))).toEqual([])
    }
  })

  it('keeps the CQ and AGN the sets sent before, AGN on F8 and B4 on F7, and their his-call text on F5', () => {
    // What F1, F7 and F6 sent before the layout (the signature's "Today" column). AGN and B4
    // then traded keys (the operator, 2026-10-10): AGN on F8.
    for (const [layout, cq] of [
      [CW_CONTEST_LAYOUT, 'CQ TEST DE {MYCALL} {MYCALL} K'],
      [CW_CONTEST_NO_REPORT_LAYOUT, 'CQ TEST DE {MYCALL} {MYCALL} K'],
      [CW_FIELD_DAY_LAYOUT, 'CQ FD DE {MYCALL} {MYCALL} K'],
    ] as const) {
      expect(layout.find((m) => m.key === 'F1')?.text).toBe(cq)
      expect(layout.find((m) => m.key === 'F8')?.text).toBe('AGN AGN')
      expect(layout.find((m) => m.key === 'F7')?.text).toBe('! QSO B4')
      expect(layout.find((m) => m.key === 'F5')?.text).toBe('! ')
    }
  })

  it('sends AGN from F8, the key the sets now carry it on', () => {
    expect(CONTEST_LAYOUT_ROLES.again).toEqual(['F8'])
  })

  it('sends QSO B4 from F7, the key all three sets hold it on, and no other step from it', () => {
    expect(CONTEST_LAYOUT_ROLES.b4).toEqual(['F7'])
    const others = Object.entries(CONTEST_LAYOUT_ROLES).filter(([role]) => role !== 'b4')
    expect(others.filter(([, keys]) => keys?.includes('F7'))).toEqual([])
  })

  it('gets the layout\'s role table only when the set in use IS a layout set, key for key and text for text', () => {
    expect(cwBuiltInRoles(CW_CONTEST_LAYOUT)).toBe(CONTEST_LAYOUT_ROLES)
    expect(cwBuiltInRoles(CW_CONTEST_NO_REPORT_LAYOUT)).toBe(CONTEST_LAYOUT_ROLES)
    expect(cwBuiltInRoles(CW_FIELD_DAY_LAYOUT)).toBe(CONTEST_LAYOUT_ROLES)
    const edited = CW_FIELD_DAY_LAYOUT.map((m) => (m.key === 'F3' ? { ...m, text: '73' } : m))
    expect(cwBuiltInRoles(edited)).toBeNull()
    expect(cwBuiltInRoles(CW_CONTEST_LAYOUT.slice(0, 7))).toBeNull()
  })

  it('is on the air: the CW cockpit sends the three layout sets, F3 is TU in each, and ESM finds the layout in them', () => {
    expect(CW_CONTEST_SETS).toEqual({
      fieldDay: CW_FIELD_DAY_LAYOUT,
      report: CW_CONTEST_LAYOUT,
      noReport: CW_CONTEST_NO_REPORT_LAYOUT,
    })
    for (const set of Object.values(CW_CONTEST_SETS)) {
      expect(set.find((m) => m.key === 'F3')?.text).toBe('TU {MYCALL}')
      expect(cwBuiltInRoles(set)).toBe(CONTEST_LAYOUT_ROLES)
    }
  })

  it('shows Settings the steps of the sets the cockpit actually sends', () => {
    expect(CW_BUILT_IN_ROLES).toBe(CONTEST_LAYOUT_ROLES)
    for (const set of Object.values(CW_CONTEST_SETS)) expect(cwBuiltInRoles(set)).toBe(CW_BUILT_IN_ROLES)
  })
})

describe('the voice-slot convention', () => {
  const recorded = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F8'].map((key) => ({ key, text: `${key}.wav` }))

  it('F1 CQ · F2 my exchange · F3 TU · F4 my call · F8 AGN, and no slot for his call or for QSO B4', () => {
    expect(messages(VOICE_SLOT_ROLES, recorded)).toEqual({
      cq: 'F1.wav',
      callExch: { why: 'unmapped', role: 'callExch' },
      tu: 'F3.wav',
      myCall: 'F4.wav',
      exch: 'F2.wav',
      again: 'F8.wav',
      b4: { why: 'unmapped', role: 'b4' },
    })
  })

  it('sends AGN from the key the CW and RTTY contest sets send it from', () => {
    expect(VOICE_SLOT_ROLES.again).toEqual(['F8'])
    expect(VOICE_SLOT_ROLES.again).toEqual(CONTEST_LAYOUT_ROLES.again)
  })

  it('leaves F5 and F6 to the operator: F5 is his call on CW, and a recording cannot say one', () => {
    const played = Object.values(VOICE_SLOT_ROLES).flat()
    expect(played.filter((key) => key === 'F5' || key === 'F6')).toEqual([])
  })

  it('offers the role picker the keyer’s slots: F1–F6, and F8 for AGN', () => {
    expect(VOICE_KEYS).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F8'])
  })

  it('names the slot to record when it is empty', () => {
    const noTu = recorded.map((s) => (s.key === 'F3' ? { ...s, text: '' } : s))
    expect(resolveEsmRole(VOICE_SLOT_ROLES, 'tu', noTu)).toEqual({ why: 'empty', role: 'tu', key: 'F3' })
  })

  it('reads the keyer\'s slots: a recorded one by its label, or its key when the label is blank; an empty one as empty', () => {
    expect(
      esmVoiceSlots([
        { slot: 1, label: 'CQ', file: 'cq.wav' },
        { slot: 2, label: 'Exchange', file: '' },
        { slot: 3, label: '  ', file: 'tu.wav' },
      ]),
    ).toEqual([
      { key: 'F1', text: 'CQ' },
      { key: 'F2', text: '' },
      { key: 'F3', text: 'F3' },
    ])
  })
})

describe('the role picker — the operator\'s own macros mapped to ESM\'s steps', () => {
  // A CW set of the operator's own: his call on F5, the exchange alone on F2.
  const own: EsmSlot[] = [
    { key: 'F1', text: 'CQ TEST {MYCALL}' },
    { key: 'F2', text: '5NN {EXCH}' },
    { key: 'F3', text: 'TU {MYCALL} TEST' },
    { key: 'F4', text: '{MYCALL}' },
    { key: 'F5', text: '!' },
    { key: 'F6', text: '' },
  ]
  const mapped: EsmRoleMap = { cq: ['F1'], callExch: ['F5', 'F2'], tu: ['F3'], myCall: ['F4'] }

  it('resolves a mapped role to its key and text', () => {
    expect(resolveEsmRole(mapped, 'tu', own)).toEqual({ keys: ['F3'], text: 'TU {MYCALL} TEST' })
  })

  it('sends two keys as one message, in order — F5 then F2', () => {
    expect(resolveEsmRole(mapped, 'callExch', own)).toEqual({ keys: ['F5', 'F2'], text: '! 5NN {EXCH}' })
  })

  it('refuses a role with no macro, by name', () => {
    expect(resolveEsmRole(mapped, 'again', own)).toEqual({ why: 'unmapped', role: 'again' })
    expect(resolveEsmRole({ ...mapped, exch: [] }, 'exch', own)).toEqual({ why: 'unmapped', role: 'exch' })
  })

  it('refuses a role whose key sends nothing, naming the role and the key', () => {
    expect(resolveEsmRole({ exch: ['F6'] }, 'exch', own)).toEqual({ why: 'empty', role: 'exch', key: 'F6' })
    expect(resolveEsmRole({ exch: ['F8'] }, 'exch', own)).toEqual({ why: 'empty', role: 'exch', key: 'F8' })
    expect(resolveEsmRole({ callExch: ['F5', 'F6'] }, 'callExch', own)).toEqual({ why: 'empty', role: 'callExch', key: 'F6' })
  })

  it('a set of the operator\'s own has only the roles they mapped', () => {
    expect(esmRoles(null, mapped)).toEqual(mapped)
    expect(esmRoles(null, undefined)).toEqual({})
  })

  it('over a built-in set, the operator\'s mapping wins role by role and the rest keep the built-in key', () => {
    const roles = esmRoles(CONTEST_LAYOUT_ROLES, { tu: ['F5'], again: [] })
    expect(roles).toEqual({ ...CONTEST_LAYOUT_ROLES, tu: ['F5'] })
    const roleNames: EsmRole[] = ['cq', 'callExch', 'myCall', 'exch', 'again']
    for (const role of roleNames) expect(roles[role]).toEqual(CONTEST_LAYOUT_ROLES[role])
  })
})

// CW SPEED MATCH: which CW messages answer a station.
describe('cwSendsAnswer', () => {
  it('reads a contest step by its role: every step answers but the CQ', () => {
    const r = CONTEST_LAYOUT_ROLES
    expect(cwSendsAnswer(r, 'F1', 'CQ TEST DE {MYCALL} {MYCALL} K')).toBe(false)
    for (const key of ['F2', 'F3', 'F4', 'F6', 'F7', 'F8']) {
      expect(cwSendsAnswer(r, key, 'anything'), key).toBe(true)
    }
  })
  it('reads the role over the text: a CQ step is no answer whatever it says, a step is', () => {
    expect(cwSendsAnswer({ cq: ['F1'] }, 'F1', 'TEST {MYCALL}')).toBe(false)
    expect(cwSendsAnswer({ myCall: ['F1'] }, 'F1', 'CQ')).toBe(true)
  })
  it('reads a key with no step, and typed text, by its first word', () => {
    expect(cwSendsAnswer({}, 'F1', 'CQ CQ DE {MYCALL} K')).toBe(false)
    expect(cwSendsAnswer({}, 'F2', '! DE {MYCALL} {MYCALL} K')).toBe(true)
    expect(cwSendsAnswer({}, null, '  cq test')).toBe(false)
    expect(cwSendsAnswer({}, null, 'TU 73')).toBe(true)
    expect(cwSendsAnswer(CONTEST_LAYOUT_ROLES, 'F5', '! ')).toBe(true)
  })
})

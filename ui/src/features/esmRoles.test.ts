// @vitest-environment jsdom
// Enter Sends Message — which key sends each step, by value: the contest layout on RTTY's
// contest set as it ships, the CW contest and Field Day sets re-laid out (SIGNED texts, not on
// the air yet), the voice-slot convention, and the role picker's model of the operator's own
// macros.
//
// jsdom only because the CW cockpit's live sets are imported from the component that sends
// them: the layout must be checked against what is on the air today, not a copy of it.
import { describe, expect, it } from 'vitest'
import {
  CW_CONTEST_SETS,
  DEFAULT_CONTEST_MACROS,
  DEFAULT_CONTEST_NO_REPORT_MACROS,
  DEFAULT_FD_MACROS,
} from '../components/CwCockpit'
import { EN } from '../i18n'
import { ESM_ROLES, type EsmRole } from './esm'
import {
  CONTEST_LAYOUT_ROLES,
  CW_BUILT_IN_ROLES,
  CW_CONTEST_LAYOUT,
  CW_CONTEST_NO_REPORT_LAYOUT,
  CW_FIELD_DAY_LAYOUT,
  CW_LAYOUT_ON_AIR,
  RTTY_SET_ROLES,
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
      callExch: '{CALL} 599 {EXCH} {EXCH}',
      tu: 'TU {MYCALL} CQ',
      myCall: '{MYCALL} {MYCALL}',
      exch: 'TU 599 {EXCH} {EXCH}',
      again: 'AGN? AGN?',
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
      ['F7', 'AGN', 'AGN AGN'],
      ['F8', 'B4', '! QSO B4'],
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
      ['F7', 'AGN', 'AGN AGN'],
      ['F8', 'B4', '! QSO B4'],
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
      ['F7', 'AGN', 'AGN AGN'],
      ['F8', 'B4', '! QSO B4'],
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
    })
    expect(messages(CONTEST_LAYOUT_ROLES, CW_FIELD_DAY_LAYOUT)).toEqual({
      cq: 'CQ FD DE {MYCALL} {MYCALL} K',
      callExch: '! {EXCH}',
      tu: 'TU {MYCALL}',
      myCall: '{MYCALL}',
      exch: 'TU {EXCH}',
      again: 'AGN AGN',
    })
    for (const set of [CW_CONTEST_NO_REPORT_LAYOUT, CW_FIELD_DAY_LAYOUT]) {
      expect(set.filter((m) => m.text.includes('{RST}'))).toEqual([])
    }
  })

  it('keeps today\'s CQ and AGN, and today\'s his-call text, now on F5', () => {
    const live = (set: typeof DEFAULT_CONTEST_MACROS, key: string) => set.find((m) => m.key === key)?.text
    for (const [layout, today] of [
      [CW_CONTEST_LAYOUT, DEFAULT_CONTEST_MACROS],
      [CW_CONTEST_NO_REPORT_LAYOUT, DEFAULT_CONTEST_NO_REPORT_MACROS],
      [CW_FIELD_DAY_LAYOUT, DEFAULT_FD_MACROS],
    ] as const) {
      expect(layout.find((m) => m.key === 'F1')?.text).toBe(live(today, 'F1'))
      expect(layout.find((m) => m.key === 'F7')?.text).toBe(live(today, 'F7'))
      expect(layout.find((m) => m.key === 'F5')?.text).toBe(live(today, 'F6'))
    }
  })

  it('gets the layout\'s role table only when the set in use IS a layout set, key for key and text for text', () => {
    expect(cwBuiltInRoles(CW_CONTEST_LAYOUT)).toBe(CONTEST_LAYOUT_ROLES)
    expect(cwBuiltInRoles(CW_CONTEST_NO_REPORT_LAYOUT)).toBe(CONTEST_LAYOUT_ROLES)
    expect(cwBuiltInRoles(CW_FIELD_DAY_LAYOUT)).toBe(CONTEST_LAYOUT_ROLES)
    const edited = CW_FIELD_DAY_LAYOUT.map((m) => (m.key === 'F3' ? { ...m, text: '73' } : m))
    expect(cwBuiltInRoles(edited)).toBeNull()
    expect(cwBuiltInRoles(CW_CONTEST_LAYOUT.slice(0, 7))).toBeNull()
  })

  it('is not on the air: the switch is off, the CW cockpit sends today\'s sets, and ESM finds no role table in them', () => {
    expect(CW_LAYOUT_ON_AIR).toBe(false)
    expect(CW_CONTEST_SETS).toEqual({
      fieldDay: DEFAULT_FD_MACROS,
      report: DEFAULT_CONTEST_MACROS,
      noReport: DEFAULT_CONTEST_NO_REPORT_MACROS,
    })
    expect(DEFAULT_CONTEST_MACROS.find((m) => m.key === 'F3')?.text).toBe('! DE {MYCALL} {RST} {EXCH} {EXCH} K')
    expect(DEFAULT_CONTEST_NO_REPORT_MACROS.find((m) => m.key === 'F3')?.text).toBe('! DE {MYCALL} {EXCH} {EXCH} K')
    expect(DEFAULT_FD_MACROS.find((m) => m.key === 'F3')?.text).toBe('! DE {MYCALL} {EXCH} {EXCH} K')
    expect(cwBuiltInRoles(DEFAULT_CONTEST_MACROS)).toBeNull()
    expect(cwBuiltInRoles(DEFAULT_CONTEST_NO_REPORT_MACROS)).toBeNull()
    expect(cwBuiltInRoles(DEFAULT_FD_MACROS)).toBeNull()
  })

  it('shows Settings the steps of the sets the cockpit actually sends, whichever way the switch is set', () => {
    for (const set of Object.values(CW_CONTEST_SETS)) expect(cwBuiltInRoles(set)).toBe(CW_BUILT_IN_ROLES)
  })
})

describe('the voice-slot convention', () => {
  const recorded = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6'].map((key) => ({ key, text: `${key}.wav` }))

  it('F1 CQ · F2 my exchange · F3 TU · F4 my call · F5 AGN, and no slot for his call', () => {
    expect(messages(VOICE_SLOT_ROLES, recorded)).toEqual({
      cq: 'F1.wav',
      callExch: { why: 'unmapped', role: 'callExch' },
      tu: 'F3.wav',
      myCall: 'F4.wav',
      exch: 'F2.wav',
      again: 'F5.wav',
    })
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
  // An N1MM-style CW set of the operator's own: his call on F5, the exchange alone on F2.
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

  it('sends two keys as one message, in order — N1MM\'s "F5 followed by F2"', () => {
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

// @vitest-environment jsdom
//
// ENTER SENDS MESSAGE'S ROLE PICKER, by value: it MAPS (a choice writes the set's own mapping),
// it RESOLVES (each step shows what Enter would send, through the model a press uses), and it
// REFUSES BY NAME (a step that would send nothing says why, naming the step and the key). A
// built-in table is shown read-only beside the operator's own choice, and a set that is not
// the operator's to map shows no control at all.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render } from '@testing-library/react'
import { EsmRolePicker } from './EsmRolePicker'
import { CONTEST_LAYOUT_ROLES, RTTY_SET_ROLES, VOICE_KEYS, VOICE_SLOT_ROLES, type EsmRoleMap } from '../features/esmRoles'
import { MACRO_KEYS } from '../features/macroSets'
import { resolveRttySet } from '../features/rttyMacros'

// THE BUDGET (2026-10-09). Every case here renders one small table and takes well under 0.1 s on
// one core; a loaded full suite on this box has run cases up to 20 times slower than one core,
// past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

afterEach(cleanup)

/** An N1MM-style CW set of the operator's own: his call on F5, the exchange alone on F2. */
const OWN_CW = [
  { key: 'F1', text: 'CQ TEST {MYCALL}' },
  { key: 'F2', text: '5NN {EXCH}' },
  { key: 'F3', text: 'TU {MYCALL} TEST' },
  { key: 'F4', text: '{MYCALL}' },
  { key: 'F5', text: '!' },
  { key: 'F6', text: '' },
]
const identity = (k: string) => k

/** Each step's row as the operator reads it: what it sends, or its refusal's reason. */
const rows = () =>
  Object.fromEntries(
    [...document.querySelectorAll('[data-esm-role]')].map((row) => {
      const sends = row.querySelector('.esm-role-sends')
      return [
        row.getAttribute('data-esm-role'),
        sends ? { why: sends.getAttribute('data-esm-refused') ?? null, text: sends.textContent } : null,
      ]
    }),
  )

const keySelect = (role: string) =>
  document.querySelector(`[data-esm-role="${role}"] select`) as HTMLSelectElement
const thenSelect = (role: string) =>
  document.querySelectorAll(`[data-esm-role="${role}"] select`)[1] as HTMLSelectElement | undefined

describe('the role picker on a set of the operator\'s own', () => {
  it('maps a step: the choice writes the set\'s own mapping, by value', () => {
    const onChange = vi.fn()
    render(<EsmRolePicker cockpit="cw" builtIn={null} slots={OWN_CW} keys={MACRO_KEYS} onChange={onChange} />)
    fireEvent.change(keySelect('tu'), { target: { value: 'F3' } })
    expect(onChange).toHaveBeenLastCalledWith({ tu: ['F3'] })
  })

  it('maps two keys to one step, in order — N1MM\'s "F5 followed by F2"', () => {
    const onChange = vi.fn()
    const own: EsmRoleMap = { callExch: ['F5'] }
    render(<EsmRolePicker cockpit="cw" builtIn={null} own={own} slots={OWN_CW} keys={MACRO_KEYS} onChange={onChange} />)
    fireEvent.change(thenSelect('callExch')!, { target: { value: 'F2' } })
    expect(onChange).toHaveBeenLastCalledWith({ callExch: ['F5', 'F2'] })
  })

  it('resolves each mapped step to what Enter would send, two keys joined', () => {
    const own: EsmRoleMap = { cq: ['F1'], callExch: ['F5', 'F2'], tu: ['F3'], myCall: ['F4'], exch: ['F6'] }
    render(<EsmRolePicker cockpit="cw" builtIn={null} own={own} slots={OWN_CW} keys={MACRO_KEYS} onChange={() => {}} />)
    expect(rows()).toEqual({
      cq: { why: null, text: 'CQ TEST {MYCALL}' },
      callExch: { why: null, text: '! 5NN {EXCH}' },
      tu: { why: null, text: 'TU {MYCALL} TEST' },
      myCall: { why: null, text: '{MYCALL}' },
      exch: { why: 'empty', text: 'Your S&P exchange: F6 is empty, so Enter sends nothing and logs nothing at that step.' },
      again: { why: 'unmapped', text: 'AGN: no key is mapped, so Enter sends nothing and logs nothing at that step.' },
    })
  })

  it('clears a step, and with the last one gone the set has no mapping at all', () => {
    const onChange = vi.fn()
    const { rerender } = render(
      <EsmRolePicker cockpit="cw" builtIn={null} own={{ tu: ['F3'], again: ['F7'] }} slots={OWN_CW} keys={MACRO_KEYS} onChange={onChange} />,
    )
    fireEvent.change(keySelect('tu'), { target: { value: '' } })
    expect(onChange).toHaveBeenLastCalledWith({ again: ['F7'] })
    rerender(<EsmRolePicker cockpit="cw" builtIn={null} own={{ again: ['F7'] }} slots={OWN_CW} keys={MACRO_KEYS} onChange={onChange} />)
    fireEvent.change(keySelect('again'), { target: { value: '' } })
    expect(onChange).toHaveBeenLastCalledWith(undefined)
  })

  it('says a set with nothing mapped makes ESM step aside', () => {
    render(<EsmRolePicker cockpit="cw" builtIn={null} slots={OWN_CW} keys={MACRO_KEYS} onChange={() => {}} />)
    expect(document.querySelector('.esm-roles-none')?.textContent).toBe(
      'This set has no step mapped, so ESM steps aside: Enter logs as it does with ESM off.',
    )
    // POSITIVE CONTROL: one step mapped, and the sentence goes.
    cleanup()
    render(<EsmRolePicker cockpit="cw" builtIn={null} own={{ cq: ['F1'] }} slots={OWN_CW} keys={MACRO_KEYS} onChange={() => {}} />)
    expect(document.querySelector('.esm-roles-none')).toBeNull()
  })
})

describe('the role picker over a built-in table', () => {
  const contest = resolveRttySet(undefined, 'contest', identity)

  it('shows the built-in key of each step read-only, and a step left on it sends the built-in key\'s text', () => {
    render(<EsmRolePicker cockpit="rtty" builtIn={RTTY_SET_ROLES.contest} slots={contest} keys={MACRO_KEYS} onChange={() => {}} />)
    const builtIn = Object.fromEntries(
      [...document.querySelectorAll('[data-esm-role]')].map((row) => [
        row.getAttribute('data-esm-role'),
        row.querySelector('.esm-role-builtin')?.textContent,
      ]),
    )
    expect(builtIn).toEqual({ cq: 'F1', callExch: 'F2', tu: 'F3', myCall: 'F4', exch: 'F6', again: 'F7' })
    expect(rows().tu).toEqual({ why: null, text: 'TU {MYCALL} CQ' })
    expect(keySelect('tu').value).toBe('')
  })

  it('lets the operator\'s choice win, step by step, over the built-in key', () => {
    render(
      <EsmRolePicker cockpit="rtty" builtIn={CONTEST_LAYOUT_ROLES} own={{ tu: ['F8'] }} slots={contest} keys={MACRO_KEYS} onChange={() => {}} />,
    )
    expect(rows().tu).toEqual({ why: null, text: '{CALL} QSO B4 TU {MYCALL}' })
    expect(rows().cq).toEqual({ why: null, text: 'CQ TEST {MYCALL} {MYCALL} CQ' })
  })
})

describe('the role picker on the voice keyer', () => {
  const slots = [
    { key: 'F1', text: 'CQ' },
    { key: 'F2', text: 'Exchange' },
    { key: 'F3', text: '' },
    { key: 'F4', text: 'My call' },
    { key: 'F5', text: 'Again' },
    { key: 'F6', text: '' },
  ]

  it('takes one slot per step, names the slot to record, and refuses a step mapped to two', () => {
    render(
      <EsmRolePicker cockpit="phone" builtIn={VOICE_SLOT_ROLES} own={{ again: ['F5', 'F4'] }} slots={slots} keys={VOICE_KEYS} onChange={() => {}} />,
    )
    expect(thenSelect('cq')).toBeUndefined()
    expect([...keySelect('cq').options].map((o) => o.value)).toEqual(['', 'F1', 'F2', 'F3', 'F4', 'F5', 'F6'])
    expect(rows()).toEqual({
      cq: { why: null, text: 'CQ' },
      callExch: { why: 'unmapped', text: 'His call and your exchange: no key is mapped, so Enter sends nothing and logs nothing at that step.' },
      tu: { why: 'empty', text: 'TU: F3 is empty, so Enter sends nothing and logs nothing at that step.' },
      myCall: { why: null, text: 'My call' },
      exch: { why: null, text: 'Exchange' },
      again: { why: 'oneSlot', text: 'AGN: mapped to two recordings, and the keyer plays one per press.' },
    })
  })
})

describe('a set that is not the operator\'s to map', () => {
  it('shows the built-in steps\' keys and no control', () => {
    render(<EsmRolePicker cockpit="cw" builtIn={CONTEST_LAYOUT_ROLES} keys={MACRO_KEYS} />)
    expect(document.querySelectorAll('select')).toHaveLength(0)
    expect(document.querySelectorAll('.esm-role-sends')).toHaveLength(0)
    expect([...document.querySelectorAll('.esm-role-builtin')].map((k) => k.textContent)).toEqual([
      'F1', 'F2', 'F3', 'F4', 'F6', 'F7',
    ])
  })

  it('with no built-in steps, says only that ESM steps aside', () => {
    render(<EsmRolePicker cockpit="cw" builtIn={null} keys={MACRO_KEYS} />)
    expect(document.querySelectorAll('[data-esm-role]')).toHaveLength(0)
    expect(document.querySelector('.esm-roles-none')?.textContent).toBe(
      'This set has no step mapped, so ESM steps aside: Enter logs as it does with ESM off.',
    )
  })
})

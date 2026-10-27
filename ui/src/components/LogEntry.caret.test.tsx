// @vitest-environment jsdom
//
// THE CARET STAYS WHERE THE OPERATOR IS TYPING. The Call box upper-cases as it goes, and a
// controlled input whose value comes back changed puts the caret at the end. An operator who types
// the suffix first ("abc"), goes Home and types the prefix had the caret jump past the C after the
// first letter. These type the way a keyboard does — the character goes in AT the caret — in the
// real `LogEntry`, normal strip and contest strip (the same strip the logger window and the contest
// screen render).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import { ContestContactEdit } from './ContestContactEdit'
import type { AppSnapshot, FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestLogManual: vi.fn(() => Promise.resolve({})),
  contestWorking: vi.fn(() => Promise.resolve({})),
  contestEntryReset: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
  setLogFormGrid: vi.fn(() => Promise.resolve()),
}))

const snap = { radio: { band: '20m', dialMhz: 14.2 }, hunt: null } as unknown as AppSnapshot

const cqww = {
  running: true,
  state: 'Idle',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
  role: '',
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'CQZ', kind: 'pattern', required: true },
  ],
  composing: [{ key: 'CQZ', raw: '14' }],
} as unknown as FieldDayStatus

function strip(fieldDay?: FieldDayStatus) {
  return render(
    <LogEntry
      onOpenLogbook={() => {}}
      snap={snap}
      mode="SSB"
      defaultRst="59"
      exchange="terrestrial"
      titled={false}
      onSpot={() => {}}
      pendingWork={null}
      onConsumeWork={() => {}}
      fieldDay={fieldDay}
      fdMode="PH"
    />,
  )
}

/** One key at a time, as a browser types it: the character replaces the selection, the caret
 *  lands after it, then `input` fires. */
function typeAtCaret(el: HTMLInputElement, text: string) {
  el.focus()
  for (const ch of text) {
    act(() => {
      el.setRangeText(ch, el.selectionStart ?? el.value.length, el.selectionEnd ?? el.value.length, 'end')
      el.dispatchEvent(new Event('input', { bubbles: true }))
    })
  }
}

/** Suffix first, Home, then the prefix — the operator's own way of entering DL1ABC. */
function suffixThenPrefix(el: HTMLInputElement) {
  typeAtCaret(el, 'abc')
  expect(el.value).toBe('ABC')
  el.setSelectionRange(0, 0)
  typeAtCaret(el, 'd')
  expect(el.value).toBe('DABC')
  expect(el.selectionStart).toBe(1)
  typeAtCaret(el, 'l1')
  expect(el.value).toBe('DL1ABC')
  expect([el.selectionStart, el.selectionEnd]).toEqual([3, 3])
}

afterEach(cleanup)

describe('the caret stays after the typed character in a box that upper-cases', () => {
  it('the normal log entry Call box', () => {
    strip()
    suffixThenPrefix(document.querySelector('input.le-call') as HTMLInputElement)
  })

  it('the contest strip Call box (cockpit strip, contest screen, logger window)', () => {
    strip(cqww)
    suffixThenPrefix(document.querySelector('input.le-fd-input-call') as HTMLInputElement)
  })

  it('the contest strip Call box drops a space and keeps the caret beside the typed text', () => {
    strip(cqww)
    const el = document.querySelector('input.le-fd-input-call') as HTMLInputElement
    typeAtCaret(el, 'abc')
    el.setSelectionRange(0, 0)
    act(() => {
      el.setRangeText('dl 1', 0, 0, 'end')
      el.dispatchEvent(new Event('input', { bubbles: true }))
    })
    expect(el.value).toBe('DL1ABC')
    expect(el.selectionStart).toBe(3)
  })

  it('a contest exchange box', () => {
    strip(cqww)
    const boxes = document.querySelectorAll('.le-fd-big input.le-fd-input-code')
    const zone = boxes[boxes.length - 1] as HTMLInputElement
    typeAtCaret(zone, 'dl')
    zone.setSelectionRange(0, 0)
    typeAtCaret(zone, 'e')
    expect(zone.value).toBe('EDL')
    expect(zone.selectionStart).toBe(1)
  })

  it('the contest contact editor', () => {
    render(
      <ContestContactEdit
        rows={[{ call: '', fields: {} } as never]}
        receives={[{ key: 'CQZ', kind: 'pattern', required: true } as never]}
        role=""
        onDone={() => {}}
      />,
    )
    suffixThenPrefix(document.querySelector('.contest-edit-call input') as HTMLInputElement)
  })
})

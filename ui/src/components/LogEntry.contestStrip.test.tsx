// @vitest-environment jsdom
//
// THE DYNAMIC ENTRY STRIP (spec §9), rendered with THE PROPS APP GIVES IT.
//
// ⚠️ **Not a stubbed component.** These render the real `LogEntry` with the prop set
// `PhoneCockpit` passes (snap, mode, defaultRst, exchange, titled, onSpot, pendingWork,
// onConsumeWork, fieldDay, fdMode) and a `fieldDay` object of the shape the DTO really
// produces — `receives`, `composing`, `role` and all. A stub proves prop-passing;
// this project has shipped a broken feature through five green reviews that way.
//
// The five properties the Field Day strip has TODAY and must still have once the boxes
// are driven by the session's receive order rather than hardcoded:
//
//   1. stacked, captioned fields — Call plus one box per received slot, IN ORDER
//   2. the space walk — Call → field₁ → … → Call, and the space never reaches a value
//   3. the fields persist across log-and-clear, so a run does not re-type them
//   4. the verdict slot is always present, empty or not (a fixed-height reservation)
//   5. the while-typing verdict costs ZERO IPC
//
// ⚠️ **jsdom lays out nothing** — every `getBoundingClientRect` here is 0×0. Not one
// assertion in this file is about geometry. The 1024 px width budget is measured in
// `ui/layout-harness/entry-strip.html` under headless Chrome, and nowhere else.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot, FieldDayStatus } from '../types'

// The whole api surface this strip imports, mocked — so "zero IPC" can be asserted as
// "not one of these was called", which is the only form of that claim worth making.
vi.mock('../api', () => ({
  fdLogManual: vi.fn(() => Promise.resolve({})),
  contestIMoved: vi.fn(() => Promise.resolve({})),
  logQso: vi.fn(() => Promise.resolve({})),
  getLog: vi.fn(() => Promise.resolve([])),
  lookupPark: vi.fn(() => Promise.resolve(null)),
  lookupParkLive: vi.fn(() => Promise.resolve(null)),
  qrzLookup: vi.fn(() => Promise.resolve(null)),
  resolveEntity: vi.fn(() => Promise.resolve(null)),
  searchParks: vi.fn(() => Promise.resolve([])),
  setCwPeerInfo: vi.fn(() => Promise.resolve()),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const snap = {
  radio: { band: '20m', dialMhz: 14.2 },
  hunt: null,
} as unknown as AppSnapshot

/** The Field Day session as the engine really serialises it — receive order, the
 *  composing exchange as a VECTOR, and the empty role of a symmetric contest. */
const fdStatus = (over: Partial<FieldDayStatus> = {}): FieldDayStatus =>
  ({
    running: true,
    state: 'Idle',
    qsoCount: 0,
    sections: 0,
    points: 0,
    log: [],
    role: '',
    receives: [
      { key: 'CLASS', kind: 'pattern', required: true },
      { key: 'SECTION', kind: 'enum', required: true, domain: 'fd_sections' },
    ],
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'WI', domain: 'fd_sections' },
    ],
    ...over,
  }) as unknown as FieldDayStatus

/** Exactly the props `PhoneCockpit` passes (`PhoneCockpit.tsx`, the LOG pane). */
function renderStrip(fieldDay: FieldDayStatus = fdStatus()) {
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

const box = (label: string) => screen.getByText(label).closest('label')!.querySelector('input')!
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
})

describe('1 — the boxes are the session’s received slots, in receive order', () => {
  it('renders Call plus one captioned box per received slot', () => {
    renderStrip()
    const caps = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].map((n) => n.textContent)
    expect(caps).toEqual(['Call', 'Class', 'Section'])
    // Each is a STACKED field: a caption and an input inside one label, which is what
    // makes the exchange readable to a logger sitting beside the operator.
    for (const l of document.querySelectorAll('.le-fd-big .le-fd-field')) {
      expect(l.querySelector('.le-fd-cap')).not.toBeNull()
      expect(l.querySelector('input')).not.toBeNull()
    }
  })

  it('follows the DTO rather than a hardcoded pair — five slots render five boxes', () => {
    // The Sweepstakes shape (serial, precedence, call, check, section): five received
    // slots, which is the ceiling §9 budgets for and the rules validator enforces.
    renderStrip(
      fdStatus({
        receives: [
          { key: 'NR', kind: 'serial', required: true },
          { key: 'PREC', kind: 'pattern', required: true },
          { key: 'CALL', kind: 'call', required: true },
          { key: 'CK', kind: 'pattern', required: true },
          { key: 'SEC', kind: 'enum', required: true, domain: 'ss_sections' },
        ],
      }),
    )
    const caps = [...document.querySelectorAll('.le-fd-big .le-fd-cap')].map((n) => n.textContent)
    // A slot with no catalog caption shows its SLOT ID — an invariant token, correctly
    // untranslated, rather than an invented English word.
    expect(caps).toEqual(['Call', 'NR', 'PREC', 'CALL', 'CK', 'SEC'])
  })
})

describe('2 — space walks Call → field₁ → … → Call', () => {
  it('advances through every box and loops back to Call', () => {
    renderStrip()
    const c = callBox()
    const cls = box('Class')
    const sec = box('Section')
    c.focus()
    fireEvent.keyDown(c, { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(cls)
    fireEvent.keyDown(cls, { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(sec)
    fireEvent.keyDown(sec, { key: ' ', code: 'Space' })
    expect(document.activeElement).toBe(c)
  })

  it('walks all five when the session receives five', () => {
    renderStrip(
      fdStatus({
        receives: [
          { key: 'NR', kind: 'serial', required: true },
          { key: 'PREC', kind: 'pattern', required: true },
          { key: 'CALL', kind: 'call', required: true },
          { key: 'CK', kind: 'pattern', required: true },
          { key: 'SEC', kind: 'enum', required: true, domain: 'ss_sections' },
        ],
      }),
    )
    const order = [callBox(), box('NR'), box('PREC'), box('CALL'), box('CK'), box('SEC')]
    for (let i = 0; i < order.length; i++) {
      order[i].focus()
      fireEvent.keyDown(order[i], { key: ' ', code: 'Space' })
      expect(document.activeElement).toBe(order[(i + 1) % order.length])
    }
  })

  it('does not let the space reach a value — a reflexive space must not log "3A "', () => {
    renderStrip()
    const cls = box('Class')
    fireEvent.change(cls, { target: { value: '3A' } })
    const e = fireEvent.keyDown(cls, { key: ' ', code: 'Space' })
    expect(e).toBe(false) // preventDefault() was called
    expect(cls.value).toBe('3A')
  })
})

describe('3 — the exchange persists across log-and-clear', () => {
  it('keeps the fields and clears the call, so a run does not re-type them', async () => {
    renderStrip()
    fireEvent.change(callBox(), { target: { value: 'K1ABC' } })
    fireEvent.change(box('Class'), { target: { value: '2A' } })
    fireEvent.change(box('Section'), { target: { value: 'EMA' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Log FD' }))
    })
    expect(api.fdLogManual).toHaveBeenCalledWith('K1ABC', '2A', 'EMA', 'PH', undefined)
    expect(callBox().value).toBe('')
    expect(box('Class').value).toBe('2A')
    expect(box('Section').value).toBe('EMA')
  })
})

describe('4 — the verdict slot is always present', () => {
  it('reserves its height whether or not a verdict is showing', () => {
    renderStrip()
    // Nothing typed: the slot exists and is empty. This is the RESERVATION — without
    // it the fields move up and down under the fingers that are typing.
    const slot = document.querySelector('.le-fd-verdicts')!
    expect(slot).not.toBeNull()
    expect(slot.children.length).toBe(0)
    fireEvent.change(callBox(), { target: { value: 'K1ABC' } })
    expect(document.querySelector('.le-fd-verdicts')!.children.length).toBe(1)
  })
})

describe('5 — the while-typing verdict costs zero IPC', () => {
  it('refuses a blank class and a bogus section without a single call', () => {
    renderStrip()
    fireEvent.change(callBox(), { target: { value: 'K1ABC' } })
    // ⚠️ THE CALLSIGN IS NOT PART OF THE CLAIM. Typing a call fires the entity resolve
    // and the debounced callbook lookup, and always has — the property under test is
    // that the EXCHANGE verdict is free, so the count starts from here.
    for (const f of Object.values(api)) f.mockClear()
    // A blank Section reads exactly as it always has, em dash and all.
    expect(screen.getByRole('alert').textContent).toBe(
      'Section "—" isn\'t a known ARRL/RAC section — required to log.',
    )
    // And a blank Class reads Field Day's own sentence.
    fireEvent.change(box('Class'), { target: { value: '' } })
    expect(screen.getByRole('alert').textContent).toBe('Enter their Field Day class to log.')
    fireEvent.change(box('Class'), { target: { value: '2A' } })
    fireEvent.change(box('Section'), { target: { value: 'ZZZ' } })
    expect(screen.getByRole('alert').textContent).toBe(
      'Section "ZZZ" isn\'t a known ARRL/RAC section — required to log.',
    )
    expect((screen.getByRole('button', { name: 'Log FD' }) as HTMLButtonElement).disabled).toBe(
      true,
    )
    // A real section clears it and arms the button.
    fireEvent.change(box('Section'), { target: { value: 'EMA' } })
    expect(screen.queryByRole('alert')).toBeNull()
    expect((screen.getByRole('button', { name: 'Log FD' }) as HTMLButtonElement).disabled).toBe(
      false,
    )
    // THE POINT: every verdict above came from data already in hand — the full own log
    // rides every snapshot, and the domain's value set is a static table.
    for (const [name, fn] of Object.entries(api)) {
      expect(fn, `${name} was called while typing the exchange`).not.toHaveBeenCalled()
    }
  })
})

describe('the sent side, read-only, with its one action', () => {
  it('shows what the session is composing, and the role only when it names one', () => {
    renderStrip()
    expect(document.querySelector('.le-fd-sent-val')!.textContent).toBe('3A WI')
    // Field Day is symmetric: one unconditional role, an empty id, and no chip.
    expect(document.querySelector('.le-fd-role')).toBeNull()
    cleanup()
    renderStrip(fdStatus({ role: 'in_state' }))
    expect(document.querySelector('.le-fd-role')!.textContent).toBe(' in_state')
  })

  it('"I moved" sends only the slots that changed, and takes effect on the next contact', async () => {
    renderStrip()
    fireEvent.click(screen.getByRole('button', { name: 'I moved' }))
    // The read-only value is replaced by one box per composing slot, pre-filled.
    const sent = document.querySelector('.le-fd-sent')!
    const sec = [...sent.querySelectorAll('input')].find(
      (i) => i.getAttribute('aria-label') === 'Section',
    )!
    expect(sec.value).toBe('WI')
    fireEvent.change(sec, { target: { value: 'IL' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    // CLASS did not change, so it is not sent — a move edits what moved.
    expect(api.contestIMoved).toHaveBeenCalledWith([['SECTION', 'IL']])
  })

  it('commits on Enter and backs out on Escape — the hands stay on the keyboard', async () => {
    renderStrip()
    fireEvent.click(screen.getByRole('button', { name: 'I moved' }))
    const sent = document.querySelector('.le-fd-sent')!
    const sec = [...sent.querySelectorAll('input')].find(
      (i) => i.getAttribute('aria-label') === 'Section',
    )!
    fireEvent.change(sec, { target: { value: 'IL' } })
    fireEvent.keyDown(sec, { key: 'Escape' })
    // Backed out: the read-only value is showing again and nothing was sent.
    expect(document.querySelector('.le-fd-sent-val')!.textContent).toBe('3A WI')
    expect(api.contestIMoved).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'I moved' }))
    const sec2 = [...document.querySelectorAll('.le-fd-sent input')].find(
      (i) => i.getAttribute('aria-label') === 'Section',
    )!
    await act(async () => {
      fireEvent.change(sec2, { target: { value: 'IL' } })
      fireEvent.keyDown(sec2, { key: 'Enter' })
    })
    expect(api.contestIMoved).toHaveBeenCalledWith([['SECTION', 'IL']])
  })

  it('sends nothing when nothing changed', async () => {
    renderStrip()
    fireEvent.click(screen.getByRole('button', { name: 'I moved' }))
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    })
    expect(api.contestIMoved).not.toHaveBeenCalled()
  })
})

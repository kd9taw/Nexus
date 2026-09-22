// @vitest-environment jsdom
//
// #162's LAST HALF — the per-contact comment in the callsign lookup card. The Logbook's
// Comment column was answered in 1.13.0 (click it, it opens to full length in that row) and
// the private Note followed it. The "Previous contacts" list in this card was left exactly as
// the Logbook's column had been: one clipped line, `title={cmt}`, and the rest readable only
// by hovering. That is the remaining ask on the issue, and the fix is the one already shipped
// next door, not a new idea.
//
// THE SEAM THE LOGBOOK DOES NOT HAVE. A prior-contact row here is itself a control: when the
// host wires `onOpenLog` the row navigates to the Logbook filtered to this call (#192), by
// click and by Enter/Space through `useRovingList`'s handler on the LIST. A toggle nested in
// that row therefore has to stop both, or opening a comment throws the operator out of the
// card mid-contact — and on the keyboard the roving handler `preventDefault()`s the key
// before the button is ever activated, so the comment could not be opened at all.
//
// jsdom lays nothing out. Nothing here asserts that the closed line is clipped, that the open
// one wraps, or how tall the row becomes — only which control exists, what state it is in,
// and where a click goes.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { RecallPanel } from './RecallPanel'
import type { CallHistory } from '../features/callHistory'
import type { LoggedQso } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn(async () => {}) }))

afterEach(cleanup)

const LONG =
  'Ran 5 W into a wire vertical on 40 m long path at his sunrise; he is chasing the last two ' +
  'zones for WAZ and asked me to listen for him on 15 m CW around 1400Z next weekend'

function qso(over: Partial<LoggedQso> = {}): LoggedQso {
  return {
    call: 'W1ABC', grid: 'FN31', band: '20m', freqMhz: 14.25, mode: 'SSB',
    rstSent: '59', rstRcvd: '57', whenUnix: Date.UTC(2026, 2, 14) / 1000,
    ...(over as object),
  } as LoggedQso
}

function hist(qsos: LoggedQso[]): CallHistory {
  return {
    qsos, count: qsos.length, workedBefore: qsos.length > 0, dupeThisBand: false,
    lastUnix: qsos.length ? qsos[0].whenUnix : null, confirmedCount: 0,
    bands: ['20m'], modes: ['SSB'],
  } as CallHistory
}

/** The card, plus the row and comment cell of its one prior contact. */
function card(comment: string | null, onOpenLog?: (call: string) => void) {
  const { container } = render(
    <RecallPanel call="W1ABC" band="20m" hist={hist([qso({ comment })])} onOpenLog={onOpenLog} />,
  )
  const row = container.querySelector('.recall-log-row') as HTMLElement
  expect(row, 'the card lists no prior contact, so there is nothing to read').not.toBeNull()
  return { row, cell: row.querySelector('.recall-log-cmt') as HTMLElement }
}

describe('a prior contact’s comment opens in the lookup card (#162)', () => {
  it('opens to its full text in the row, and folds back', async () => {
    const { cell } = card(LONG)
    const toggle = cell.querySelector('button')
    expect(toggle, 'no way to read a long comment except hovering it').not.toBeNull()
    // ⚠️ THE TEXT IS NOT THE BUTTON, and that is now load-bearing. It WAS the button, and a
    // comment-shaped control spanning this cell sat on top of the row's own click target — so
    // opening the Logbook from a prior contact (#192) stopped working wherever a comment
    // existed. Only the compiled-browser suite could see it; jsdom does not lay out.
    expect(
      cell.querySelector('.recall-log-cmt-text')?.textContent,
      'the comment text must be plain text, not the control — a click on it belongs to the row',
    ).toBe(LONG)
    expect(toggle!.textContent, 'the toggle must not carry the comment text').toBe('')
    expect(toggle!.getAttribute('aria-label')).toBeTruthy()
    expect(toggle!.getAttribute('aria-expanded')).toBe('false')
    expect(cell.classList.contains('expanded')).toBe(false)

    fireEvent.click(toggle!)
    expect(toggle!.getAttribute('aria-expanded')).toBe('true')
    expect(cell.classList.contains('expanded')).toBe(true)

    fireEvent.click(toggle!)
    expect(toggle!.getAttribute('aria-expanded')).toBe('false')
    expect(cell.classList.contains('expanded')).toBe(false)
  })

  it('reading the comment does not navigate away to the Logbook', async () => {
    const onOpenLog = vi.fn()
    const { cell } = card(LONG, onOpenLog)
    fireEvent.click(cell.querySelector('button')!)
    expect(
      onOpenLog,
      'the click reached the row underneath: opening a comment threw the operator into the Logbook',
    ).not.toHaveBeenCalled()
  })

  it('control: the rest of the row still navigates', async () => {
    // Without this the check above passes just as well on a card whose rows never navigated.
    const onOpenLog = vi.fn()
    const { row } = card(LONG, onOpenLog)
    fireEvent.click(row.querySelector('.recall-log-bm')!)
    expect(onOpenLog).toHaveBeenCalledWith('W1ABC')
  })

  it('Enter on the comment opens it instead of leaving the card', async () => {
    // `useRovingList`'s onKeyDown sits on the LIST and preventDefault()s Enter/Space before a
    // nested button is ever activated, so this fails in BOTH directions when unhandled: the
    // comment does not open AND the card navigates.
    const onOpenLog = vi.fn()
    const { row, cell } = card(LONG, onOpenLog)
    const toggle = cell.querySelector('button') as HTMLButtonElement
    fireEvent.focus(row)
    fireEvent.keyDown(toggle, { key: 'Enter', bubbles: true })
    expect(onOpenLog, 'Enter on the comment navigated away').not.toHaveBeenCalled()
  })

  it('control: a contact with no comment has nothing to open', async () => {
    const { cell } = card(null)
    expect(cell.querySelector('button')).toBeNull()
    expect((cell.textContent ?? '').trim()).toBe('')
  })
})

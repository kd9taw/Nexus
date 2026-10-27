// @vitest-environment jsdom
//
// THE LAST CONTACTS UNDER THE CONTEST ENTRY — the newest ten, newest first, and each one correctable
// where it stands: its Edit opens the contest screen's own edit (`ContestContactEdit`, the engine's
// `contest_edit`) in the row's place, with the same refusals (a club, a press through Remote) and the
// same line once it is over, the "edit it in the logbook too" warning included. That line stands
// outside the list's scroller, so it is on screen however far the list is scrolled, and goes at the
// next edit or contact logged. Save or Esc gives the keyboard back to the entry (`onEditDone`).
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, fireEvent, cleanup, act, screen } from '@testing-library/react'
import { createRef } from 'react'
import { LAST_CONTACTS, LastContacts } from './ContestLogger'
import type { FdClubStatus, FieldDayQso, FieldDayStatus } from '../types'
import { StreamInputDispatcher } from '../remote-native/stream-input'
import { t } from '../i18n'
import { contactLabel } from '../features/contestRemoval'
import { ContestContactEdit } from './ContestContactEdit'

vi.mock('../api', () => ({
  contestEdit: vi.fn(),
  resolveEntity: vi.fn(async () => null),
  contestLogManual: vi.fn(),
  fdLogManual: vi.fn(),
  logQso: vi.fn(),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const row = (call: string, zone: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'PH', submode: 'USB', whenUnix, rcvd: ['59', zone] }) as FieldDayQso

const cqww = (log: FieldDayQso[], club?: FdClubStatus): FieldDayStatus =>
  ({
    running: true,
    state: 'run',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'cqww_ssb',
    rulesYear: 2026,
    role: '',
    bands: ['80m', '40m', '20m', '15m', '10m'],
    receives: [
      { key: 'RST', kind: 'rst', required: true, adif: 'RST_RCVD' },
      { key: 'ZN', kind: 'number', required: true, min: 1, max: 40, adif: 'CQZ' },
    ],
    composing: [
      { key: 'RST', raw: '59' },
      { key: 'ZN', raw: '4' },
    ],
    club,
  }) as unknown as FieldDayStatus

/** Twelve contacts, oldest first, a minute apart: K00AA … K11AA. */
const LOG = Array.from({ length: 12 }, (_, i) => row(`K${String(i).padStart(2, '0')}AA`, '5', T0 + i * 60))

const CLUB = { syncState: 'synced', queued: 0, hosting: false, dupes: [], board: [] } as unknown as FdClubStatus

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

const onEditDone = vi.fn()
const list = (fieldDay: FieldDayStatus) => (
  <LastContacts fieldDay={fieldDay} style={{}} boxRef={createRef<HTMLElement>()} onEditDone={onEditDone} />
)
function show(fieldDay: FieldDayStatus) {
  return render(list(fieldDay))
}
/** `n` more contacts after LOG's twelve, a minute apart: N00NN … */
const more = (n: number) => Array.from({ length: n }, (_, i) => row(`N${String(i).padStart(2, '0')}NN`, '5', T0 + (12 + i) * 60))

const calls = () => [...document.querySelectorAll('.contest-logger-recent .fd-log-row .fd-col.call')].map((c) => c.textContent)
const editButtons = () => [...document.querySelectorAll<HTMLButtonElement>('.contest-logger-recent .fd-edit')]
const editor = () => document.querySelector<HTMLElement>('.contest-logger-recent .contest-edit')
const boxes = () => [...(editor()?.querySelectorAll<HTMLInputElement>('input') ?? [])]
const noteEl = () => document.querySelector<HTMLElement>('.contest-logger-recent-note')
const note = () => noteEl()?.textContent ?? null
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })

afterEach(() => {
  cleanup()
  onEditDone.mockClear()
  for (const f of Object.values(api)) f.mockClear()
})

describe('the last contacts list the newest ten', () => {
  it('shows the ten newest, newest first, of a longer log', () => {
    expect(LAST_CONTACTS).toBe(10)
    show(cqww(LOG))
    expect(calls()).toEqual(LOG.slice(-10).reverse().map((q) => q.call))
  })
})

describe('a last contact is corrected in place', () => {
  it('every row has an Edit named for its contact, and it opens that contact where it stands', () => {
    show(cqww(LOG))
    expect(editButtons()).toHaveLength(10)
    const third = screen.getByRole('button', { name: t('contest.edit.aria', { contact: contactLabel([LOG[9]]) }) })
    expect(third.getAttribute('aria-label')).toMatch(/^Edit K09AA · 20m USB · /)
    fireEvent.click(third)
    expect(editor()).not.toBeNull()
    expect(boxes().map((b) => b.value)).toEqual(['K09AA', '59', '5'])
    expect(document.activeElement).toBe(boxes()[0])
    expect(calls(), 'the row it edits is the edit now').not.toContain('K09AA')
    expect(calls()).toHaveLength(9)
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('Enter sends the edit through the engine, logs nothing, and says what came of it', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[11]],
      after: [row('K11AB', '14', LOG[11].whenUnix!)],
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'k11ab')
    type(boxes()[2], '14')
    await act(async () => {
      fireEvent.keyDown(boxes()[2], { key: 'Enter' })
    })
    await settle()
    expect(api.contestEdit).toHaveBeenCalledWith('K11AA', LOG[11].whenUnix, {
      call: 'K11AB',
      rows: [[['RST', '59'], ['ZN', '14']]],
      band: '20m',
      // Unchanged, as the contest screen's edit sends it.
      mode: '',
    }, [LOG[11]])
    expect(api.contestLogManual).not.toHaveBeenCalled()
    expect(api.logQso).not.toHaveBeenCalled()
    expect(editor()).toBeNull()
    expect(note()).toMatch(/^Edited K11AA: now K11AB/)
  })

  it('after the merge, says the logbook copy still has the old call', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[11]],
      after: [row('K11AB', '5', LOG[11].whenUnix!)],
      sentTo: { wsjtx: false, logbook: true, uploaded: [] },
    })
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'K11AB')
    await act(async () => {
      fireEvent.click(editor()!.querySelector('.contest-edit-save')!)
    })
    await settle()
    expect(note()).toContain(t('contest.edit.logbook', { call: 'K11AA' }))
  })

  it('Esc leaves it as it was and sends nothing', () => {
    show(cqww(LOG))
    fireEvent.click(editButtons()[1])
    type(boxes()[0], 'N0NE')
    fireEvent.keyDown(boxes()[0], { key: 'Escape' })
    expect(editor()).toBeNull()
    expect(calls()).toEqual(LOG.slice(-10).reverse().map((q) => q.call))
    expect(api.contestEdit).not.toHaveBeenCalled()
  })

  it('closes when its contact leaves the log meanwhile', async () => {
    const view = show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    expect(editor()).not.toBeNull()
    view.rerender(list(cqww(LOG.slice(0, 11))))
    await settle()
    expect(editor()).toBeNull()
  })

  it('keeps the contact it opened while new contacts push its row down, and out of the ten', async () => {
    api.contestEdit.mockResolvedValue({ outcome: 'refused', refusal: 'changed' })
    const view = show(cqww(LOG))
    // K09AA, third from the top.
    fireEvent.click(editButtons()[2])
    type(boxes()[0], 'K09AB')
    // Three logged meanwhile: its row is now sixth.
    view.rerender(list(cqww([...LOG, ...more(3)])))
    expect(boxes()[0].value, 'what was typed is still there').toBe('K09AB')
    expect(document.activeElement).toBe(boxes()[0])
    // Ten more: it is past the ten shown, and the edit is still open, after them.
    view.rerender(list(cqww([...LOG, ...more(10)])))
    expect(editor()).not.toBeNull()
    expect(boxes()[0].value).toBe('K09AB')
    expect(document.activeElement).toBe(boxes()[0])
    expect(calls()).toEqual(more(10).reverse().map((q) => q.call))
    expect(editor()!.compareDocumentPosition(document.querySelectorAll('.contest-logger-recent .fd-log-row')[9]) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy()
    await act(async () => {
      fireEvent.keyDown(boxes()[0], { key: 'Enter' })
    })
    await settle()
    // The contact it opened, by its call and time — never the one now in its old place.
    expect(api.contestEdit).toHaveBeenCalledTimes(1)
    expect(api.contestEdit.mock.calls[0].slice(0, 2)).toEqual(['K09AA', LOG[9].whenUnix])
    expect(api.contestEdit.mock.calls[0][2].call).toBe('K09AB')
  })

  it('gives the keyboard back to the entry once it is over, by Save and by Esc', async () => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[11]],
      after: [row('K11AB', '5', LOG[11].whenUnix!)],
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    expect(onEditDone).not.toHaveBeenCalled()
    await act(async () => {
      fireEvent.keyDown(boxes()[0], { key: 'Enter' })
    })
    await settle()
    expect(onEditDone).toHaveBeenCalledTimes(1)
    fireEvent.click(editButtons()[0])
    fireEvent.keyDown(boxes()[0], { key: 'Escape' })
    expect(onEditDone).toHaveBeenCalledTimes(2)
  })
})

describe('the line an edit leaves', () => {
  const edited = () =>
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[11]],
      after: [row('K11AB', '5', LOG[11].whenUnix!)],
      sentTo: { wsjtx: false, logbook: true, uploaded: [] },
    })
  const saveNewest = async () => {
    fireEvent.click(editButtons()[0])
    type(boxes()[0], 'K11AB')
    await act(async () => {
      fireEvent.keyDown(boxes()[0], { key: 'Enter' })
    })
    await settle()
  }

  it('is the last thing in the list’s box, after every row, held to its bottom edge so no scroll position hides it', async () => {
    edited()
    show(cqww(LOG))
    await saveNewest()
    expect(note()).toContain(t('contest.edit.logbook', { call: 'K11AA' }))
    // Its place on screen, in every window size, is the contest logger probe's to measure.
    const box = document.querySelector('.contest-logger-recent')!
    expect(box.lastElementChild, 'after every row, not among them').toBe(noteEl())
    expect(noteEl()!.closest('.contest-logger-recent-body'), 'not among the rows').toBeNull()
  })

  it('goes when a contact is logged, and stays while the log only changes otherwise', async () => {
    edited()
    const view = show(cqww(LOG))
    await saveNewest()
    const after = [...LOG.slice(0, 11), row('K11AB', '5', LOG[11].whenUnix!)]
    view.rerender(list(cqww(after)))
    expect(note(), 'the edit itself is not a new contact').not.toBeNull()
    view.rerender(list(cqww([...after, ...more(1)])))
    expect(note()).toBeNull()
  })

  it('goes at the next edit', () => {
    show(cqww(LOG, CLUB))
    fireEvent.click(editButtons()[0])
    expect(note()).toBe(t('contest.edit.club'))
    cleanup()
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    expect(note()).toBeNull()
  })
})

describe('one edit of a contact at a time', () => {
  it('another edit open in the window: Edit opens nothing, says so, and gives the keyboard back to it as typed', () => {
    render(
      <>
        <ContestContactEdit rows={[LOG[11]]} receives={cqww(LOG).receives ?? []} onDone={() => {}} />
        {list(cqww(LOG))}
      </>,
    )
    const other = document.querySelector<HTMLInputElement>('.contest-edit input')!
    type(other, 'K11AB')
    editButtons()[0].focus()
    fireEvent.click(editButtons()[0])
    expect(editor(), 'no second edit').toBeNull()
    expect(note()).toBe(t('contest.edit.open'))
    expect(document.activeElement).toBe(other)
    expect(other.value).toBe('K11AB')
  })
})

describe('the same refusals as the contest screen', () => {
  it('at a club says how a contact is fixed there and opens no edit; CONTROL: alone it opens', () => {
    show(cqww(LOG, CLUB))
    fireEvent.click(editButtons()[0])
    expect(editor()).toBeNull()
    expect(note()).toBe(t('contest.edit.club'))
    cleanup()
    show(cqww(LOG))
    fireEvent.click(editButtons()[0])
    expect(editor()).not.toBeNull()
  })

  describe('a press through the Remote stream edits nothing', () => {
    let under: Element | null = null
    let stream: StreamInputDispatcher | null = null
    beforeEach(() => {
      under = null
      Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
      stream = new StreamInputDispatcher(window)
    })
    afterEach(() => {
      stream?.dispose()
      stream = null
      delete (document as { elementFromPoint?: unknown }).elementFromPoint
    })
    const pointer = (action: 'down' | 'up') => ({
      type: 'pointer',
      action,
      x: 0.5,
      y: 0.5,
      button: 0,
      buttons: action === 'down' ? 1 : 0,
      modifiers: 0,
      pointerType: 'mouse',
      clicks: 1,
    })

    it('refuses Edit clicked through the stream, and says why; CONTROL: at the station it opens', async () => {
      show(cqww(LOG))
      under = editButtons()[0]
      await act(async () => {
        stream!.handle(pointer('down'))
        stream!.handle(pointer('up'))
      })
      expect(editor()).toBeNull()
      expect(note()).toBe(t('contest.edit.remote'))
      fireEvent.click(editButtons()[0])
      expect(editor()).not.toBeNull()
    })
  })

  it('a satellite contact keeps its band in the edit', () => {
    show(cqww([{ ...row('K00AA', '5', T0), sat: 'SO-50' } as FieldDayQso]))
    fireEvent.click(editButtons()[0])
    const [band] = [...editor()!.querySelectorAll<HTMLSelectElement>('select')]
    expect(band.disabled).toBe(true)
  })
})

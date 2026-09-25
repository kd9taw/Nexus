// @vitest-environment jsdom
//
// THE LOGBOOK CHANGES A CONTACT BY ITS ID AND THE EDIT KEY OF THE VERSION ON SCREEN (SPEC-2 v2 §3,
// §6; C16's commands). Each change — a QSL mark, a paper card, a satellite tag, a delete, the edit
// form — names its contact by `RowRef {id, editKey}`, both handed out with the page the row was
// drawn from. The engine makes the change only while the contact is still that version, and says
// what it did: applied / deleted, or — nothing written — `changed` (the contact changed since this
// window read it: another window, a sync) or `gone` (deleted meanwhile). The operator is told the
// last two in plain words, and the list (or the form) then shows the contact as it is.
//
// The form's edit is ONE change, its QSL-sent and paper-card marks included, where it was three
// commands. The old key-based commands must not be reached (`old`, below).

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Logbook } from './Logbook'
import { t } from '../i18n'
import { deleteQsoById, editQsoById, markQslCardById, markQslSentById, setSatTagById } from '../api'
import { pushToast } from '../toast'
import { createAskingLogSource } from '../features/askingLogSource'
import { answerFrom, type AnswerTo, type LogPage, type LogQuestion } from '../features/logAnswers'
import { setLogSource } from '../features/logSource'
import type { LoggedQso } from '../types'

/** The key-based commands the desktop sent before: never again from this view. */
const old = vi.hoisted(() => ({
  editQso: vi.fn(),
  markQslSent: vi.fn(),
  markQslCard: vi.fn(),
  setSatTag: vi.fn(),
  deleteQso: vi.fn(),
}))
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    ...old,
    editQsoById: vi.fn(),
    markQslSentById: vi.fn(),
    markQslCardById: vi.fn(),
    setSatTagById: vi.fn(),
    deleteQsoById: vi.fn(),
    exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    lotwSatNames: vi.fn(async () => ['AO-91', 'SO-50']),
    saveTextToDownloads: noop(), logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(),
    qrzPushQso: noop(), clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn((run: () => Promise<unknown>) => run()) }))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => true) }))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

const contact = (i: number, over: Partial<LoggedQso> = {}) =>
  ({
    id: `id-${i}`, call: `K${i}ABC`, grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
    rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
    country: 'United States', whenUnix: 1_700_000_000 + i * 60, confirmed: false, awardConfirmed: false,
    qslRcvd: null, qslSent: null, ota: null, ...over,
  }) as unknown as LoggedQso

/** The engine: the log as it stands, at a revision, and the edit key of each contact's version —
 *  here its id and the revision it was last changed at, as distinct as the engine's hashes. */
const engine = { log: [] as LoggedQso[], rev: 1, changedAt: new Map<string, number>() }
const keyOf = (id: string) => `ek:${id}@${engine.changedAt.get(id) ?? 1}`
/** Every page the window asked for, in order. */
const pagesAsked: number[] = []

beforeEach(() => {
  engine.log = [contact(0), contact(1), contact(2)]
  engine.rev = 1
  engine.changedAt.clear()
  pagesAsked.length = 0
  setLogSource(
    createAskingLogSource(async <Q extends LogQuestion>(q: Q) => {
      const a = answerFrom(engine.log, q, engine.rev) as AnswerTo<Q>
      if (q.kind !== 'page') return a
      pagesAsked.push(engine.rev)
      const page = a as LogPage
      return { ...page, editKeys: page.keys.map(keyOf) } as AnswerTo<Q>
    }),
  )
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

/** The newest-first list: K2ABC is the top row. */
async function renderLogbook() {
  const view = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1} />)
  await screen.findByRole('button', { name: t('logbook.row.edit', { call: 'K2ABC' }) })
  return view
}
const qslMenu = (call: string) => screen.getByRole('combobox', { name: t('logbook.row.qslSent.aria', { call }) })
const satMenu = (call: string) => screen.getByRole('combobox', { name: t('logbook.row.sat.aria', { call }) })
const K2 = { id: 'id-2', editKey: 'ek:id-2@1' }
/** A contact as the engine now holds it, answered with its key. */
const now = (row: LoggedQso, at: number) => {
  engine.changedAt.set(row.id!, at)
  return { row, editKey: keyOf(row.id!) }
}
const toasts = () => vi.mocked(pushToast).mock.calls.map((c) => c[0])

describe('each change names its contact by id and the edit key on screen', () => {
  it('FIX: a QSL mark, sent and card', async () => {
    vi.mocked(markQslSentById).mockResolvedValue({ kind: 'applied', current: now(contact(2), 1) })
    vi.mocked(markQslCardById).mockResolvedValue({ kind: 'applied', current: now(contact(2), 1) })
    await renderLogbook()
    fireEvent.change(qslMenu('K2ABC'), { target: { value: 'B' } })
    await waitFor(() => expect(markQslSentById).toHaveBeenCalledWith(K2, 'B'))
    await waitFor(() => expect(toasts()).toContain(t('logbook.qsl.marked', { call: 'K2ABC', via: t('logbook.qsl.via.bureau') })))
    fireEvent.change(qslMenu('K2ABC'), { target: { value: 'R' } })
    await waitFor(() => expect(markQslCardById).toHaveBeenCalledWith(K2, true))
    expect(old.markQslSent).not.toHaveBeenCalled()
    expect(old.markQslCard).not.toHaveBeenCalled()
  })

  it('FIX: the satellite tag', async () => {
    vi.mocked(setSatTagById).mockResolvedValue({ kind: 'applied', current: now(contact(2), 1) })
    await renderLogbook()
    fireEvent.change(satMenu('K2ABC'), { target: { value: 'AO-91' } })
    await waitFor(() => expect(setSatTagById).toHaveBeenCalledWith(K2, 'AO-91'))
    await waitFor(() => expect(toasts()).toContain(t('logbook.sat.tagged', { call: 'K2ABC', sat: 'AO-91' })))
    expect(old.setSatTag).not.toHaveBeenCalled()
  })

  it('FIX: a delete', async () => {
    vi.mocked(deleteQsoById).mockResolvedValue({ kind: 'deleted' })
    await renderLogbook()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.row.delete', { call: 'K1ABC' }) }))
    await waitFor(() => expect(deleteQsoById).toHaveBeenCalledWith({ id: 'id-1', editKey: 'ek:id-1@1' }))
    await waitFor(() => expect(toasts()).toContain(t('logbook.delete.done', { call: 'K1ABC' })))
    expect(old.deleteQso).not.toHaveBeenCalled()
  })

  it('FIX: the edit form saves ONE change by id, its QSL marks in it', async () => {
    vi.mocked(editQsoById).mockResolvedValue({ kind: 'applied', current: { row: contact(2, { comment: 'Nice signal' }), editKey: 'ek:id-2@2' } })
    await renderLogbook()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'K2ABC' }) }))
    fireEvent.change(await screen.findByPlaceholderText(t('logbook.field.comment.placeholder')), { target: { value: 'Nice signal' } })
    fireEvent.change(screen.getByLabelText(t('logbook.field.qslSent.label')), { target: { value: 'D' } })
    fireEvent.click(screen.getByLabelText(t('logbook.field.qslCard.label')))
    fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
    await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(1))
    const [target, edit] = vi.mocked(editQsoById).mock.calls[0]
    expect(target).toEqual(K2)
    // Exactly the form's fields, as the form has always filled them — and the two marks.
    expect(edit).toEqual({
      call: 'K2ABC', grid: 'EN37', state: null, band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: 'Nice signal', notes: null,
      txPower: null, whenUnix: 1_700_000_120, timeOffUnix: null,
      ota: { myProgram: null, myRef: null, theirProgram: null, theirRef: null },
      myGrid: null, myRig: null, qslSentVia: 'D', qslCard: true,
    })
    await waitFor(() => expect(toasts()).toContain(t('logbook.form.updated', { call: 'K2ABC' })))
    expect(old.editQso).not.toHaveBeenCalled()
    expect(old.markQslSent).not.toHaveBeenCalled()
    expect(old.markQslCard).not.toHaveBeenCalled()
  })

  it('a change after the list was read again sends the key of the version now on screen', async () => {
    await renderLogbook()
    // Another window marks a card on K2ABC: the list is read again (this window's own refresh).
    vi.mocked(markQslCardById).mockImplementationOnce(async () => {
      engine.log = engine.log.map((r) => (r.id === 'id-2' ? { ...r, qslRcvd: { card: true } } as unknown as LoggedQso : r))
      engine.rev = 2
      return { kind: 'applied', current: now(engine.log[2], 2) }
    })
    fireEvent.change(qslMenu('K2ABC'), { target: { value: 'R' } })
    await waitFor(() => expect(pagesAsked).toContain(2))
    vi.mocked(markQslSentById).mockResolvedValue({ kind: 'applied', current: now(engine.log[2], 2) })
    await waitFor(() => {
      fireEvent.change(qslMenu('K2ABC'), { target: { value: 'E' } })
      expect(markQslSentById).toHaveBeenCalled()
    })
    const calls = vi.mocked(markQslSentById).mock.calls
    expect(calls[calls.length - 1]).toEqual([{ id: 'id-2', editKey: 'ek:id-2@2' }, 'E'])
  })
})

describe('a change the engine refuses is said plainly, and the contact shown as it is', () => {
  it('changed: a row menu change', async () => {
    await renderLogbook()
    const asked = pagesAsked.length
    vi.mocked(markQslSentById).mockResolvedValue({ kind: 'changed', current: now(contact(2, { call: 'K2ABD' }), 2) })
    fireEvent.change(qslMenu('K2ABC'), { target: { value: 'B' } })
    await waitFor(() => expect(toasts()).toContain(t('logbook.change.changed', { call: 'K2ABC' })))
    expect(toasts(), 'a change that was not made was reported as made').not.toContain(
      t('logbook.qsl.marked', { call: 'K2ABC', via: t('logbook.qsl.via.bureau') }),
    )
    await waitFor(() => expect(pagesAsked.length, 'the list was not read again').toBeGreaterThan(asked))
  })

  it('gone: a row menu change', async () => {
    await renderLogbook()
    vi.mocked(setSatTagById).mockResolvedValue({ kind: 'gone' })
    fireEvent.change(satMenu('K1ABC'), { target: { value: 'SO-50' } })
    await waitFor(() => expect(toasts()).toContain(t('logbook.change.gone', { call: 'K1ABC' })))
    expect(toasts()).not.toContain(t('logbook.sat.tagged', { call: 'K1ABC', sat: 'SO-50' }))
  })

  it('changed: the edit form shows the contact as it is now, and saves against that version', async () => {
    await renderLogbook()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'K2ABC' }) }))
    const comment = await screen.findByPlaceholderText(t('logbook.field.comment.placeholder'))
    fireEvent.change(comment, { target: { value: 'Mine' } })
    vi.mocked(editQsoById).mockResolvedValueOnce({ kind: 'changed', current: now(contact(2, { comment: 'Theirs' }), 3) })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
    await waitFor(() => expect(toasts()).toContain(t('logbook.change.changed', { call: 'K2ABC' })))
    expect(toasts()).not.toContain(t('logbook.form.updated', { call: 'K2ABC' }))
    // Here it is now: the form holds the contact as the other window left it.
    await waitFor(() => expect((screen.getByPlaceholderText(t('logbook.field.comment.placeholder')) as HTMLInputElement).value).toBe('Theirs'))
    vi.mocked(editQsoById).mockResolvedValueOnce({ kind: 'applied', current: now(contact(2, { comment: 'Theirs' }), 4) })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
    await waitFor(() => expect(editQsoById).toHaveBeenCalledTimes(2))
    expect(vi.mocked(editQsoById).mock.calls[1][0]).toEqual({ id: 'id-2', editKey: 'ek:id-2@3' })
  })

  it('gone: the edit form closes', async () => {
    await renderLogbook()
    fireEvent.click(screen.getByRole('button', { name: t('logbook.row.edit', { call: 'K2ABC' }) }))
    await screen.findByPlaceholderText(t('logbook.field.comment.placeholder'))
    vi.mocked(editQsoById).mockResolvedValue({ kind: 'gone' })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.form.save') }))
    await waitFor(() => expect(toasts()).toContain(t('logbook.change.gone', { call: 'K2ABC' })))
    await waitFor(() => expect(screen.queryByRole('button', { name: t('logbook.form.save') })).toBeNull())
  })

  it('a delete: changed is not a delete; gone is', async () => {
    await renderLogbook()
    vi.mocked(deleteQsoById).mockResolvedValueOnce({ kind: 'changed', current: now(contact(1), 2) })
    fireEvent.click(screen.getByRole('button', { name: t('logbook.row.delete', { call: 'K1ABC' }) }))
    await waitFor(() => expect(toasts()).toContain(t('logbook.delete.changed', { call: 'K1ABC' })))
    expect(toasts()).not.toContain(t('logbook.delete.done', { call: 'K1ABC' }))
    vi.mocked(deleteQsoById).mockResolvedValueOnce({ kind: 'gone' })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: t('logbook.row.delete', { call: 'K0ABC' }) }))
    })
    await waitFor(() => expect(toasts()).toContain(t('logbook.delete.gone', { call: 'K0ABC' })))
    expect(toasts()).not.toContain(t('logbook.delete.done', { call: 'K0ABC' }))
  })
})

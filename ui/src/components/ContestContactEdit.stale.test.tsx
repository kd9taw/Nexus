// @vitest-environment jsdom
//
// ⛔ AN EDIT FILLED FROM WHAT A CONTACT WAS NEVER SAVES OVER WHAT IT IS NOW. The logger window and
// the main window's contest screen can each have an edit of one contact open (each window keeps its
// own one-edit-at-a-time rule). The first saves; the second, whose form still holds the old values,
// closes and says so, and sends nothing. A save pressed before the change reaches this window sends
// the rows its edit opened from, for the engine to refuse. This edit's own save, under way while the
// change it made arrives, answers for itself.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, fireEvent, cleanup, act, within } from '@testing-library/react'
import { createRef } from 'react'
import { ContestView } from './ContestView'
import { LastContacts } from './ContestLogger'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayQso, FieldDayStatus } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  contestRemoveLast: vi.fn(),
  contestRestore: vi.fn(),
  contestEdit: vi.fn(),
  resolveEntity: vi.fn(async () => null),
  contestLogManual: vi.fn(),
  fdLogManual: vi.fn(),
  logQso: vi.fn(),
}))
const api = (await import('../api')) as unknown as Record<string, ReturnType<typeof vi.fn>>

const T0 = 1_792_342_800 // 2026-10-18 17:00Z

const row = (call: string, qth: string, whenUnix: number): FieldDayQso =>
  ({ call, class: '', section: '', band: '20m', mode: 'CW', submode: '', whenUnix, rcvd: ['599', qth] }) as FieldDayQso

const party = (log: FieldDayQso[]): FieldDayStatus =>
  ({
    running: true,
    state: 'sp',
    qsoCount: log.length,
    sections: 0,
    points: 0,
    log,
    event: 'ilqp',
    rulesYear: 2026,
    role: 'in_state',
    bands: ['80m', '40m', '20m'],
    receives: [
      { key: 'RST', kind: 'rst', required: true },
      { key: 'QTH', kind: 'enum', required: true, domain: 'il_counties', domains: ['il_counties'] },
    ],
    composing: [
      { key: 'RST', raw: '599' },
      { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
    ],
  }) as unknown as FieldDayStatus

const LOG = [row('K9AAA', 'COOK', T0), row('W9BBB', 'LAKE', T0 + 60)]
/** The log once the logger window's edit of K9AAA (Cook → DuPage) is in. */
const EDITED = [row('K9AAA', 'DUPG', T0), LOG[1]]
/** …and once a call fix of K9AAA (to K9AAC) is in, the commonest contest edit. */
const CALLED = [row('K9AAC', 'COOK', T0), LOG[1]]
const CHANGED = 'That contact changed in the contest log since its edit opened, so nothing was edited. To edit it as it is now, open it again.'
const GONE = 'That contact is no longer in the contest log, so nothing was edited.'

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

/** Both windows on one log: the logger window's last contacts (another window, so not on this
 *  one's screen: `hidden`) and the main window's contest screen. */
const both = (log: FieldDayQso[]) => (
  <>
    <div data-testid="logger" hidden>
      <LastContacts fieldDay={party(log)} style={{}} boxRef={createRef<HTMLElement>()} onEditDone={() => {}} />
    </div>
    <div data-testid="main">
      <ContestView fieldDay={party(log)} onSetMode={() => {}} fdActive tier="FT8" />
    </div>
  </>
)
const win = (id: 'logger' | 'main') => within(document.querySelector<HTMLElement>(`[data-testid="${id}"]`)!)
const editOf = (id: 'logger' | 'main', call: string) =>
  win(id).getByRole('button', { name: new RegExp(`^${t('contest.edit.aria', { contact: call })}`), hidden: true })
const editor = (id: 'logger' | 'main') =>
  document.querySelector<HTMLElement>(`[data-testid="${id}"] .contest-edit`)
const boxes = (id: 'logger' | 'main') => [...(editor(id)?.querySelectorAll<HTMLInputElement>('input') ?? [])]
const mainNote = () => document.querySelector('[data-testid="main"] .fd-removed-note')?.textContent ?? null
const loggerNote = () => document.querySelector('[data-testid="logger"] .contest-logger-recent-note')?.textContent ?? null
const note = (id: 'logger' | 'main') => (id === 'main' ? mainNote() : loggerNote())
const type = (el: HTMLInputElement, value: string) => fireEvent.change(el, { target: { value } })
const enter = async (el: HTMLInputElement) => {
  await act(async () => {
    fireEvent.keyDown(el, { key: 'Enter' })
  })
  await settle()
}
const edited = (after: FieldDayQso[]) =>
  api.contestEdit.mockResolvedValueOnce({
    outcome: 'edited',
    before: [LOG[0]],
    after,
    sentTo: { wsjtx: false, logbook: false, uploaded: [] },
  })

/** Both windows with an edit of K9AAA open. */
async function bothOpen() {
  const view = render(both(LOG))
  await settle()
  fireEvent.click(editOf('logger', 'K9AAA'))
  fireEvent.click(editOf('main', 'K9AAA'))
  expect(boxes('logger').map((b) => b.value), 'premise: the logger window edits K9AAA').toEqual(['K9AAA', '599', 'COOK'])
  expect(boxes('main').map((b) => b.value), 'premise: so does the main window').toEqual(['K9AAA', '599', 'COOK'])
  return view
}

afterEach(() => {
  cleanup()
  for (const f of Object.values(api)) f.mockClear()
  api.contestEdit.mockReset()
})

describe('an edit of a contact changed in another window', () => {
  it('closes, says so, and sends nothing once the change arrives', async () => {
    edited([EDITED[0]])
    const view = await bothOpen()
    type(boxes('logger')[2], 'DUPG')
    await enter(boxes('logger')[2])
    expect(api.contestEdit).toHaveBeenCalledTimes(1)
    // The engine's next snapshot, which both windows get.
    view.rerender(both(EDITED))
    await settle()
    expect(editor('main'), 'the old form is gone').toBeNull()
    expect(mainNote()).toBe(CHANGED)
    expect(api.contestEdit, 'the main window sent nothing').toHaveBeenCalledTimes(1)
  })

  it('sends, when saved before the change arrives, the rows it opened from — for the engine to refuse', async () => {
    edited([EDITED[0]])
    await bothOpen()
    type(boxes('logger')[2], 'DUPG')
    await enter(boxes('logger')[2])
    api.contestEdit.mockResolvedValueOnce({ outcome: 'refused', refusal: 'changed' })
    type(boxes('main')[0], 'K9AAB')
    await enter(boxes('main')[0])
    expect(api.contestEdit).toHaveBeenCalledTimes(2)
    expect(api.contestEdit.mock.calls[1][3], 'what the form was filled from').toEqual([LOG[0]])
    expect(mainNote()).toBe(CHANGED)
  })

  it('answers for its own save: the change it made arriving first is not "changed elsewhere"', async () => {
    let answer: (a: unknown) => void = () => {}
    api.contestEdit.mockReturnValueOnce(new Promise((r) => (answer = r)))
    const view = render(both(LOG))
    await settle()
    fireEvent.click(editOf('main', 'K9AAA'))
    type(boxes('main')[2], 'DUPG')
    await enter(boxes('main')[2])
    view.rerender(both(EDITED))
    await settle()
    expect(mainNote()).toBeNull()
    await act(async () => {
      answer({ outcome: 'edited', before: [LOG[0]], after: [EDITED[0]], sentTo: { wsjtx: false, logbook: false, uploaded: [] } })
    })
    await settle()
    expect(editor('main')).toBeNull()
    expect(mainNote()).toMatch(/^Edited K9AAA: now K9AAA /)
  })

  // The host finds the edit's contact by the call and time it opened with, so a call fix leaves it
  // nothing to show: it closes the edit, and must say why.
  it.each([
    ['main', 'logger'],
    ['logger', 'main'],
  ] as const)('a call fix in the %s window closes the %s window’s edit and says the contact changed', async (fixer, other) => {
    api.contestEdit.mockResolvedValueOnce({
      outcome: 'edited',
      before: [LOG[0]],
      after: [CALLED[0]],
      sentTo: { wsjtx: false, logbook: false, uploaded: [] },
    })
    const view = await bothOpen()
    type(boxes(fixer)[0], 'K9AAC')
    await enter(boxes(fixer)[0])
    expect(api.contestEdit).toHaveBeenCalledTimes(1)
    expect(note(fixer), 'premise: the fixing window says what it did').toMatch(/^Edited K9AAA: now K9AAC /)
    view.rerender(both(CALLED))
    await settle()
    expect(editor(other), 'the old form is gone').toBeNull()
    expect(note(other)).toBe(CHANGED)
    expect(note(fixer), 'the fixing window keeps its own line').toMatch(/^Edited K9AAA: now K9AAC /)
    expect(api.contestEdit, 'the other window sent nothing').toHaveBeenCalledTimes(1)
  })

  // CONTROL: this edit's own call fix, its answer still on the way when the change arrives, is not
  // "changed elsewhere": the answer says what it did.
  it.each(['main', 'logger'] as const)('the %s window’s own call fix answers for itself', async (id) => {
    let answer: (a: unknown) => void = () => {}
    api.contestEdit.mockReturnValueOnce(new Promise((r) => (answer = r)))
    const view = render(both(LOG))
    await settle()
    fireEvent.click(editOf(id, 'K9AAA'))
    type(boxes(id)[0], 'K9AAC')
    await enter(boxes(id)[0])
    view.rerender(both(CALLED))
    await settle()
    expect(editor(id), 'the host has nothing to show it on').toBeNull()
    expect(note(id), 'no "changed" while its own answer is on the way').toBeNull()
    await act(async () => {
      answer({ outcome: 'edited', before: [LOG[0]], after: [CALLED[0]], sentTo: { wsjtx: false, logbook: false, uploaded: [] } })
    })
    await settle()
    expect(note(id)).toMatch(/^Edited K9AAA: now K9AAC /)
  })

  it('removed elsewhere, the edit closes and says the contact is no longer in the log', async () => {
    const view = await bothOpen()
    view.rerender(both([LOG[1]]))
    await settle()
    expect([editor('main'), editor('logger')]).toEqual([null, null])
    expect([mainNote(), loggerNote()]).toEqual([GONE, GONE])
  })

  it('a save the engine refuses as gone says the contact is no longer in the log', async () => {
    api.contestEdit.mockResolvedValueOnce({ outcome: 'refused', refusal: 'gone' })
    await bothOpen()
    type(boxes('main')[2], 'DUPG')
    await enter(boxes('main')[2])
    expect(mainNote()).toBe(GONE)
  })
})

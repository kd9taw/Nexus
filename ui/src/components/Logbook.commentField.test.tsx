// @vitest-environment jsdom
//
// THE SHARED COMMENT IN THE LOGBOOK'S EDIT FORM: a multi-line box of its own, full width, directly
// above Notes (operator, 1.21.0: it was a 140 px grid cell among the short fields). It holds ONE line
// of data, as ADIF's COMMENT does: a typed or pasted line break lands as a space and Enter saves, as
// it did from the old box. A comment stored with line breaks (an import keeps what its file had) is
// shown as it is and saved untouched when another field is edited; the old one-line box dropped its
// breaks on every save, gluing the words together.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, fireEvent, cleanup, within } from '@testing-library/react'
import { Logbook } from './Logbook'
import * as api from '../api'
import type { LogQuestion } from '../features/logAnswers'

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('../api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
    deleteQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    editQsoById: vi.fn(() => Promise.resolve({ kind: 'applied' })),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // Empty list => no satellite picker rendered, so this suite's DOM is unchanged.
    lotwSatNames: vi.fn(async () => [] as string[]), setSatTagById: vi.fn(async () => ({})),
    logQso: noop(), purgeLog: noop(), qrzLookup: noop(),
    markQslSentById: noop(),
    markQslCardById: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(), wrlPushQso: noop(),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((run: () => Promise<unknown>) => run()),
}))

function oneContact(over: Record<string, unknown> = {}) {
  return [
    {
      id: 'id-K0ABC', call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000,
      confirmed: false, awardConfirmed: false,
      qslRcvd: null, qslSent: null, ota: null, upload: undefined,
      myGrid: 'EN52XA', myRig: 'IC-705',
      ...over,
    },
  ]
}

async function renderLog(over: Record<string, unknown> = {}) {
  engineLog.mockResolvedValue(oneContact(over))
  const utils = render(<Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />)
  await waitFor(() => expect(utils.container.querySelector('.logbook-row:not(.head):not(.placeholder)')).not.toBeNull())
  return utils
}

async function openEdit(container: HTMLElement) {
  fireEvent.click(container.querySelector('button[aria-label="Edit K0ABC"]') as HTMLButtonElement)
  await waitFor(() => expect(container.querySelector('.logbook-form')).not.toBeNull())
  return container.querySelector('.logbook-form') as HTMLElement
}

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})


const commentBox = (form: HTMLElement) => within(form).getByLabelText('Comment') as HTMLTextAreaElement
const notesBox = (form: HTMLElement) => within(form).getByLabelText('Notes') as HTMLTextAreaElement
const editQsoById = () => api.editQsoById as ReturnType<typeof vi.fn>

describe('the shared comment in the edit form', () => {
  it('is a full-width multi-line box directly above Notes', async () => {
    const { container } = await renderLog({ comment: 'tnx QSO' })
    const form = await openEdit(container)
    const comment = commentBox(form)
    expect(comment.tagName, 'the comment is still a one-line input').toBe('TEXTAREA')
    expect(comment.value).toBe('tnx QSO')
    const field = comment.closest('.logbook-field')!
    expect(field.classList.contains('logbook-field-wide'), 'the comment does not span the form').toBe(true)
    expect(field.nextElementSibling, 'the comment is not directly above Notes').toBe(notesBox(form).closest('.logbook-field'))
  })

  it('saves a stored multi-line comment untouched when another field is edited', async () => {
    const { container } = await renderLog({ comment: 'line one\nline two' })
    const form = await openEdit(container)
    expect(commentBox(form).value).toBe('line one\nline two')
    fireEvent.change(within(form).getByLabelText('Rig'), { target: { value: 'FT-991A' } })
    fireEvent.click(within(form).getByRole('button', { name: /save/i }))
    await waitFor(() => expect(editQsoById()).toHaveBeenCalled())
    expect(editQsoById().mock.calls[0][1].comment).toBe('line one\nline two')
  })

  it('a typed line break lands as a space, and Enter saves', async () => {
    const { container } = await renderLog()
    const form = await openEdit(container)
    fireEvent.change(commentBox(form), { target: { value: 'tnx for\nthe QSO' } })
    expect(commentBox(form).value).toBe('tnx for the QSO')
    expect(fireEvent.keyDown(commentBox(form), { key: 'Enter' }), 'Enter would type a line break').toBe(false)
    await waitFor(() => expect(editQsoById()).toHaveBeenCalled())
    expect(editQsoById().mock.calls[0][1].comment).toBe('tnx for the QSO')
  })
})

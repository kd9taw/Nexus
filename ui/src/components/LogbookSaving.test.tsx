// @vitest-environment jsdom
//
// The quit, when the logbook still has changes on their way to disk (SPEC-1's C10). The station
// keeps the main window open, says what it is doing, and asks the operator only when it cannot
// finish: after a minute, or when the logbook refused a change. These tests drive the dialog the
// way the station does — through the three events it emits — and read back what the operator
// would see and what the buttons send.
//
// The words are asserted twice on purpose: once literally, for the spec's own wording ("Saving
// your logbook…", "Keep trying", "Quit without the last N changes"), and otherwise through the
// catalog, so a reworded sentence does not read as a broken dialog.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import {
  LogbookSaving,
  LOGBOOK_SAVING,
  LOGBOOK_SAVE_FAILED,
  LOGBOOK_SAVE_DONE,
  SAVING_GRACE_MS,
} from './LogbookSaving'
import { t } from '../i18n'

type Handler = (e: { payload: unknown }) => void
let handlers: Record<string, Handler>
let invoked: Array<[string, unknown]>

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  handlers = {}
  invoked = []
  ;(window as unknown as { __TAURI__?: unknown }).__TAURI__ = {
    event: {
      listen: async (name: string, h: Handler) => {
        handlers[name] = h
        return () => {
          delete handlers[name]
        }
      },
    },
    core: {
      invoke: (cmd: string, args?: unknown) => {
        invoked.push([cmd, args])
        return Promise.resolve(undefined)
      },
    },
  }
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
})

/** Mount the host and let its three listeners register. */
async function mount() {
  render(<LogbookSaving />)
  await act(async () => {})
  expect(Object.keys(handlers).sort(), 'the host listens for all three events').toEqual(
    [LOGBOOK_SAVE_DONE, LOGBOOK_SAVE_FAILED, LOGBOOK_SAVING].sort(),
  )
}

/** Deliver one event the way the station emits it. */
function emit(name: string, payload: unknown) {
  act(() => handlers[name]!({ payload }))
}

function advance(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms)
  })
}

const dialog = () => screen.queryByRole('dialog')
const button = (name: string) => screen.queryByRole('button', { name })

describe('the logbook-saving dialog', () => {
  it('shows nothing for a save that finishes inside the grace — a quick quit stays a quick quit', async () => {
    await mount()
    emit(LOGBOOK_SAVING, { pending: 2, radioLive: false })
    advance(SAVING_GRACE_MS - 50)
    expect(dialog()).toBeNull()
    emit(LOGBOOK_SAVE_DONE, { saved: true })
    advance(2000)
    expect(dialog(), 'the grace timer died with the save').toBeNull()
  })

  it('shows the saving line after the grace, with the live count, and progress never restarts the grace', async () => {
    await mount()
    emit(LOGBOOK_SAVING, { pending: 3, radioLive: false })
    advance(250)
    // The station reports progress every 250 ms. If each report restarted a 300 ms grace, the
    // line would never appear on a slow disk — exactly the case it exists for.
    emit(LOGBOOK_SAVING, { pending: 2, radioLive: false })
    advance(SAVING_GRACE_MS - 250 - 1)
    expect(dialog(), 'still inside the grace').toBeNull()
    advance(1)
    expect(dialog()).not.toBeNull()
    expect(screen.getByText('Saving your logbook…')).toBeTruthy()
    expect(screen.getByText(t('quit.logbook.saving.pending', { count: 2 }))).toBeTruthy()
    expect(screen.getByText('2 changes still to write')).toBeTruthy()
    // …and it counts down as the changes land.
    emit(LOGBOOK_SAVING, { pending: 1, radioLive: false })
    expect(screen.getByText('1 change still to write')).toBeTruthy()
  })

  it('names no count once only log.adi is left to write', async () => {
    await mount()
    emit(LOGBOOK_SAVING, { pending: 0, radioLive: false })
    advance(SAVING_GRACE_MS)
    expect(screen.getByText('Saving your logbook…')).toBeTruthy()
    expect(dialog()!.textContent).not.toMatch(/change/)
  })

  it('a slow save offers both choices, and Keep trying goes back to the line at once', async () => {
    await mount()
    emit(LOGBOOK_SAVING, { pending: 2, radioLive: false })
    advance(SAVING_GRACE_MS)
    emit(LOGBOOK_SAVE_FAILED, { pending: 2, refused: 0, reason: null, radioLive: false })
    expect(screen.getByText(t('quit.logbook.slow.title'))).toBeTruthy()
    expect(screen.getByText(t('quit.logbook.slow.body', { count: 2 }))).toBeTruthy()
    const keep = button('Keep trying')
    const quit = button('Quit without the last 2 changes')
    expect(keep, 'Keep trying is offered while changes are still on their way').not.toBeNull()
    expect(quit).not.toBeNull()

    fireEvent.click(keep!)
    await act(async () => {})
    expect(invoked).toContainEqual(['logbook_save_choice', { keepTrying: true }])
    expect((button('Keep trying') as HTMLButtonElement).disabled, 'one answer per question').toBe(true)

    emit(LOGBOOK_SAVING, { pending: 1, radioLive: false })
    expect(screen.getByText('Saving your logbook…'), 'no second grace after an answer').toBeTruthy()
    expect(screen.getByText('1 change still to write')).toBeTruthy()
  })

  it('Quit without sends that answer', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, { pending: 3, refused: 0, reason: null, radioLive: false })
    fireEvent.click(button('Quit without the last 3 changes')!)
    await act(async () => {})
    expect(invoked).toContainEqual(['logbook_save_choice', { keepTrying: false }])
    expect(invoked.some(([, a]) => (a as { keepTrying?: boolean })?.keepTrying === true)).toBe(false)
  })

  it('a refused change offers only the quit — waiting cannot save it — and names the reason', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, {
      pending: 0,
      refused: 1,
      reason: 'database or disk is full',
      radioLive: false,
    })
    expect(screen.getByText(t('quit.logbook.refused.title'))).toBeTruthy()
    expect(
      screen.getByText(t('quit.logbook.refused.body', { count: 1, reason: 'database or disk is full' })),
    ).toBeTruthy()
    expect(dialog()!.textContent).toContain('database or disk is full')
    expect(button('Keep trying'), 'nothing is left for more waiting to finish').toBeNull()
    expect(button('Quit without the last change')).not.toBeNull()
  })

  it('counts refused and still-pending changes together in the quit, and says both', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, { pending: 2, refused: 1, reason: 'disk I/O error', radioLive: false })
    expect(screen.getByText(t('quit.logbook.refused.body', { count: 1, reason: 'disk I/O error' }))).toBeTruthy()
    expect(screen.getByText(t('quit.logbook.slow.body', { count: 2 }))).toBeTruthy()
    expect(button('Keep trying')).not.toBeNull()
    expect(button('Quit without the last 3 changes')).not.toBeNull()
  })

  it('a change refused for a reason that can pass offers Keep trying, which sends it again', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, {
      pending: 0,
      retryable: 1,
      refused: 0,
      reason: null,
      retryReason: 'database or disk is full',
      radioLive: false,
    })
    expect(
      screen.getByText(t('quit.logbook.retry.body', { count: 1, reason: 'database or disk is full' })),
    ).toBeTruthy()
    expect(dialog()!.textContent, 'not the words for a change refused for good').not.toContain(
      t('quit.logbook.refused.body', { count: 1, reason: 'database or disk is full' }),
    )
    const keep = button('Keep trying')
    expect(keep, 'sending it again can land it, so Keep trying is on offer').not.toBeNull()
    expect(button('Quit without the last change')).not.toBeNull()
    fireEvent.click(keep!)
    await act(async () => {})
    expect(invoked).toContainEqual(['logbook_save_choice', { keepTrying: true }])
  })

  it('both kinds of refusal at once: each line names its own reason, and the quit counts both', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, {
      pending: 0,
      retryable: 1,
      refused: 1,
      reason: 'UNIQUE constraint failed',
      retryReason: 'database or disk is full',
      radioLive: false,
    })
    expect(
      screen.getByText(t('quit.logbook.retry.body', { count: 1, reason: 'database or disk is full' })),
    ).toBeTruthy()
    expect(
      screen.getByText(t('quit.logbook.refused.body', { count: 1, reason: 'UNIQUE constraint failed' })),
    ).toBeTruthy()
    expect(button('Keep trying')).not.toBeNull()
    expect(button('Quit without the last 2 changes')).not.toBeNull()
  })

  it('the save finishing hides it', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, { pending: 1, refused: 0, reason: null, radioLive: false })
    expect(dialog()).not.toBeNull()
    emit(LOGBOOK_SAVE_DONE, { saved: false })
    expect(dialog()).toBeNull()
  })

  it('Escape does not dismiss it — the station is still deciding', async () => {
    await mount()
    emit(LOGBOOK_SAVE_FAILED, { pending: 1, refused: 0, reason: null, radioLive: false })
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' })
    await act(async () => {})
    expect(dialog()).not.toBeNull()
    expect(invoked.filter(([c]) => c === 'logbook_save_choice'), 'and Escape answers nothing').toEqual([])
  })

  it('over a radio that is still running it carries Stop TX, and over a stopped one it does not', async () => {
    await mount()
    emit(LOGBOOK_SAVING, { pending: 1, radioLive: true })
    advance(SAVING_GRACE_MS)
    const stop = button(t('quit.logbook.stopTx'))
    expect(stop, 'the dialog covers every stop control, so it brings one').not.toBeNull()
    fireEvent.click(stop!)
    await act(async () => {})
    expect(invoked.map(([c]) => c)).toContain('halt_tx')

    emit(LOGBOOK_SAVE_FAILED, { pending: 1, refused: 0, reason: null, radioLive: true })
    expect(button(t('quit.logbook.stopTx')), 'the question keeps it too').not.toBeNull()

    emit(LOGBOOK_SAVE_DONE, { saved: true })
    emit(LOGBOOK_SAVING, { pending: 1, radioLive: false })
    advance(SAVING_GRACE_MS)
    expect(dialog()).not.toBeNull()
    expect(button(t('quit.logbook.stopTx')), 'control: a stopped radio has nothing to stop').toBeNull()
  })

  it('is inert without the desktop event bridge — a browser has no quit to report', async () => {
    delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
    expect(() => render(<LogbookSaving />)).not.toThrow()
    await act(async () => {})
    expect(dialog()).toBeNull()
  })
})

// CLOSED WITHOUT A QUIT, THE KEYBOARD GOES BACK TO WHERE IT WAS (focusReturn.ts) — and the return
// adds nothing that could hold a quit: one synchronous focus inside the close Radix schedules
// anyway, no timer of its own, nothing left running once the dialog is gone.
describe('closing gives the keyboard back, and leaves nothing running', () => {
  it('back to the control the operator was on, with no timer left behind', async () => {
    const { unmount } = render(<button type="button">Log it</button>)
    const where = screen.getByRole('button', { name: 'Log it' })
    act(() => where.focus())
    await mount()
    emit(LOGBOOK_SAVE_FAILED, { pending: 1, refused: 0, reason: null, radioLive: false })
    expect(dialog()).not.toBeNull()
    expect(document.activeElement, 'the question has the keyboard').not.toBe(where)
    emit(LOGBOOK_SAVE_DONE, { saved: true })
    expect(dialog()).toBeNull()
    act(() => {
      // Radix's own close, which gives the keyboard back (0 ms; so is jsdom's selectionchange for
      // the focus). A millisecond, not "every pending timer": a timer left waiting — a watch —
      // must still be pending below, not run away here.
      vi.advanceTimersByTime(1)
    })
    expect(document.activeElement).toBe(where)
    expect(vi.getTimerCount(), 'no timer outlives the dialog').toBe(0)
    unmount()
  })
})

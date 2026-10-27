// @vitest-environment jsdom
//
// R3, the rendering half. The action button used to run `toast.action?.()` and then
// `dismissToast(toast.id)` unconditionally and synchronously, so a toast retired itself the
// instant its button was pressed — whether or not the thing it promised had happened. That is
// what let the updater's Download button close the update notice on a Linux box where no
// browser ever opened (and, with the old lying backend, record the version as dismissed for
// good). Every OTHER toast in the app raises a synchronous action (Work, Answer, QSY) and must
// keep behaving exactly as it did.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, act } from '@testing-library/react'
import { Toasts } from './Toasts'
import { pushToast, dismissToast } from '../toast'

/** Clear the module-level bus between cases — it is an app-wide singleton. */
let ids: number[] = []
function push(action: () => unknown, message: string) {
  const id = pushToast(message, 'info', 0, { action, actionLabel: 'Go' })
  ids.push(id)
  return id
}

beforeEach(() => {
  ids = []
})
afterEach(() => {
  for (const id of ids) dismissToast(id)
  cleanup()
  vi.restoreAllMocks()
})

describe('a toast action button decides whether its toast closes', () => {
  it('a SYNCHRONOUS action closes it, exactly as before', async () => {
    const action = vi.fn()
    render(<Toasts />)
    push(action, 'sync toast')
    fireEvent.click(await screen.findByRole('button', { name: /Go/ }))
    expect(action).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByText('sync toast')).toBeNull())
  })

  it('an async action that SUCCEEDS closes it', async () => {
    render(<Toasts />)
    push(() => Promise.resolve(), 'ok toast')
    fireEvent.click(await screen.findByRole('button', { name: /Go/ }))
    await waitFor(() => expect(screen.queryByText('ok toast')).toBeNull())
  })

  it('an async action that FAILS leaves it on screen — the whole point (R3)', async () => {
    const action = vi.fn(() => Promise.reject(new Error('xdg-open not found')))
    render(<Toasts />)
    push(action, 'failing toast')
    fireEvent.click(await screen.findByRole('button', { name: /Go/ }))
    expect(action).toHaveBeenCalledTimes(1)
    // Let the rejection settle, then assert the toast survived it.
    await Promise.resolve()
    await new Promise((r) => setTimeout(r, 0))
    expect(screen.queryByText('failing toast')).toBeTruthy()
  })

  it('and stays clickable, so the operator can try again', async () => {
    const action = vi.fn(() => Promise.reject(new Error('still no browser')))
    render(<Toasts />)
    push(action, 'retry toast')
    const btn = await screen.findByRole('button', { name: /Go/ })
    fireEvent.click(btn)
    await new Promise((r) => setTimeout(r, 0))
    fireEvent.click(screen.getByRole('button', { name: /Go/ }))
    expect(action).toHaveBeenCalledTimes(2)
  })

  it('a failed action outlives its expiry, says why, and closes once a second press succeeds', async () => {
    // An Undo refused while transmitting: the operator stops, then presses it again — later than
    // the toast's own expiry would have allowed.
    const action = vi
      .fn()
      .mockImplementationOnce(() => Promise.reject(new Error('Stop transmitting first')))
      .mockImplementationOnce(() => Promise.resolve())
    render(<Toasts />)
    ids.push(pushToast('timed toast', 'success', 40, { action, actionLabel: 'Go' }))
    fireEvent.click(await screen.findByRole('button', { name: /Go/ }))
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'Stop transmitting first')
    await new Promise((r) => setTimeout(r, 120))
    expect(screen.queryByText('timed toast')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Go/ }))
    await waitFor(() => expect(screen.queryByText('timed toast')).toBeNull())
  })

  it('an action that outlasts its expiry keeps its toast until it answers, and then says why it failed', async () => {
    // A large purge's Undo waits for the writer for seconds: longer than the toast's own expiry.
    let refuse: (e: Error) => void = () => {}
    const action = vi.fn(() => new Promise<void>((_, reject) => { refuse = reject }))
    render(<Toasts />)
    ids.push(pushToast('slow toast', 'success', 40, { action, actionLabel: 'Go' }))
    fireEvent.click(await screen.findByRole('button', { name: /Go/ }))
    await new Promise((r) => setTimeout(r, 120))
    expect(screen.queryByText('slow toast')).toBeTruthy()
    await act(async () => refuse(new Error('Stop transmitting first')))
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'Stop transmitting first')
  })

  it('an action in flight cannot be sent again: its button is disabled until it answers', async () => {
    let answer: () => void = () => {}
    const action = vi.fn(() => new Promise<void>((resolve) => { answer = resolve }))
    render(<Toasts />)
    push(action, 'busy toast')
    const btn = await screen.findByRole('button', { name: /Go/ })
    // Two presses before the screen has redrawn: a double-click, or an operator who saw nothing happen.
    act(() => {
      btn.click()
      btn.click()
    })
    expect(action).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('button', { name: /Go/ })).toHaveProperty('disabled', true)
    await act(async () => answer())
    await waitFor(() => expect(screen.queryByText('busy toast')).toBeNull())
  })

  it('a failed action frees its button again', async () => {
    const action = vi.fn(() => Promise.reject(new Error('Stop transmitting first')))
    render(<Toasts />)
    push(action, 'freed toast')
    fireEvent.click(await screen.findByRole('button', { name: /Go/ }))
    await screen.findByRole('alert')
    expect(screen.getByRole('button', { name: /Go/ })).toHaveProperty('disabled', false)
  })
})

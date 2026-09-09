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
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
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
})

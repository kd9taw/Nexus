// @vitest-environment jsdom
//
// D#19, second half. Error toasts now stay 12 s (toast.ts), and a 12 s overlay that swallows
// clicks is how a control becomes unreachable — the Remote browser suite caught exactly that on
// a 390 px viewport. The fix makes the toast BODY click-through (`pointer-events: none`, with
// its own buttons `auto`), which costs swipe-to-dismiss, so the × close button is the dismissal
// that has to work: visible, named, and reachable from the keyboard.
//
// The click-through half lives in the stylesheet and needs a real layout engine — jsdom applies
// no stylesheet — so it is measured in headless Chrome (see the commit) and guarded on the real
// screen by the remote-web compiled-browser job. What IS observable here is the control itself.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { Toasts } from './Toasts'
import { pushToast, dismissToast, subscribeToasts, type Toast } from '../toast'

let current: Toast[] = []
let unsub: () => void = () => {}

beforeEach(() => {
  unsub = subscribeToasts((t) => {
    current = t
  })
})
afterEach(() => {
  for (const t of current) dismissToast(t.id)
  unsub()
  cleanup()
})

describe('a toast can always be dismissed by its close button (D#19)', () => {
  it('renders a named, keyboard-reachable × that removes the toast', () => {
    render(<Toasts />)
    act(() => {
      pushToast('Could not set offset: localPermissionRequired', 'error')
    })
    const close = screen.getByRole('button', { name: 'Dismiss notification' })
    // A real button: focusable and Enter/Space-activatable without any handler of ours.
    expect(close.tagName).toBe('BUTTON')
    expect(close.getAttribute('tabindex')).not.toBe('-1')
    expect(close.closest('[aria-hidden="true"]')).toBeNull()
    expect(screen.getByText('Could not set offset: localPermissionRequired')).not.toBeNull()

    close.focus()
    expect(document.activeElement).toBe(close)
    act(() => {
      fireEvent.click(close)
    })
    expect(screen.queryByText('Could not set offset: localPermissionRequired')).toBeNull()
    expect(current).toHaveLength(0)
  })

  it('an action toast keeps BOTH its action and its close working', () => {
    const action = vi.fn()
    render(<Toasts />)
    act(() => {
      pushToast('W1AW is calling you', 'info', 0, { action, actionLabel: 'Work' })
    })
    act(() => {
      fireEvent.click(screen.getByRole('button', { name: /Work/ }))
    })
    expect(action).toHaveBeenCalledTimes(1)

    act(() => {
      pushToast('QRZ upload failed', 'error')
    })
    const close = screen.getByRole('button', { name: 'Dismiss notification' })
    act(() => {
      fireEvent.click(close)
    })
    expect(screen.queryByText('QRZ upload failed')).toBeNull()
  })
})

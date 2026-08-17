// @vitest-environment jsdom
//
// The confirm dialog replaces window.confirm, which is INERT in the Tauri webview: WKWebView only
// shows a JS confirm if the host implements runJavaScriptConfirmPanel, and wry does not. So
// `confirm()` returned false with no dialog, and every `if (!window.confirm(…)) return` guard
// silently cancelled — Remove radio, Restore, Reset and eleven others did nothing at all
// (operator, 2026-08-14).
//
// What is pinned here is the property that matters most: with no host mounted it must resolve
// FALSE. A missing dialog that answered "yes" would turn this bug into silent data loss, which is
// strictly worse than the silence it replaces.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, waitFor } from '@testing-library/react'
import { confirmDialog, ConfirmHost } from './confirm'

afterEach(cleanup)

describe('confirmDialog', () => {
  it('resolves FALSE when no host is mounted — never proceeds unconfirmed', async () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {})
    await expect(confirmDialog({ title: 'Delete everything?' })).resolves.toBe(false)
    expect(err).toHaveBeenCalled() // and says so, rather than failing mutely
    err.mockRestore()
  })

  it('shows the question and resolves true only on the affirmative button', async () => {
    render(<ConfirmHost />)
    const answer = confirmDialog({
      title: 'Remove FT-710?',
      body: 'This deletes its CAT config.',
      confirmLabel: 'Remove radio',
    })
    await waitFor(() => expect(screen.getByText('Remove FT-710?')).toBeTruthy())
    expect(screen.getByText('This deletes its CAT config.')).toBeTruthy()
    screen.getByRole('button', { name: 'Remove radio' }).click()
    await expect(answer).resolves.toBe(true)
  })

  it('answers a superseded question NO instead of leaving its await hanging forever', async () => {
    render(<ConfirmHost />)
    const first = confirmDialog({ title: 'Delete radio 1?' })
    await waitFor(() => expect(screen.getByText('Delete radio 1?')).toBeTruthy())

    // A second question arrives before the first is answered — the fire-and-forget call sites in
    // RadioProgView do not await one another. The first used to be dropped unresolved, so its
    // caller waited for the lifetime of the window.
    const second = confirmDialog({ title: 'Delete radio 2?' })
    await expect(first).resolves.toBe(false)

    // And the survivor is genuinely still live, not collateral damage from settling the first.
    await waitFor(() => expect(screen.getByText('Delete radio 2?')).toBeTruthy())
    screen.getByRole('button', { name: 'Confirm' }).click()
    await expect(second).resolves.toBe(true)
  })

  it('resolves false on Cancel', async () => {
    render(<ConfirmHost />)
    const answer = confirmDialog({ title: 'Reset everything?' })
    await waitFor(() => expect(screen.getByText('Reset everything?')).toBeTruthy())
    screen.getByRole('button', { name: 'Cancel' }).click()
    await expect(answer).resolves.toBe(false)
  })
})

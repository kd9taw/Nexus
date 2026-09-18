// @vitest-environment jsdom
//
// Prove TX, end to end through its confirmation — the second of the two transmit-path guards that
// were left on `window.confirm` when the other twelve were converted.
//
// `window.confirm` is INERT in the Tauri webview (wry implements no `runJavaScriptConfirmPanel`,
// so WKWebView shows nothing and returns false). Both remaining calls were transmit-path, so
// moving them needed sign-off rather than a UI pass, and they sat for 32 days. The cost was
// exactly what `confirm.tsx` predicted: the guard FAILED CLOSED, so nothing ever keyed unasked —
// but on macOS **Prove TX was a dead button**. No dialog, no carrier, no error.
//
// ⭐ AND THE REASON IT LIVED THAT LONG IS THE REASON THIS FILE EXISTS. `confirm.tsx`'s own header
// says the suite missed the original bug because "none of the converted paths had ANY coverage —
// a guard nothing exercises cannot fail, whatever it returns". Converting these two without
// covering them would have repeated that exactly: the code would look fixed and nothing would know.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SetupHealth } from './SetupHealth'
import { ConfirmHost } from '../confirm'

const radio = { rxLevel: 0.5, txEnabled: false, tuning: false }

function mount(onProveTx: () => void) {
  return render(
    <>
      <SetupHealth radio={radio} catResult={null} onProveTx={onProveTx} />
      <ConfirmHost />
    </>,
  )
}

afterEach(cleanup)

describe('Prove TX asks first, every time', () => {
  it('shows a real dialog and keys only after the operator agrees', async () => {
    const onProveTx = vi.fn()
    mount(onProveTx)

    fireEvent.click(screen.getByRole('button', { name: 'Prove TX' }))

    // The dialog is REAL — this is the half that was dead on macOS.
    await screen.findByText('Prove the transmit path?')
    expect(screen.getByText(/keys your transmitter for ~2 seconds/i)).toBeTruthy()
    expect(onProveTx).not.toHaveBeenCalled() // nothing keys on the ASK

    // ⚠️ Scope to the DIALOG. Two buttons are named 'Prove TX' once it opens — the health
    // chip and the dialog's confirm — and `getByRole` has no `selector` option (that is
    // `getByText`); passing one type-checks as an error and is IGNORED at runtime, so the
    // query silently resolved by something else entirely. `within(role=dialog)` says which
    // button is meant instead of relying on the modal happening to hide the other one.
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Prove TX' }))
    await waitFor(() => expect(onProveTx).toHaveBeenCalledTimes(1))
  })

  it('keys nothing when the operator declines', async () => {
    const onProveTx = vi.fn()
    mount(onProveTx)

    fireEvent.click(screen.getByRole('button', { name: 'Prove TX' }))
    await screen.findByText('Prove the transmit path?')
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    await waitFor(() => expect(screen.queryByText('Prove the transmit path?')).toBeNull())
    expect(onProveTx).not.toHaveBeenCalled()
  })

  // ⛔ FAIL CLOSED, and this is the assertion that matters most. With no <ConfirmHost/> mounted
  // `confirmDialog` resolves FALSE rather than proceeding — so a missing dialog can never become a
  // transmission nobody consented to. That default is what made 32 days of this bug merely useless
  // instead of dangerous, and it must survive any future refactor of the host.
  it('never keys when no dialog host is mounted', async () => {
    const onProveTx = vi.fn()
    render(<SetupHealth radio={radio} catResult={null} onProveTx={onProveTx} />)

    fireEvent.click(screen.getByRole('button', { name: 'Prove TX' }))
    await new Promise((r) => setTimeout(r, 20))
    expect(onProveTx).not.toHaveBeenCalled()
  })
})
